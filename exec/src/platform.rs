//! Topologie des harts fournie par la plateforme.
//!
//! La découverte matérielle réelle viendra du firmware/DTB. Ce type permet
//! déjà de tester la politique « tous les harts disponibles » sous QEMU.

use crate::scheduler::{MAX_HARTS, QEMU_HARTS};
use crate::{ExecError, ExecResult};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HartTopology {
    detected: usize,
    online: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HartState {
    Offline,
    Booting,
    Online,
    Parked,
    Stopping,
    Stopped,
}

#[derive(Clone, Debug)]
pub struct HartRuntime {
    topology: HartTopology,
    states: [HartState; MAX_HARTS],
}

impl HartRuntime {
    pub const fn new(topology: HartTopology) -> Self {
        Self {
            topology,
            states: [HartState::Offline; MAX_HARTS],
        }
    }

    pub const fn qemu() -> Self {
        Self::new(HartTopology::discover(QEMU_HARTS))
    }

    pub fn boot_primary(&mut self) -> ExecResult<()> {
        self.boot_primary_on(0)
    }

    pub fn boot_primary_on(&mut self, hart: usize) -> ExecResult<()> {
        self.transition(hart, HartState::Booting)?;
        self.transition(hart, HartState::Online)
    }

    pub fn boot_secondary(&mut self, hart: usize) -> ExecResult<()> {
        if hart == 0 || hart >= self.topology.detected() {
            return Err(ExecError::InvalidArgument);
        }
        self.transition(hart, HartState::Booting)?;
        self.transition(hart, HartState::Online)
    }

    pub fn park(&mut self, hart: usize) -> ExecResult<()> {
        if self.state(hart)? != HartState::Online {
            return Err(ExecError::NotReady);
        }
        self.states[hart] = HartState::Parked;
        Ok(())
    }

    pub fn stop(&mut self, hart: usize) -> ExecResult<()> {
        if !matches!(self.state(hart)?, HartState::Online | HartState::Parked) {
            return Err(ExecError::NotReady);
        }
        self.states[hart] = HartState::Stopping;
        self.states[hart] = HartState::Stopped;
        Ok(())
    }

    pub fn state(&self, hart: usize) -> ExecResult<HartState> {
        if hart >= self.topology.detected() {
            return Err(ExecError::InvalidArgument);
        }
        Ok(self.states[hart])
    }

    pub fn online_count(&self) -> usize {
        self.states
            .iter()
            .take(self.topology.detected())
            .filter(|state| matches!(state, HartState::Online | HartState::Parked))
            .count()
    }

    fn transition(&mut self, hart: usize, expected: HartState) -> ExecResult<()> {
        if self.state(hart)? != HartState::Offline && expected == HartState::Booting {
            return Err(ExecError::NotReady);
        }
        self.states[hart] = expected;
        Ok(())
    }
}

impl HartTopology {
    pub const fn discover(detected: usize) -> Self {
        let detected = if detected > MAX_HARTS {
            MAX_HARTS
        } else {
            detected
        };
        Self {
            detected,
            online: 0,
        }
    }

    pub const fn qemu() -> Self {
        Self::discover(QEMU_HARTS).enable_all_const()
    }

    pub fn enable_all(&mut self) -> ExecResult<usize> {
        if self.detected == 0 {
            return Err(ExecError::NotReady);
        }
        self.online = self.detected;
        Ok(self.online)
    }

    pub const fn detected(&self) -> usize {
        self.detected
    }

    pub const fn online(&self) -> usize {
        self.online
    }

    pub const fn is_online(&self, hart: usize) -> bool {
        hart < self.online
    }

    const fn enable_all_const(mut self) -> Self {
        self.online = self.detected;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qemu_topology_brings_all_eight_harts_online() {
        let topology = HartTopology::qemu();
        assert_eq!(topology.detected(), 8);
        assert_eq!(topology.online(), 8);
        assert!(topology.is_online(7));
        assert!(!topology.is_online(8));
    }

    #[test]
    fn future_platform_is_bounded_and_can_enable_all_detected_harts() {
        let mut topology = HartTopology::discover(512);
        assert_eq!(topology.detected(), MAX_HARTS);
        assert_eq!(topology.enable_all(), Ok(MAX_HARTS));
    }

    #[test]
    fn qemu_hart_runtime_boots_all_harts_and_stops_one_cleanly() {
        let mut runtime = HartRuntime::qemu();
        runtime.boot_primary().expect("primary");
        for hart in 1..QEMU_HARTS {
            runtime.boot_secondary(hart).expect("secondary");
        }
        assert_eq!(runtime.online_count(), QEMU_HARTS);
        runtime.stop(7).expect("stop");
        assert_eq!(runtime.state(7), Ok(HartState::Stopped));
        assert_eq!(runtime.online_count(), QEMU_HARTS - 1);
    }
}
