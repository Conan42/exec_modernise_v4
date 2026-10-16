//! Contrat de disposition du trampoline RV64.
//!
//! L’assembleur sera raccordé dans la crate noyau `no_std` dédiée aux cibles
//! RISC-V. Ces constantes vérifient dès maintenant le contrat partagé avec
//! Rust, sans introduire d’accès privilégié dans la crate portable.

use crate::context::SupervisorTrapFrame;

pub const TRAP_VECTOR_SYMBOL: &str = "exec_trap_vector";

pub struct TrapFrameLayout;

impl TrapFrameLayout {
    pub const REGISTER_BASE: usize = 0;
    pub const SEPC: usize = 32 * 8;
    pub const SSTATUS: usize = Self::SEPC + 8;
    pub const SCAUSE: usize = Self::SSTATUS + 8;
    pub const STVAL: usize = Self::SCAUSE + 8;
    pub const SIZE: usize = Self::STVAL + 8;

    pub const fn is_compatible() -> bool {
        core::mem::size_of::<SupervisorTrapFrame>() == Self::SIZE
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReturnMode {
    User,
    Supervisor,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TrampolineContract {
    pub saves_all_registers: bool,
    pub restores_all_registers: bool,
    pub returns_with_sret: bool,
    pub return_mode: ReturnMode,
}

impl TrampolineContract {
    pub const USER: Self = Self {
        saves_all_registers: true,
        restores_all_registers: true,
        returns_with_sret: true,
        return_mode: ReturnMode::User,
    };
}

const _: () = assert!(TrapFrameLayout::is_compatible());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_matches_rv64_context() {
        assert_eq!(TrapFrameLayout::REGISTER_BASE, 0);
        assert_eq!(TrapFrameLayout::SEPC, 256);
        assert_eq!(TrapFrameLayout::SSTATUS, 264);
        assert_eq!(TrapFrameLayout::SCAUSE, 272);
        assert_eq!(TrapFrameLayout::STVAL, 280);
        assert_eq!(TrapFrameLayout::SIZE, 288);
        assert!(TrapFrameLayout::is_compatible());
    }

    #[test]
    fn user_contract_requires_full_save_restore_and_sret() {
        assert_eq!(
            TrampolineContract::USER,
            TrampolineContract {
                saves_all_registers: true,
                restores_all_registers: true,
                returns_with_sret: true,
                return_mode: ReturnMode::User,
            }
        );
    }
}
