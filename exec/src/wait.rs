//! Files d’attente bornées pour les threads bloqués.

use crate::{ExecError, ExecResult, ThreadId};

pub const MAX_WAITERS: usize = 64;
pub const MAX_JOIN_TARGETS: usize = 32;
pub const MAX_INPUT_WAIT_TARGETS: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WaitQueue {
    ids: [ThreadId; MAX_WAITERS],
    len: usize,
}

impl WaitQueue {
    pub const fn new() -> Self {
        Self {
            ids: [0; MAX_WAITERS],
            len: 0,
        }
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub const fn len(&self) -> usize {
        self.len
    }

    pub fn push(&mut self, thread: ThreadId) -> ExecResult<()> {
        if thread == 0 || self.ids[..self.len].contains(&thread) {
            return Err(ExecError::InvalidArgument);
        }
        if self.len == MAX_WAITERS {
            return Err(ExecError::QueueFull);
        }
        self.ids[self.len] = thread;
        self.len += 1;
        Ok(())
    }

    pub fn remove(&mut self, thread: ThreadId) -> ExecResult<()> {
        let index = self.ids[..self.len]
            .iter()
            .position(|candidate| *candidate == thread)
            .ok_or(ExecError::NotFound)?;
        self.ids.copy_within(index + 1..self.len, index);
        self.len -= 1;
        self.ids[self.len] = 0;
        Ok(())
    }

    pub fn wake_one(&mut self) -> ExecResult<ThreadId> {
        if self.is_empty() {
            return Err(ExecError::NotReady);
        }
        let thread = self.ids[0];
        self.remove(thread)?;
        Ok(thread)
    }

    /// Réveille tous les attenteurs dans leur ordre d’arrivée.
    pub fn wake_all(&mut self, mut wake: impl FnMut(ThreadId)) -> usize {
        let mut count = 0;
        while let Ok(thread) = self.wake_one() {
            wake(thread);
            count += 1;
        }
        count
    }
}

impl Default for WaitQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct JoinWaitEntry {
    target: ThreadId,
    waiters: WaitQueue,
}

/// Registre borné associant une cible de `ThreadJoin` à ses attenteurs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JoinWaitRegistry {
    entries: [Option<JoinWaitEntry>; MAX_JOIN_TARGETS],
}

impl JoinWaitRegistry {
    pub const fn new() -> Self {
        Self {
            entries: [None; MAX_JOIN_TARGETS],
        }
    }

    pub fn register(&mut self, target: ThreadId, waiter: ThreadId) -> ExecResult<()> {
        if target == 0 || waiter == 0 || target == waiter {
            return Err(ExecError::InvalidArgument);
        }
        if let Some(entry) = self
            .entries
            .iter_mut()
            .flatten()
            .find(|entry| entry.target == target)
        {
            return entry.waiters.push(waiter);
        }
        let slot = self
            .entries
            .iter_mut()
            .position(|entry| entry.is_none())
            .ok_or(ExecError::QueueFull)?;
        let mut waiters = WaitQueue::new();
        waiters.push(waiter)?;
        self.entries[slot] = Some(JoinWaitEntry { target, waiters });
        Ok(())
    }

    pub fn wake_one(&mut self, target: ThreadId) -> ExecResult<ThreadId> {
        let slot = self
            .entries
            .iter()
            .position(|entry| entry.is_some_and(|entry| entry.target == target))
            .ok_or(ExecError::NotReady)?;
        let waiter = self.entries[slot]
            .as_mut()
            .ok_or(ExecError::NotReady)?
            .waiters
            .wake_one()?;
        if self.entries[slot].is_some_and(|entry| entry.waiters.is_empty()) {
            self.entries[slot] = None;
        }
        Ok(waiter)
    }

    pub fn cancel(&mut self, target: ThreadId, waiter: ThreadId) -> ExecResult<()> {
        let slot = self
            .entries
            .iter()
            .position(|entry| entry.is_some_and(|entry| entry.target == target))
            .ok_or(ExecError::NotFound)?;
        self.entries[slot]
            .as_mut()
            .ok_or(ExecError::NotFound)?
            .waiters
            .remove(waiter)?;
        if self.entries[slot].is_some_and(|entry| entry.waiters.is_empty()) {
            self.entries[slot] = None;
        }
        Ok(())
    }
}

impl Default for JoinWaitRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Associe une fenêtre logique à ses threads qui attendent un événement.
/// L'identifiant est fourni par Intuition ; Exec ne connaît pas la fenêtre,
/// il ne fait que garantir une file bornée et un réveil FIFO.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InputWaitRegistry {
    entries: [Option<(u32, WaitQueue)>; MAX_INPUT_WAIT_TARGETS],
}

