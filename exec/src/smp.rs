//! État d'exécution local à chaque hart.
//!
//! Les registres de processus, de threads et les décisions transversales du
//! scheduler restent partagés. En revanche, le hart courant, le contexte
//! publié pour le trampoline et les demandes de rescheduling appartiennent à
//! un seul hart et ne doivent pas être stockés dans le verrou global Exec.
//! Ce module fournit cette frontière sous forme de cellules atomiques, sans
//! pointeur partagé ni aliasage mutable entre harts.

use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};

use crate::scheduler::MAX_HARTS;
use crate::{ExecError, ExecResult, ThreadId};

pub const HART_BOOTING: u8 = 0;
pub const HART_READY_TO_RUN: u8 = 1;
pub const HART_USER_READY: u8 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HartContextSnapshot {
    pub thread: ThreadId,
    pub satp: u64,
    pub sepc: u64,
    pub user_stack: u64,
    pub kernel_stack: u64,
}

struct HartLocalState {
    current_thread: AtomicU64,
    satp: AtomicU64,
    sepc: AtomicU64,
    user_stack: AtomicU64,
    kernel_stack: AtomicU64,
    boot_state: AtomicU8,
    reschedule_pending: AtomicBool,
}

impl HartLocalState {
    const fn new() -> Self {
        Self {
            current_thread: AtomicU64::new(0),
            satp: AtomicU64::new(0),
            sepc: AtomicU64::new(0),
            user_stack: AtomicU64::new(0),
            kernel_stack: AtomicU64::new(0),
            boot_state: AtomicU8::new(HART_BOOTING),
            reschedule_pending: AtomicBool::new(false),
        }
    }

    fn snapshot(&self) -> HartContextSnapshot {
        HartContextSnapshot {
            thread: self.current_thread.load(Ordering::Acquire),
            satp: self.satp.load(Ordering::Acquire),
            sepc: self.sepc.load(Ordering::Acquire),
            user_stack: self.user_stack.load(Ordering::Acquire),
            kernel_stack: self.kernel_stack.load(Ordering::Acquire),
        }
    }
}

/// État atomique indépendant de chaque hart.
pub struct SmpState {
    harts: [HartLocalState; MAX_HARTS],
}

impl SmpState {
    pub const fn new() -> Self {
        Self {
            harts: [const { HartLocalState::new() }; MAX_HARTS],
        }
    }

    pub fn current_thread(&self, hart: usize) -> ExecResult<ThreadId> {
        self.hart(hart)
            .map(|state| state.current_thread.load(Ordering::Acquire))
    }

    pub fn set_current_thread(&self, hart: usize, thread: ThreadId) -> ExecResult<()> {
        self.hart(hart)?
            .current_thread
            .store(thread, Ordering::Release);
        Ok(())
    }

    /// Publie un contexte complet avec le thread comme dernier champ publié.
    /// Le trampoline lit ensuite les valeurs avec `Acquire` et ne consulte
    /// aucun registre mutable du runtime global.
    pub fn publish_context(
        &self,
        hart: usize,
        thread: ThreadId,
        satp: u64,
        sepc: u64,
        user_stack: u64,
        kernel_stack: u64,
    ) -> ExecResult<()> {
        let state = self.hart(hart)?;
        state.satp.store(satp, Ordering::Relaxed);
        state.sepc.store(sepc, Ordering::Relaxed);
        state.user_stack.store(user_stack, Ordering::Relaxed);
        state.kernel_stack.store(kernel_stack, Ordering::Relaxed);
        state.current_thread.store(thread, Ordering::Release);
        Ok(())
    }

    pub fn context_snapshot(&self, hart: usize) -> ExecResult<HartContextSnapshot> {
        Ok(self.hart(hart)?.snapshot())
    }

    pub fn boot_state(&self, hart: usize) -> ExecResult<u8> {
        Ok(self.hart(hart)?.boot_state.load(Ordering::Acquire))
    }

    pub fn set_boot_state(&self, hart: usize, state: u8) -> ExecResult<()> {
        if state > HART_USER_READY {
            return Err(ExecError::InvalidArgument);
        }
        self.hart(hart)?.boot_state.store(state, Ordering::Release);
        Ok(())
    }

    /// Retourne l'ancien état et publie le nouveau, utile pour les barrières
    /// de démarrage sans verrou inter-harts.
    pub fn swap_boot_state(&self, hart: usize, state: u8) -> ExecResult<u8> {
        if state > HART_USER_READY {
            return Err(ExecError::InvalidArgument);
        }
        Ok(self.hart(hart)?.boot_state.swap(state, Ordering::AcqRel))
    }

    pub fn request_reschedule(&self, hart: usize) -> ExecResult<()> {
        self.hart(hart)?
            .reschedule_pending
            .store(true, Ordering::Release);
        Ok(())
    }

    pub fn take_reschedule_request(&self, hart: usize) -> ExecResult<bool> {
        Ok(self
            .hart(hart)?
            .reschedule_pending
            .swap(false, Ordering::AcqRel))
    }

    fn hart(&self, hart: usize) -> ExecResult<&HartLocalState> {
        self.harts.get(hart).ok_or(ExecError::InvalidArgument)
    }
}

impl Default for SmpState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_hart_owns_an_independent_execution_snapshot() {
        let state = SmpState::new();
        state
            .publish_context(1, 11, 22, 33, 44, 55)
            .expect("hart 1");
        state
            .publish_context(2, 66, 77, 88, 99, 111)
            .expect("hart 2");

        assert_eq!(
            state.context_snapshot(1),
            Ok(HartContextSnapshot {
                thread: 11,
                satp: 22,
                sepc: 33,
                user_stack: 44,
                kernel_stack: 55,
            })
        );
        assert_eq!(state.current_thread(2), Ok(66));
        assert_eq!(state.context_snapshot(0).unwrap().thread, 0);
    }

    #[test]
    fn reschedule_requests_are_coalesced_per_hart() {
        let state = SmpState::new();
        state.request_reschedule(3).expect("request");
        state.request_reschedule(3).expect("duplicate request");
        assert_eq!(state.take_reschedule_request(3), Ok(true));
        assert_eq!(state.take_reschedule_request(3), Ok(false));
        assert_eq!(state.take_reschedule_request(4), Ok(false));
    }

    #[test]
    fn boot_state_transitions_are_bounded() {
        let state = SmpState::new();
        assert_eq!(
            state.swap_boot_state(5, HART_READY_TO_RUN),
            Ok(HART_BOOTING)
        );
        assert_eq!(state.boot_state(5), Ok(HART_READY_TO_RUN));
        assert_eq!(
            state.set_boot_state(5, HART_USER_READY + 1),
            Err(ExecError::InvalidArgument)
        );
        assert_eq!(
            state.current_thread(MAX_HARTS),
            Err(ExecError::InvalidArgument)
        );
    }
}
