//! Registre borné des processus légers.
//!
//! Ce module ne dépend pas du matériel. Il porte les invariants de cycle de
//! vie qui seront utilisés ensuite par le scheduler et le dispatcher syscall.

use crate::{
    ExecError, ExecResult, ThreadCreateArgs, ThreadExitCode, ThreadId, ThreadInfo, ThreadState,
};

pub const MAX_THREADS: usize = ThreadCreateArgs::MAX_THREADS_PER_PROCESS;
pub const MAX_PROCESSES: usize = 16;
/// Limite ABI historique conservée pour les identifiants de processus.
pub const MAX_PROCESS_ID: crate::ProcessId = 1000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessState {
    Running,
    Exiting,
    Exited,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessInfo {
    pub id: crate::ProcessId,
    pub main_thread: ThreadId,
    pub state: ProcessState,
}

#[derive(Clone, Copy, Debug)]
pub struct ProcessRegistry {
    records: [Option<ProcessInfo>; MAX_PROCESSES],
    next_id: crate::ProcessId,
}

impl Default for ProcessRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessRegistry {
    pub const fn new() -> Self {
        Self {
            records: [None; MAX_PROCESSES],
            next_id: 1,
        }
    }

    pub fn create(
        &mut self,
        id: crate::ProcessId,
        main_thread: ThreadId,
    ) -> ExecResult<ProcessInfo> {
        if id == 0
            || id >= MAX_PROCESS_ID
            || main_thread == 0
            || self
                .records
                .iter()
                .any(|record| record.is_some_and(|record| record.id == id))
        {
            return Err(ExecError::InvalidArgument);
        }
        let slot = self
            .records
            .iter()
            .position(Option::is_none)
            .ok_or(ExecError::OutOfMemory)?;
        let info = ProcessInfo {
            id,
            main_thread,
            state: ProcessState::Running,
        };
        self.records[slot] = Some(info);
        if id == self.next_id {
            self.next_id = self.next_id.saturating_add(1);
            if self.next_id >= MAX_PROCESS_ID {
                self.next_id = 1;
            }
        }
        Ok(info)
    }

    /// Réserve le prochain identifiant libre sans publier encore de
    /// processus. La réservation est consommée même si la création
    /// transactionnelle qui suit échoue.
    pub fn allocate_id(&mut self) -> ExecResult<crate::ProcessId> {
        let start = self.next_id;
        let mut candidate = start;
        loop {
            if !self
                .records
                .iter()
                .any(|record| record.is_some_and(|record| record.id == candidate))
            {
                self.next_id = candidate.saturating_add(1);
                if self.next_id >= MAX_PROCESS_ID {
                    self.next_id = 1;
                }
                return Ok(candidate);
            }
            candidate = candidate.saturating_add(1);
            if candidate >= MAX_PROCESS_ID {
                candidate = 1;
            }
            if candidate == start {
                return Err(ExecError::OutOfMemory);
            }
        }
    }

    pub fn info(&self, id: crate::ProcessId) -> ExecResult<ProcessInfo> {
        self.records
            .iter()
            .find(|record| record.is_some_and(|record| record.id == id))
            .and_then(|record| *record)
            .ok_or(ExecError::NotFound)
    }

    pub fn mark_exiting(&mut self, id: crate::ProcessId) -> ExecResult<()> {
        let record = self.find_mut(id)?;
        if record.state != ProcessState::Running {
            return Err(ExecError::NotReady);
        }
        record.state = ProcessState::Exiting;
        Ok(())
    }

    pub fn exit(&mut self, id: crate::ProcessId) -> ExecResult<()> {
        let record = self.find_mut(id)?;
        if record.state != ProcessState::Exiting {
            return Err(ExecError::NotReady);
        }
        record.state = ProcessState::Exited;
        Ok(())
    }

    pub fn remove(&mut self, id: crate::ProcessId) -> ExecResult<()> {
        let index = self
            .records
            .iter_mut()
            .position(|record| record.is_some_and(|record| record.id == id))
            .ok_or(ExecError::NotFound)?;
        if self.records[index].is_some_and(|record| record.state != ProcessState::Exited) {
            return Err(ExecError::NotReady);
        }
        self.records[index] = None;
        Ok(())
    }

    fn find_mut(&mut self, id: crate::ProcessId) -> ExecResult<&mut ProcessInfo> {
        self.records
            .iter_mut()
            .find_map(|record| record.as_mut().filter(|record| record.id == id))
            .ok_or(ExecError::NotFound)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ThreadRecord {
    info: ThreadInfo,
}

impl ThreadRecord {
    const EMPTY: Self = Self {
        info: ThreadInfo {
            id: 0,
            process: 0,
            state: ThreadState::Exited,
            exit_code: None,
        },
    };
}

/// État borné des threads d’un processus ou d’un groupe de processus.
#[derive(Clone, Debug)]
pub struct ThreadRegistry {
    next_id: ThreadId,
    records: [ThreadRecord; MAX_THREADS],
}

impl Default for ThreadRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ThreadRegistry {
    pub const fn new() -> Self {
        Self {
            next_id: 1,
            records: [ThreadRecord::EMPTY; MAX_THREADS],
        }
    }

    pub fn create(&mut self, args: ThreadCreateArgs) -> ExecResult<ThreadId> {
        args.validate()?;
        let slot = self
            .records
            .iter()
            .position(|record| record.info.id == 0)
            .ok_or(ExecError::OutOfMemory)?;
        let id = self.allocate_id()?;
        self.records[slot].info = ThreadInfo {
            id,
            process: args.process,
            state: ThreadState::Ready,
            exit_code: None,
        };
        Ok(id)
    }

    pub fn info(&self, thread: ThreadId) -> ExecResult<ThreadInfo> {
        self.find(thread).map(|record| record.info)
    }

    pub fn count_process(&self, process: crate::ProcessId) -> usize {
        self.records
            .iter()
            .filter(|record| {
                record.info.id != 0
                    && record.info.process == process
                    && record.info.state != ThreadState::Exited
            })
            .count()
    }

    /// Indique si le registre conserve encore un thread du processus, y
    /// compris un thread terminé dont le code de sortie n’a pas été rejoint.
    pub fn has_process_threads(&self, process: crate::ProcessId) -> bool {
        self.records
            .iter()
            .any(|record| record.info.id != 0 && record.info.process == process)
    }

    pub fn mark_running(&mut self, thread: ThreadId) -> ExecResult<()> {
        let record = self.find_mut(thread)?;
        if record.info.state != ThreadState::Ready {
            return Err(ExecError::NotReady);
        }
        record.info.state = ThreadState::Running;
        Ok(())
    }

    pub fn mark_blocked(&mut self, thread: ThreadId) -> ExecResult<()> {
        let record = self.find_mut(thread)?;
        if record.info.state != ThreadState::Running {
            return Err(ExecError::NotReady);
        }
        record.info.state = ThreadState::Blocked;
        Ok(())
    }

    pub fn mark_ready(&mut self, thread: ThreadId) -> ExecResult<()> {
        let record = self.find_mut(thread)?;
        // Un thread peut redevenir éligible après une attente bloquante. Le
        // passage `Blocked → Ready` est donc une transition valide, au même
        // titre que `Running → Ready` lors d'une désactivation volontaire.
        if !matches!(
            record.info.state,
            ThreadState::Running | ThreadState::Blocked
        ) {
            return Err(ExecError::NotReady);
        }
        record.info.state = ThreadState::Ready;
        Ok(())
    }

    pub fn exit(&mut self, thread: ThreadId, code: ThreadExitCode) -> ExecResult<()> {
        let record = self.find_mut(thread)?;
        if record.info.state == ThreadState::Exited {
            return Err(ExecError::NotReady);
        }
        record.info.state = ThreadState::Exited;
        record.info.exit_code = Some(code);
        Ok(())
    }

    pub fn join(
        &mut self,
        thread: ThreadId,
        deadline: Option<crate::Timestamp>,
    ) -> ExecResult<ThreadExitCode> {
        let record = self.find(thread)?;
        if record.info.state != ThreadState::Exited {
            return Err(if deadline == Some(0) {
                ExecError::Timeout
            } else {
                ExecError::NotReady
            });
        }
        let code = record.info.exit_code.ok_or(ExecError::Fault)?;
        self.remove(thread)?;
        Ok(code)
    }

    fn allocate_id(&mut self) -> ExecResult<ThreadId> {
        for _ in 0..MAX_THREADS {
            let id = self.next_id;
            self.next_id = self.next_id.checked_add(1).unwrap_or(1);
            if id != 0 && self.records.iter().all(|record| record.info.id != id) {
                return Ok(id);
            }
        }
        Err(ExecError::OutOfMemory)
    }

    fn find(&self, thread: ThreadId) -> ExecResult<&ThreadRecord> {
        self.records
            .iter()
            .find(|record| record.info.id == thread)
            .ok_or(ExecError::NotFound)
    }

    fn find_mut(&mut self, thread: ThreadId) -> ExecResult<&mut ThreadRecord> {
        self.records
            .iter_mut()
            .find(|record| record.info.id == thread)
            .ok_or(ExecError::NotFound)
    }

    fn remove(&mut self, thread: ThreadId) -> ExecResult<()> {
        let record = self
            .records
            .iter_mut()
            .find(|record| record.info.id == thread)
            .ok_or(ExecError::NotFound)?;
        *record = ThreadRecord::EMPTY;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Address, ByteCount};

    fn args() -> ThreadCreateArgs {
        ThreadCreateArgs {
            process: 7,
            entry: Address(0x1000),
            stack_size: ByteCount(4096),
            priority: 10,
            argument: 0,
        }
    }

    #[test]
    fn lifecycle_preserves_process_owner_and_exit_code() {
        let mut registry = ThreadRegistry::new();
        let thread = registry.create(args()).expect("create");
        assert_eq!(registry.info(thread).expect("info").process, 7);
        registry.mark_running(thread).expect("run");
        registry.exit(thread, -3).expect("exit");
        assert_eq!(registry.join(thread, None), Ok(-3));
        assert_eq!(registry.info(thread), Err(ExecError::NotFound));
    }

    #[test]
    fn join_of_live_thread_is_bounded() {
        let mut registry = ThreadRegistry::new();
        let thread = registry.create(args()).expect("create");
        assert_eq!(registry.join(thread, Some(0)), Err(ExecError::Timeout));
        assert_eq!(registry.join(thread, None), Err(ExecError::NotReady));
    }

    #[test]
    fn process_owns_a_main_thread_and_has_explicit_lifecycle() {
        let mut processes = ProcessRegistry::new();
        let info = processes.create(7, 11).expect("create");
        assert_eq!(info.main_thread, 11);
        assert_eq!(processes.remove(7), Err(ExecError::NotReady));
        processes.mark_exiting(7).expect("exiting");
        processes.exit(7).expect("exit");
        assert_eq!(processes.info(7).expect("info").state, ProcessState::Exited);
        processes.remove(7).expect("remove");
        assert_eq!(processes.info(7), Err(ExecError::NotFound));
    }

    #[test]
    fn process_ids_are_bounded_and_cursor_skips_live_processes() {
        let mut processes = ProcessRegistry::new();
        assert_eq!(processes.allocate_id(), Ok(1));
        processes.create(1, 11).expect("publish");
        assert_eq!(processes.allocate_id(), Ok(2));
        processes.create(2, 12).expect("publish");
        assert_eq!(processes.allocate_id(), Ok(3));

        // Un processus vivant n'est jamais réattribué, même si le curseur
        // revient sur son identifiant après un cycle.
        processes.next_id = 1;
        assert_eq!(processes.allocate_id(), Ok(3));
        assert_eq!(processes.allocate_id(), Ok(4));
        assert!(MAX_PROCESS_ID > 3);
    }
}