impl InputWaitRegistry {
    pub const fn new() -> Self {
        Self {
            entries: [None; MAX_INPUT_WAIT_TARGETS],
        }
    }

    pub fn len(&self, window: u32) -> usize {
        self.entries
            .iter()
            .flatten()
            .find(|(candidate, _)| *candidate == window)
            .map_or(0, |(_, waiters)| waiters.len())
    }

    pub fn register(&mut self, window: u32, thread: ThreadId) -> ExecResult<()> {
        if window == 0 || thread == 0 {
            return Err(ExecError::InvalidArgument);
        }
        if let Some((_, waiters)) = self
            .entries
            .iter_mut()
            .flatten()
            .find(|(candidate, _)| *candidate == window)
        {
            return waiters.push(thread);
        }
        let slot = self
            .entries
            .iter_mut()
            .position(|entry| entry.is_none())
            .ok_or(ExecError::QueueFull)?;
        let mut waiters = WaitQueue::new();
        waiters.push(thread)?;
        self.entries[slot] = Some((window, waiters));
        Ok(())
    }

    pub fn wake_all(&mut self, window: u32, mut wake: impl FnMut(ThreadId)) -> usize {
        let Some(slot) = self
            .entries
            .iter()
            .position(|entry| entry.is_some_and(|(candidate, _)| candidate == window))
        else {
            return 0;
        };
        let Some((_, waiters)) = self.entries[slot].as_mut() else {
            return 0;
        };
        let count = waiters.wake_all(&mut wake);
        if waiters.is_empty() {
            self.entries[slot] = None;
        }
        count
    }
}

impl Default for InputWaitRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wait_queue_is_bounded_fifo_and_duplicate_free() {
        let mut queue = WaitQueue::new();
        queue.push(3).expect("first");
        queue.push(7).expect("second");
        assert_eq!(queue.push(3), Err(ExecError::InvalidArgument));
        assert_eq!(queue.wake_one(), Ok(3));
        assert_eq!(queue.wake_one(), Ok(7));
        assert_eq!(queue.wake_one(), Err(ExecError::NotReady));
    }

    #[test]
    fn wait_queue_rejects_zero_and_unknown_removal() {
        let mut queue = WaitQueue::new();
        assert_eq!(queue.push(0), Err(ExecError::InvalidArgument));
        assert_eq!(queue.remove(9), Err(ExecError::NotFound));
    }

    #[test]
    fn wait_queue_wakes_all_in_fifo_order() {
        let mut queue = WaitQueue::new();
        queue.push(4).expect("first");
        queue.push(2).expect("second");
        let mut woken = [0; 2];
        let mut count = 0;
        assert_eq!(
            queue.wake_all(|thread| {
                woken[count] = thread;
                count += 1;
            }),
            2
        );
        assert_eq!(woken, [4, 2]);
        assert!(queue.is_empty());
    }

    #[test]
    fn join_wait_registry_groups_and_wakes_target_waiters() {
        let mut registry = JoinWaitRegistry::new();
        registry.register(9, 3).expect("first waiter");
        registry.register(9, 4).expect("second waiter");
        assert_eq!(registry.register(9, 9), Err(ExecError::InvalidArgument));
        assert_eq!(registry.wake_one(9), Ok(3));
        assert_eq!(registry.wake_one(9), Ok(4));
        assert_eq!(registry.wake_one(9), Err(ExecError::NotReady));
    }

    #[test]
    fn join_wait_registry_can_cancel_one_waiter() {
        let mut registry = JoinWaitRegistry::new();
        registry.register(9, 3).expect("first waiter");
        registry.register(9, 4).expect("second waiter");
        registry.cancel(9, 3).expect("cancel");
        assert_eq!(registry.wake_one(9), Ok(4));
        assert_eq!(registry.cancel(9, 4), Err(ExecError::NotFound));
    }

    #[test]
    fn input_wait_registry_wakes_window_waiters_in_fifo_order() {
        let mut registry = InputWaitRegistry::new();
        registry.register(4, 7).expect("first waiter");
        registry.register(4, 8).expect("second waiter");
        let mut woken = [0; 2];
        let mut count = 0;
        assert_eq!(
            registry.wake_all(4, |thread| {
                woken[count] = thread;
                count += 1;
            }),
            2
        );
        assert_eq!(woken, [7, 8]);
        assert_eq!(registry.wake_all(4, |_| {}), 0);
    }
}
