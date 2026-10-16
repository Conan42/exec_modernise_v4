//! Contrat portable des appels SBI HSM utilisés pour le bring-up SMP.
//!
//! Ce module ne réalise pas l'`ecall` S-Mode : il valide et décrit la requête
//! qui sera remise au backend SBI du kernel.

use crate::{Address, ExecError, ExecResult};

pub const EXTENSION_HSM: usize = 0x4853_4d;
pub const EXTENSION_IPI: usize = 0x7350_49;
/// SBI System Reset, utilisé par le kernel après la fermeture de l'application
/// graphique dans le profil QEMU.
pub const EXTENSION_SYSTEM_RESET: usize = 0x5352_5354;
pub const SYSTEM_RESET: usize = 0;
pub const SYSTEM_RESET_SHUTDOWN: usize = 0;
pub const HART_START: usize = 0;
pub const HART_STOP: usize = 1;
pub const HART_GET_STATUS: usize = 2;
pub const HART_SUSPEND: usize = 3;
pub const HART_STATUS_STARTED: usize = 0;
pub const HART_STATUS_STOPPED: usize = 1;
pub const HART_STATUS_START_PENDING: usize = 2;
pub const HART_STATUS_STOP_PENDING: usize = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IpiHartMask {
    /// Masque scalaire des harts, transmis directement dans `a0`.
    pub mask: u64,
    pub hart_mask_base: u64,
}

impl IpiHartMask {
    pub fn validate(self) -> ExecResult<()> {
        if self.mask == 0 && self.hart_mask_base != u64::MAX {
            return Err(ExecError::InvalidArgument);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HartStartRequest {
    pub hart_id: u64,
    pub start_address: Address,
    pub opaque: u64,
}

impl HartStartRequest {
    pub fn validate(self) -> ExecResult<()> {
        if self.hart_id == u64::MAX
            || self.start_address.0 == 0
            || !self.start_address.0.is_multiple_of(4)
        {
            return Err(ExecError::InvalidArgument);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SbiHsmCall {
    pub function: usize,
    pub hart_id: u64,
    pub start_address: Address,
    pub opaque: u64,
}

impl SbiHsmCall {
    pub fn hart_start(request: HartStartRequest) -> ExecResult<Self> {
        request.validate()?;
        Ok(Self {
            function: HART_START,
            hart_id: request.hart_id,
            start_address: request.start_address,
            opaque: request.opaque,
        })
    }

    pub fn hart_get_status(hart_id: u64) -> ExecResult<Self> {
        if hart_id == u64::MAX {
            return Err(ExecError::InvalidArgument);
        }
        Ok(Self {
            function: HART_GET_STATUS,
            hart_id,
            start_address: Address(0),
            opaque: 0,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SbiIpiCall {
    pub function: usize,
    pub hart_mask: IpiHartMask,
}

impl SbiIpiCall {
    pub fn send(hart_mask: IpiHartMask) -> ExecResult<Self> {
        hart_mask.validate()?;
        Ok(Self {
            function: 0,
            hart_mask,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(isize)]
pub enum SbiError {
    Failed = -1,
    NotSupported = -2,
    InvalidParam = -3,
    Denied = -4,
    InvalidAddress = -5,
    AlreadyAvailable = -6,
    AlreadyStarted = -7,
    AlreadyStopped = -8,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hart_start_builds_hsm_call_without_executing_firmware() {
        let call = SbiHsmCall::hart_start(HartStartRequest {
            hart_id: 7,
            start_address: Address(0x8020_0000),
            opaque: 0x1234,
        })
        .expect("valid HSM request");
        assert_eq!(call.function, HART_START);
        assert_eq!(call.hart_id, 7);
        assert_eq!(call.start_address, Address(0x8020_0000));
    }

    #[test]
    fn hart_start_rejects_zero_or_unaligned_entry() {
        for address in [Address(0), Address(3)] {
            assert_eq!(
                SbiHsmCall::hart_start(HartStartRequest {
                    hart_id: 1,
                    start_address: address,
                    opaque: 0,
                }),
                Err(ExecError::InvalidArgument)
            );
        }
    }

    #[test]
    fn ipi_call_requires_nonzero_scalar_mask() {
        let call = SbiIpiCall::send(IpiHartMask {
            mask: 1,
            hart_mask_base: 0,
        })
        .expect("valid mask");
        assert_eq!(call.function, 0);
        assert_eq!(
            SbiIpiCall::send(IpiHartMask {
                mask: 0,
                hart_mask_base: 0
            }),
            Err(ExecError::InvalidArgument)
        );
    }
}
