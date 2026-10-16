//! Contrats de contexte CPU RISC-V utilisés par Exec.
//!
//! La disposition reprend le trap frame de la V1. Elle reste sans assembleur
//! afin d’être testable sur l’hôte avant son raccordement au trampoline QEMU.

use crate::scheduler::MAX_HARTS;
use crate::{Address, ExecError, ExecResult, ThreadId};

pub const MAX_CONTEXTS: usize = 64;
pub const STACK_ALIGNMENT: u64 = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HartBootstrapContext {
    pub hart: usize,
    pub kernel_stack: Address,
    pub trap_vector: Address,
}

#[derive(Clone, Copy, Debug)]
pub struct HartBootstrapRegistry {
    entries: [Option<HartBootstrapContext>; MAX_HARTS],
}

impl HartBootstrapRegistry {
    pub const fn new() -> Self {
        Self {
            entries: [None; MAX_HARTS],
        }
    }

    pub fn register(&mut self, context: HartBootstrapContext) -> ExecResult<()> {
        if context.hart >= MAX_HARTS {
            return Err(ExecError::InvalidArgument);
        }
        if self.entries[context.hart].is_some() {
            return Err(ExecError::InvalidArgument);
        }
        self.entries[context.hart] = Some(context);
        Ok(())
    }

    pub const fn get(&self, hart: usize) -> Option<HartBootstrapContext> {
        if hart >= MAX_HARTS {
            None
        } else {
            self.entries[hart]
        }
    }
}

