//! File d’événements inter-harts pour le profil QEMU.
//!
//! Le backend matériel remplacera `send` par une écriture dans le contrôleur
//! d’interruptions, mais conservera cette sémantique bornée.

use crate::scheduler::{MAX_HARTS, QEMU_HARTS};
use crate::{Address, ExecError, ExecResult, ThreadId};

pub const IPI_QUEUE_CAPACITY: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IpiEvent {
    Wake(ThreadId),
    Reschedule,
    TlbShootdown { address_space: u64, page: Address },
    Stop,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IpiMessage {
    pub source: usize,
    pub event: IpiEvent,
}

#[derive(Clone, Copy, Debug)]
struct IpiMailbox {
    entries: [Option<IpiMessage>; IPI_QUEUE_CAPACITY],
    head: usize,
    tail: usize,
    len: usize,
}

impl IpiMailbox {
    const EMPTY: Self = Self {
        entries: [None; IPI_QUEUE_CAPACITY],
        head: 0,
        tail: 0,
        len: 0,
    };

    fn push(&mut self, message: IpiMessage) -> ExecResult<()> {
        if self.len == IPI_QUEUE_CAPACITY {
            return Err(ExecError::QueueFull);
        }
        self.entries[self.tail] = Some(message);
        self.tail = (self.tail + 1) % IPI_QUEUE_CAPACITY;
        self.len += 1;
        Ok(())
    }

    fn pop(&mut self) -> Option<IpiMessage> {
        let message = self.entries[self.head].take()?;
        self.head = (self.head + 1) % IPI_QUEUE_CAPACITY;
        self.len -= 1;
        Some(message)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct IpiController {
    hart_count: usize,
    mailboxes: [IpiMailbox; MAX_HARTS],
}

impl IpiController {
    pub const fn new(hart_count: usize) -> Self {
        Self {
            hart_count: if hart_count > MAX_HARTS {
                MAX_HARTS
            } else {
                hart_count
            },
            mailboxes: [IpiMailbox::EMPTY; MAX_HARTS],
        }
    }

    pub const fn qemu() -> Self {
        Self::new(QEMU_HARTS)
    }

    pub fn send(&mut self, source: usize, target: usize, event: IpiEvent) -> ExecResult<()> {
        if source >= self.hart_count || target >= self.hart_count {
            return Err(ExecError::InvalidArgument);
        }
        self.mailboxes[target].push(IpiMessage { source, event })
    }

    pub fn receive(&mut self, target: usize) -> ExecResult<Option<IpiMessage>> {
        if target >= self.hart_count {
            return Err(ExecError::InvalidArgument);
        }
        Ok(self.mailboxes[target].pop())
    }

    pub fn pending(&self, target: usize) -> ExecResult<usize> {
        if target >= self.hart_count {
            return Err(ExecError::InvalidArgument);
        }
        Ok(self.mailboxes[target].len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qemu_ipi_reaches_each_target_hart() {
        let mut ipi = IpiController::qemu();
        for target in 0..QEMU_HARTS {
            ipi.send(0, target, IpiEvent::Reschedule).expect("send");
        }
        for target in 0..QEMU_HARTS {
            assert_eq!(
                ipi.receive(target).expect("receive").unwrap().event,
                IpiEvent::Reschedule
            );
        }
    }

    #[test]
    fn ipi_queue_is_bounded_and_preserves_fifo_order() {
        let mut ipi = IpiController::new(2);
        for id in 1..=IPI_QUEUE_CAPACITY as u64 {
            ipi.send(0, 1, IpiEvent::Wake(id)).expect("send");
        }
        assert_eq!(ipi.send(0, 1, IpiEvent::Stop), Err(ExecError::QueueFull));
        assert_eq!(
            ipi.receive(1).expect("receive"),
            Some(IpiMessage {
                source: 0,
                event: IpiEvent::Wake(1)
            })
        );
    }
}
