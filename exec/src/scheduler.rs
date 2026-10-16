//! Ordonnanceur portable par hart.
//!
//! Ce modèle ne commute pas encore de registres CPU. Il valide cependant les
//! règles de sélection, d’affinité et de migration qui seront utilisées par
//! l’ordonnanceur matériel.

use crate::{ExecError, ExecResult, ThreadId};

/// Capacité de conception du modèle portable ; le nombre réellement utilisé
/// est fourni par la plateforme au démarrage.
pub const MAX_HARTS: usize = 64;
pub const QEMU_HARTS: usize = 8;
pub const MAX_RUNNABLE_THREADS: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HartMask(pub u64);

impl HartMask {
    pub const fn single(hart: usize) -> Self {
        let mut mask = 0;
        if hart < MAX_HARTS {
            mask = 1u64 << hart;
        }
        Self(mask)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CpuAffinity {
    Any,
    Mask(HartMask),
}

impl CpuAffinity {
    pub fn allows(self, hart: usize) -> bool {
        hart < MAX_HARTS
            && match self {
                Self::Any => true,
                Self::Mask(mask) => mask.0 & (1 << hart) != 0,
            }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SchedulableThread {
    pub id: ThreadId,
    pub priority: i8,
    pub affinity: CpuAffinity,
    pub hart: Option<usize>,
}

#[derive(Clone, Copy, Debug)]
struct ReadyQueue {
    ids: [ThreadId; MAX_RUNNABLE_THREADS],
    len: usize,
}

impl ReadyQueue {
    const EMPTY: Self = Self {
        ids: [0; MAX_RUNNABLE_THREADS],
        len: 0,
    };

    fn push(&mut self, id: ThreadId) -> ExecResult<()> {
        if self.len == MAX_RUNNABLE_THREADS || self.ids[..self.len].contains(&id) {
            return Err(ExecError::QueueFull);
        }
        self.ids[self.len] = id;
        self.len += 1;
        Ok(())
    }

    fn remove(&mut self, id: ThreadId) -> ExecResult<()> {
        let index = self.ids[..self.len]
            .iter()
            .position(|candidate| *candidate == id)
            .ok_or(ExecError::NotFound)?;
        self.ids.copy_within(index + 1..self.len, index);
        self.len -= 1;
        self.ids[self.len] = 0;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HartQueueSnapshot {
    pub hart: usize,
    pub online: bool,
    pub load: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SchedulerOperationScope {
    Local { hart: usize },
    CrossHart { source: usize, destination: usize },
}

#[derive(Clone, Debug)]
pub struct Scheduler {
    hart_count: usize,
    online: [bool; MAX_HARTS],
    threads: [Option<SchedulableThread>; MAX_RUNNABLE_THREADS],
    ready: [ReadyQueue; MAX_HARTS],
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl Scheduler {
    pub const fn new() -> Self {
        Self::with_hart_count(MAX_HARTS)
    }

    pub const fn with_hart_count(hart_count: usize) -> Self {
        Self {
            hart_count: if hart_count > MAX_HARTS {
                MAX_HARTS
            } else {
                hart_count
            },
            online: [const { true }; MAX_HARTS],
            threads: [None; MAX_RUNNABLE_THREADS],
            ready: [ReadyQueue::EMPTY; MAX_HARTS],
        }
    }

    pub const fn qemu() -> Self {
        Self::with_hart_count(QEMU_HARTS)
    }

    pub const fn hart_count(&self) -> usize {
        self.hart_count
    }

    pub fn set_hart_online(&mut self, hart: usize, online: bool) -> ExecResult<()> {
        if hart >= self.hart_count {
            return Err(ExecError::InvalidArgument);
        }
        if !online
            && self
                .threads
                .iter()
                .flatten()
                .any(|thread| thread.hart == Some(hart))
        {
            return Err(ExecError::NotReady);
        }
        self.online[hart] = online;
        Ok(())
    }

    pub fn set_all_harts_offline(&mut self) -> ExecResult<()> {
        if self.threads.iter().any(Option::is_some) {
            return Err(ExecError::NotReady);
        }
        for hart in 0..self.hart_count {
            self.online[hart] = false;
        }
        Ok(())
    }

    pub const fn is_hart_online(&self, hart: usize) -> bool {
        hart < self.hart_count && self.online[hart]
    }

    /// Vue bornée de la file locale d'un hart. Elle ne révèle pas les
    /// identifiants globaux et fixe l'interface qui sera produite sous le
    /// futur verrou local.
    pub fn hart_snapshot(&self, hart: usize) -> ExecResult<HartQueueSnapshot> {
        let ready = self.ready.get(hart).ok_or(ExecError::InvalidArgument)?;
        Ok(HartQueueSnapshot {
            hart,
            online: hart < self.hart_count && self.online[hart],
            load: ready.len,
        })
    }

    pub fn enqueue(
        &mut self,
        id: ThreadId,
        priority: i8,
        affinity: CpuAffinity,
    ) -> ExecResult<usize> {
        if id == 0
            || !self
                .threads
                .iter()
                .all(|thread| thread.map_or(true, |t| t.id != id))
        {
            return Err(ExecError::InvalidArgument);
        }
        let hart = self.select_hart(affinity)?;
        let slot = self
            .threads
            .iter()
            .position(Option::is_none)
            .ok_or(ExecError::QueueFull)?;
        self.threads[slot] = Some(SchedulableThread {
            id,
            priority,
            affinity,
            hart: Some(hart),
        });
        if let Err(error) = self.ready[hart].push(id) {
            self.threads[slot] = None;
            return Err(error);
        }
        Ok(hart)
    }

    /// Publie directement un thread sur un hart déjà choisi par le kernel.
    /// Cette primitive est réservée au bootstrap matériel ; l’affinité est
    /// toujours vérifiée avant toute publication.
    pub fn enqueue_on_hart(
        &mut self,
        id: ThreadId,
        priority: i8,
        affinity: CpuAffinity,
        hart: usize,
    ) -> ExecResult<usize> {
        if id == 0
            || hart >= self.hart_count
            || !affinity.allows(hart)
            || self.threads.iter().flatten().any(|thread| thread.id == id)
        {
            return Err(ExecError::InvalidArgument);
        }
        let slot = self
            .threads
            .iter()
            .position(Option::is_none)
            .ok_or(ExecError::QueueFull)?;
        self.online[hart] = true;
        self.threads[slot] = Some(SchedulableThread {
            id,
            priority,
            affinity,
            hart: Some(hart),
        });
        if let Err(error) = self.ready[hart].push(id) {
            self.threads[slot] = None;
            return Err(error);
        }
        Ok(hart)
    }

    pub fn pick_next(&self, hart: usize) -> ExecResult<ThreadId> {
        if !self.is_hart_online(hart) {
            return Err(ExecError::InvalidArgument);
        }
        self.ready[hart].ids[..self.ready[hart].len]
            .iter()
            .filter_map(|id| {
                self.threads
                    .iter()
                    .flatten()
                    .find(|thread| thread.id == *id)
            })
            .max_by_key(|thread| (thread.priority, core::cmp::Reverse(thread.id)))
            .map(|thread| thread.id)
            .ok_or(ExecError::NotReady)
    }

    pub fn pick_next_after(&self, hart: usize, current: ThreadId) -> ExecResult<ThreadId> {
        if !self.is_hart_online(hart) {
            return Err(ExecError::InvalidArgument);
        }
        self.ready[hart].ids[..self.ready[hart].len]
            .iter()
            .filter(|id| **id != current)
            .filter_map(|id| {
                self.threads
                    .iter()
                    .flatten()
                    .find(|thread| thread.id == *id)
            })
            .max_by_key(|thread| (thread.priority, core::cmp::Reverse(thread.id)))
            .map(|thread| thread.id)
            .ok_or(ExecError::NotReady)
    }

    pub fn load(&self, hart: usize) -> ExecResult<usize> {
        if !self.is_hart_online(hart) {
            return Err(ExecError::InvalidArgument);
        }
        Ok(self.ready[hart].len)
    }

    /// Retourne les attributs nécessaires pour retirer puis réinsérer un
    /// thread après une attente : priorité, affinité et hart courant.
    pub fn scheduling_info(&self, id: ThreadId) -> ExecResult<(i8, CpuAffinity, usize)> {
        let thread = self
            .threads
            .iter()
            .flatten()
            .find(|thread| thread.id == id)
            .ok_or(ExecError::NotFound)?;
        Ok((
            thread.priority,
            thread.affinity,
            thread.hart.ok_or(ExecError::NotReady)?,
        ))
    }

    /// Classe un déplacement avant toute mutation. Les opérations locales
    /// restent confinées à la file d'un hart ; une opération inter-harts doit
    /// conserver un ordre transactionnel global.
    pub fn migration_scope(
        &self,
        id: ThreadId,
        destination: usize,
    ) -> ExecResult<SchedulerOperationScope> {
        if !self.is_hart_online(destination) {
            return Err(ExecError::InvalidArgument);
        }
        let (_, affinity, source) = self.scheduling_info(id)?;
        if !affinity.allows(destination) {
            return Err(ExecError::PermissionDenied);
        }
        if source == destination {
            Ok(SchedulerOperationScope::Local { hart: source })
        } else {
            Ok(SchedulerOperationScope::CrossHart {
                source,
                destination,
            })
        }
    }

    pub fn deschedule(&mut self, id: ThreadId) -> ExecResult<()> {
        let (_, _, hart) = self.scheduling_info(id)?;
        self.deschedule_on_hart(id, hart)
    }

    /// Retire une entrée d'une file locale après vérification de son
    /// propriétaire. Le hart est fourni explicitement pour préparer la
    /// synchronisation dédiée de cette file.
    pub fn deschedule_on_hart(&mut self, id: ThreadId, hart: usize) -> ExecResult<()> {
        let (_, _, owner) = self.scheduling_info(id)?;
        if owner != hart {
            return Err(ExecError::InvalidArgument);
        }
        self.ready[hart].remove(id)
    }

    pub fn enqueue_existing(&mut self, id: ThreadId) -> ExecResult<usize> {
        let (_, _, hart) = self.scheduling_info(id)?;
        self.enqueue_existing_on_hart(id, hart)
    }

    /// Réinsère une entrée dans la file locale qui en est propriétaire.
    pub fn enqueue_existing_on_hart(&mut self, id: ThreadId, hart: usize) -> ExecResult<usize> {
        let (_, _, owner) = self.scheduling_info(id)?;
        if owner != hart {
            return Err(ExecError::InvalidArgument);
        }
        self.ready[hart].push(id)?;
        Ok(hart)
    }

    pub fn least_loaded_hart(&self, excluded: usize) -> ExecResult<usize> {
        (0..self.hart_count)
            .filter(|hart| *hart != excluded && self.is_hart_online(*hart))
            .min_by_key(|hart| self.load(*hart).unwrap_or(MAX_RUNNABLE_THREADS + 1))
            .ok_or(ExecError::NotReady)
    }

    pub fn thread_ids_on_hart(
        &self,
        hart: usize,
    ) -> ExecResult<([ThreadId; MAX_RUNNABLE_THREADS], usize)> {
        if !self.is_hart_online(hart) {
            return Err(ExecError::InvalidArgument);
        }
        let mut ids = [0; MAX_RUNNABLE_THREADS];
        let mut count = 0;
        for id in &self.ready[hart].ids[..self.ready[hart].len] {
            ids[count] = *id;
            count += 1;
        }
        Ok((ids, count))
    }

    pub fn migrate(&mut self, id: ThreadId, destination: usize) -> ExecResult<()> {
        let source = match self.migration_scope(id, destination)? {
            SchedulerOperationScope::Local { .. } => return Ok(()),
            SchedulerOperationScope::CrossHart { source, .. } => source,
        };
        let thread = self
            .threads
            .iter_mut()
            .filter_map(Option::as_mut)
            .find(|thread| thread.id == id)
            .ok_or(ExecError::NotFound)?;
        if thread.hart != Some(source) {
            return Err(ExecError::NotReady);
        }
        self.ready[source].remove(id)?;
        if let Err(error) = self.ready[destination].push(id) {
            let _ = self.ready[source].push(id);
            return Err(error);
        }
        thread.hart = Some(destination);
        Ok(())
    }

    pub fn remove(&mut self, id: ThreadId) -> ExecResult<()> {
        let thread = self
            .threads
            .iter_mut()
            .find(|thread| thread.is_some_and(|thread| thread.id == id))
            .ok_or(ExecError::NotFound)?;
        let hart = thread
            .and_then(|thread| thread.hart)
            .ok_or(ExecError::NotReady)?;
        self.ready[hart].remove(id)?;
        *thread = None;
        Ok(())
    }

    fn select_hart(&self, affinity: CpuAffinity) -> ExecResult<usize> {
        (0..self.hart_count)
            .filter(|hart| self.is_hart_online(*hart) && affinity.allows(*hart))
            .min_by_key(|hart| self.ready[*hart].len)
            .ok_or(ExecError::PermissionDenied)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distributes_unpinned_threads_across_harts() {
        let mut scheduler = Scheduler::with_hart_count(2);
        let first = scheduler.enqueue(1, 10, CpuAffinity::Any).expect("first");
        let second = scheduler.enqueue(2, 10, CpuAffinity::Any).expect("second");
        assert_eq!(first, 0);
        assert_eq!(second, 1);
    }

    #[test]
    fn priority_and_affinity_control_selection_and_migration() {
        let mut scheduler = Scheduler::with_hart_count(2);
        scheduler
            .enqueue(1, 1, CpuAffinity::Mask(HartMask::single(1)))
            .expect("pinned");
        scheduler.enqueue(2, 9, CpuAffinity::Any).expect("worker");
        assert_eq!(scheduler.pick_next(1), Ok(1));
        assert_eq!(scheduler.migrate(1, 0), Err(ExecError::PermissionDenied));
        scheduler.migrate(2, 1).expect("migration");
        assert_eq!(scheduler.pick_next(1), Ok(2));
    }

    #[test]
    fn offline_hart_is_not_selected_or_used_for_migration() {
        let mut scheduler = Scheduler::with_hart_count(2);
        scheduler.set_hart_online(1, false).expect("offline");
        assert!(!scheduler.is_hart_online(1));
        assert_eq!(scheduler.pick_next(1), Err(ExecError::InvalidArgument));
        scheduler
            .enqueue(1, 1, CpuAffinity::Any)
            .expect("online hart");
        assert_eq!(scheduler.migrate(1, 1), Err(ExecError::InvalidArgument));
    }

    #[test]
    fn online_hart_cannot_be_taken_offline_with_a_thread() {
        let mut scheduler = Scheduler::with_hart_count(1);
        scheduler.enqueue(1, 1, CpuAffinity::Any).expect("thread");
        assert_eq!(
            scheduler.set_hart_online(0, false),
            Err(ExecError::NotReady)
        );
    }

    #[test]
    fn scheduling_info_preserves_waiter_attributes() {
        let mut scheduler = Scheduler::with_hart_count(2);
        scheduler
            .enqueue(7, 12, CpuAffinity::Mask(HartMask::single(1)))
            .expect("thread");
        assert_eq!(
            scheduler.scheduling_info(7),
            Ok((12, CpuAffinity::Mask(HartMask::single(1)), 1))
        );
    }

    #[test]
    fn existing_thread_can_leave_and_reenter_ready_queue() {
        let mut scheduler = Scheduler::with_hart_count(1);
        scheduler.enqueue(7, 12, CpuAffinity::Any).expect("thread");
        scheduler.deschedule(7).expect("block");
        assert_eq!(scheduler.pick_next(0), Err(ExecError::NotReady));
        assert_eq!(scheduler.enqueue_existing(7), Ok(0));
        assert_eq!(scheduler.pick_next(0), Ok(7));
    }

    #[test]
    fn qemu_profile_uses_all_eight_harts() {
        let mut scheduler = Scheduler::qemu();
        for id in 1..=QEMU_HARTS as u64 {
            assert_eq!(
                scheduler.enqueue(id, 1, CpuAffinity::Any),
                Ok((id - 1) as usize)
            );
        }
        assert_eq!(scheduler.hart_count(), QEMU_HARTS);
        for hart in 0..QEMU_HARTS {
            assert!(scheduler.pick_next(hart).is_ok());
        }
    }

    #[test]
    fn hart_queue_snapshot_exposes_only_local_queue_state() {
        let mut scheduler = Scheduler::with_hart_count(2);
        scheduler
            .enqueue(1, 1, CpuAffinity::Mask(HartMask::single(1)))
            .expect("thread");

        assert_eq!(
            scheduler.hart_snapshot(1),
            Ok(HartQueueSnapshot {
                hart: 1,
                online: true,
                load: 1,
            })
        );
        scheduler.remove(1).expect("remove");
        scheduler.set_hart_online(1, false).expect("offline");
        assert_eq!(
            scheduler.hart_snapshot(1),
            Ok(HartQueueSnapshot {
                hart: 1,
                online: false,
                load: 0,
            })
        );
        assert_eq!(
            scheduler.hart_snapshot(MAX_HARTS),
            Err(ExecError::InvalidArgument)
        );
    }

    #[test]
    fn migration_scope_distinguishes_local_and_cross_hart_operations() {
        let mut scheduler = Scheduler::with_hart_count(2);
        scheduler.enqueue(1, 1, CpuAffinity::Any).expect("thread");
        assert_eq!(
            scheduler.migration_scope(1, 0),
            Ok(SchedulerOperationScope::Local { hart: 0 })
        );
        assert_eq!(
            scheduler.migration_scope(1, 1),
            Ok(SchedulerOperationScope::CrossHart {
                source: 0,
                destination: 1,
            })
        );
        assert_eq!(
            scheduler.deschedule_on_hart(1, 1),
            Err(ExecError::InvalidArgument)
        );
        scheduler.deschedule_on_hart(1, 0).expect("local queue");
        scheduler.enqueue_existing_on_hart(1, 0).expect("reinsert");
    }
}