impl HartBootstrapContext {
    pub fn new(hart: usize, kernel_stack: Address, trap_vector: Address) -> ExecResult<Self> {
        if hart >= MAX_HARTS
            || kernel_stack.0 == 0
            || kernel_stack.0 % STACK_ALIGNMENT != 0
            || trap_vector.0 == 0
            || trap_vector.0 % 4 != 0
        {
            return Err(ExecError::InvalidArgument);
        }
        Ok(Self {
            hart,
            kernel_stack,
            trap_vector,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StackAllocation {
    pub user_base: Address,
    pub user_top: Address,
    pub kernel_base: Address,
    pub kernel_top: Address,
    pub size: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StackAllocator {
    next_user: u64,
    next_kernel: u64,
    user_limit: u64,
    kernel_limit: u64,
}

impl StackAllocator {
    pub const fn new(
        user_base: Address,
        user_limit: Address,
        kernel_base: Address,
        kernel_limit: Address,
    ) -> ExecResult<Self> {
        if user_base.0 == 0
            || kernel_base.0 == 0
            || user_base.0 % STACK_ALIGNMENT != 0
            || kernel_base.0 % STACK_ALIGNMENT != 0
            || user_base.0 >= user_limit.0
            || kernel_base.0 >= kernel_limit.0
        {
            return Err(ExecError::InvalidArgument);
        }
        Ok(Self {
            next_user: user_base.0,
            next_kernel: kernel_base.0,
            user_limit: user_limit.0,
            kernel_limit: kernel_limit.0,
        })
    }

    pub fn allocate(&mut self, size: u64) -> ExecResult<StackAllocation> {
        if size == 0 || size % STACK_ALIGNMENT != 0 {
            return Err(ExecError::InvalidArgument);
        }
        let user_top = self
            .next_user
            .checked_add(size)
            .ok_or(ExecError::OutOfMemory)?;
        let kernel_top = self
            .next_kernel
            .checked_add(size)
            .ok_or(ExecError::OutOfMemory)?;
        if user_top > self.user_limit || kernel_top > self.kernel_limit {
            return Err(ExecError::OutOfMemory);
        }
        let allocation = StackAllocation {
            user_base: Address(self.next_user),
            user_top: Address(user_top),
            kernel_base: Address(self.next_kernel),
            kernel_top: Address(kernel_top),
            size,
        };
        self.next_user = user_top;
        self.next_kernel = kernel_top;
        Ok(allocation)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct SupervisorTrapFrame {
    pub x: [u64; 32],
    pub sepc: u64,
    pub sstatus: u64,
    pub scause: u64,
    pub stval: u64,
}

impl SupervisorTrapFrame {
    pub const fn empty() -> Self {
        Self {
            x: [0; 32],
            sepc: 0,
            sstatus: 0,
            scause: 0,
            stval: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TaskContext {
    pub trap: SupervisorTrapFrame,
    pub kernel_stack: Address,
    pub user_stack: Address,
    pub satp: u64,
    pub hart: Option<usize>,
}

impl TaskContext {
    pub const fn new(
        entry: Address,
        user_stack: Address,
        kernel_stack: Address,
        satp: u64,
    ) -> Self {
        let mut trap = SupervisorTrapFrame::empty();
        trap.sepc = entry.0;
        trap.x[2] = user_stack.0;
        // `sret` copie SPIE vers SIE. Les threads U-Mode doivent donc sortir
        // de leur premier contexte avec les interruptions autorisées.
        trap.sstatus = 1 << 5;
        Self {
            trap,
            kernel_stack,
            user_stack,
            satp,
            hart: None,
        }
    }

    pub fn assign_hart(&mut self, hart: usize, hart_count: usize) -> ExecResult<()> {
        if hart >= hart_count {
            return Err(ExecError::InvalidArgument);
        }
        self.hart = Some(hart);
        // `tp` est conservé en U-Mode comme identifiant stable du hart ; le
        // trampoline le transmet au handler pour sélectionner le bon contexte
        // SMP. Il doit donc suivre l’affectation du TaskContext.
        self.trap.x[4] = hart as u64;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ContextRegistry {
    entries: [Option<(ThreadId, TaskContext)>; MAX_CONTEXTS],
}

impl Default for ContextRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ContextRegistry {
    pub const fn new() -> Self {
        Self {
            entries: [None; MAX_CONTEXTS],
        }
    }

    pub fn insert(&mut self, thread: ThreadId, mut context: TaskContext) -> ExecResult<()> {
        if thread == 0 || context.hart.is_some_and(|hart| hart >= MAX_HARTS) {
            return Err(ExecError::InvalidArgument);
        }
        if self
            .entries
            .iter()
            .any(|entry| entry.is_some_and(|(id, _)| id == thread))
        {
            return Err(ExecError::InvalidArgument);
        }
        let slot = self
            .entries
            .iter()
            .position(Option::is_none)
            .ok_or(ExecError::OutOfMemory)?;
        if context.trap.sepc == 0 || context.user_stack.0 == 0 || context.kernel_stack.0 == 0 {
            return Err(ExecError::InvalidArgument);
        }
        context.trap.x[0] = 0;
        self.entries[slot] = Some((thread, context));
        Ok(())
    }

    pub fn get(&self, thread: ThreadId) -> ExecResult<TaskContext> {
        self.entries
            .iter()
            .find(|entry| entry.is_some_and(|(id, _)| id == thread))
            .and_then(|entry| *entry)
            .map(|(_, context)| context)
            .ok_or(ExecError::NotFound)
    }

    pub fn update_trap(&mut self, thread: ThreadId, trap: SupervisorTrapFrame) -> ExecResult<()> {
        let (_, context) = self
            .entries
            .iter_mut()
            .find_map(|entry| entry.as_mut().filter(|(id, _)| *id == thread))
            .ok_or(ExecError::NotFound)?;
        context.trap = trap;
        context.trap.x[0] = 0;
        Ok(())
    }

    pub fn assign_hart(&mut self, thread: ThreadId, hart: usize) -> ExecResult<()> {
        if hart >= MAX_HARTS {
            return Err(ExecError::InvalidArgument);
        }
        let (_, context) = self
            .entries
            .iter_mut()
            .find_map(|entry| entry.as_mut().filter(|(id, _)| *id == thread))
            .ok_or(ExecError::NotFound)?;
        context.hart = Some(hart);
        context.trap.x[4] = hart as u64;
        Ok(())
    }

    /// Sauvegarde le frame du thread interrompu et retourne celui du thread
    /// choisi par le scheduler. Le trampoline assembleur appliquera ensuite
    /// le frame retourné ; cette méthode ne touche pas aux CSR matériels.
    pub fn switch_to(
        &mut self,
        current: ThreadId,
        trap: SupervisorTrapFrame,
        next: ThreadId,
    ) -> ExecResult<SupervisorTrapFrame> {
        self.update_trap(current, trap)?;
        let context = self.get(next)?;
        if context.trap.sepc == 0 || context.user_stack.0 == 0 || context.kernel_stack.0 == 0 {
            return Err(ExecError::InvalidArgument);
        }
        Ok(context.trap)
    }

    pub fn remove(&mut self, thread: ThreadId) -> ExecResult<()> {
        let entry = self
            .entries
            .iter_mut()
            .find(|entry| entry.is_some_and(|(id, _)| id == thread))
            .ok_or(ExecError::NotFound)?;
        *entry = None;
        Ok(())
    }
}

const _: () = assert!(core::mem::size_of::<SupervisorTrapFrame>() == 36 * 8);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_matches_v1_trap_frame_shape() {
        assert_eq!(core::mem::size_of::<SupervisorTrapFrame>(), 288);
        let context = TaskContext::new(Address(0x1000), Address(0x8000), Address(0x9000), 1);
        assert_eq!(context.trap.sepc, 0x1000);
        assert_eq!(context.trap.x[2], 0x8000);
        assert_eq!(context.trap.sstatus, 1 << 5);
        assert_eq!(context.hart, None);
    }

    #[test]
    fn bootstrap_context_requires_dedicated_aligned_stack_and_trap_vector() {
        let context = HartBootstrapContext::new(3, Address(0x9000), Address(0x8000))
            .expect("valid bootstrap context");
        assert_eq!(context.hart, 3);
        assert_eq!(
            HartBootstrapContext::new(3, Address(0x9008), Address(0x8000)),
            Err(ExecError::InvalidArgument)
        );
        assert_eq!(
            HartBootstrapContext::new(3, Address(0x9000), Address(0x8002)),
            Err(ExecError::InvalidArgument)
        );
    }

    #[test]
    fn bootstrap_registry_keeps_one_context_per_hart() {
        let mut registry = HartBootstrapRegistry::new();
        let context =
            HartBootstrapContext::new(2, Address(0x9000), Address(0x8000)).expect("valid context");
        registry.register(context).expect("first registration");
        assert_eq!(registry.get(2), Some(context));
        assert_eq!(registry.register(context), Err(ExecError::InvalidArgument));
        assert_eq!(registry.get(MAX_HARTS), None);
    }

    #[test]
    fn context_rejects_hart_outside_configured_qemu_set() {
        let mut context = TaskContext::new(Address(1), Address(2), Address(3), 4);
        assert_eq!(context.assign_hart(8, 8), Err(ExecError::InvalidArgument));
        assert_eq!(context.assign_hart(7, 8), Ok(()));
    }

    #[test]
    fn registry_keeps_one_context_per_thread_and_zeroes_x0() {
        let mut registry = ContextRegistry::new();
        let context = TaskContext::new(Address(1), Address(2), Address(3), 4);
        registry.insert(9, context).expect("insert");
        assert_eq!(registry.get(9).expect("get").trap.x[0], 0);
        assert_eq!(registry.insert(9, context), Err(ExecError::InvalidArgument));
        registry.remove(9).expect("remove");
        assert_eq!(registry.get(9), Err(ExecError::NotFound));
    }

    #[test]
    fn switch_rejects_context_with_zero_program_counter() {
        let mut registry = ContextRegistry::new();
        registry
            .insert(1, TaskContext::new(Address(1), Address(2), Address(3), 0))
            .expect("current context");
        registry
            .insert(2, TaskContext::new(Address(5), Address(6), Address(7), 0))
            .expect("next context");
        let mut invalid = SupervisorTrapFrame::empty();
        invalid.sepc = 0;
        registry
            .update_trap(2, invalid)
            .expect("corrupt next frame");
        assert_eq!(
            registry.switch_to(1, SupervisorTrapFrame::empty(), 2),
            Err(ExecError::InvalidArgument)
        );
    }

    #[test]
    fn stack_allocator_reserves_aligned_user_and_kernel_ranges() {
        let mut allocator = StackAllocator::new(
            Address(0x10_000),
            Address(0x12_000),
            Address(0x20_000),
            Address(0x22_000),
        )
        .expect("allocator");
        let first = allocator.allocate(0x400).expect("first");
        let second = allocator.allocate(0x400).expect("second");

        assert_eq!(first.user_top, second.user_base);
        assert_eq!(first.kernel_top, second.kernel_base);
        assert_eq!(allocator.allocate(0x1401), Err(ExecError::InvalidArgument));
        assert!(allocator.allocate(0x1800).is_ok());
        assert_eq!(allocator.allocate(0x10), Err(ExecError::OutOfMemory));
    }

    #[test]
    fn switch_saves_current_frame_and_returns_next_frame() {
        let mut registry = ContextRegistry::new();
        registry
            .insert(1, TaskContext::new(Address(1), Address(2), Address(3), 4))
            .expect("current");
        registry
            .insert(2, TaskContext::new(Address(5), Address(6), Address(7), 8))
            .expect("next");
        let mut saved = SupervisorTrapFrame::empty();
        saved.sepc = 0x1234;
        saved.x[10] = 42;

        let next = registry.switch_to(1, saved, 2).expect("switch");

        assert_eq!(next.sepc, 5);
        assert_eq!(registry.get(1).expect("saved").trap.sepc, 0x1234);
        assert_eq!(registry.get(1).expect("saved").trap.x[10], 42);
    }
}
