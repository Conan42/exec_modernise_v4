//! Orchestration portable du registre de threads et du scheduler.

use crate::context::{ContextRegistry, StackAllocator, SupervisorTrapFrame, TaskContext};
use crate::mmu::{
    AddressSpacePlan, CowWriteResolution, FrameAllocator, MappingPlan, PAGE_SIZE, PageCopier,
    PagePermissions, ProcessAddressSpaceRegistry, ReleasedCowFrames, SharedFrameReferences,
    UserPageRange,
};
use crate::process::{ProcessInfo, ProcessRegistry, ThreadRegistry};
use crate::scheduler::{CpuAffinity, Scheduler};
use crate::wait::{InputWaitRegistry, JoinWaitRegistry};
use crate::{
    Address, ByteCount, ExecError, ExecResult, ProcessId, ThreadCreateArgs, ThreadExitCode,
    ThreadId, ThreadInfo, Timestamp,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScheduleDecision {
    pub previous: ThreadId,
    pub next: ThreadId,
    pub switched: bool,
}

#[derive(Clone, Debug)]
pub struct ExecRuntime {
    pub processes: ProcessRegistry,
    pub threads: ThreadRegistry,
    pub scheduler: Scheduler,
    pub contexts: ContextRegistry,
    pub join_waits: JoinWaitRegistry,
    pub input_waits: InputWaitRegistry,
    pub address_spaces: ProcessAddressSpaceRegistry,
    thread_stacks: StackAllocator,
}

impl Default for ExecRuntime {
    fn default() -> Self {
        Self::qemu()
    }
}

impl ExecRuntime {
    pub const fn qemu() -> Self {
        Self::with_qemu_stack_layout(
            Address(0x4000_0000),
            Address(0x4100_0000),
            Address(0x5000_0000),
            Address(0x5100_0000),
        )
    }

    /// Construit le runtime QEMU avec des plages de piles fournies par le
    /// linker. Le bring-up bare-metal peut ainsi utiliser des adresses
    /// identitaires réellement présentes en RAM, tandis que les tests hôte
    /// conservent le profil virtuel historique via [`Self::qemu`].
    pub const fn with_qemu_stack_layout(
        user_base: Address,
        user_limit: Address,
        kernel_base: Address,
        kernel_limit: Address,
    ) -> Self {
        Self {
            processes: ProcessRegistry::new(),
            threads: ThreadRegistry::new(),
            scheduler: Scheduler::qemu(),
            contexts: ContextRegistry::new(),
            join_waits: JoinWaitRegistry::new(),
            input_waits: InputWaitRegistry::new(),
            address_spaces: ProcessAddressSpaceRegistry::new(),
            thread_stacks: match StackAllocator::new(
                user_base,
                user_limit,
                kernel_base,
                kernel_limit,
            ) {
                Ok(stacks) => stacks,
                Err(_) => panic!("invalid QEMU thread stack layout"),
            },
        }
    }

    /// Crée un thread avec ses deux piles et son contexte initial. Le `satp`
    /// est fourni par le propriétaire de l’espace d’adressage ; zéro reste
    /// le mode de bring-up bare actuel.
    pub fn create_thread_for_process(
        &mut self,
        args: ThreadCreateArgs,
        affinity: CpuAffinity,
        satp: u64,
    ) -> ExecResult<(ThreadId, usize)> {
        let checkpoint = self.thread_stacks;
        let allocation = self.thread_stacks.allocate(args.stack_size.0 as u64)?;
        let result = self.create_thread_with_context(
            args,
            affinity,
            TaskContext::new(args.entry, allocation.user_top, allocation.kernel_top, satp),
        );
        if result.is_err() {
            self.thread_stacks = checkpoint;
        }
        result
    }

    pub fn create_thread(
        &mut self,
        args: ThreadCreateArgs,
        affinity: CpuAffinity,
    ) -> ExecResult<(ThreadId, usize)> {
        let thread = self.threads.create(args)?;
        match self.scheduler.enqueue(thread, args.priority, affinity) {
            Ok(hart) => Ok((thread, hart)),
            Err(error) => {
                // La création est transactionnelle : aucune entrée de thread
                // ne reste réservée si l’affectation au scheduler échoue.
                let _ = self.threads.exit(thread, -1);
                let _ = self.threads.join(thread, Some(1));
                Err(error)
            }
        }
    }

    pub fn create_thread_on_hart(
        &mut self,
        args: ThreadCreateArgs,
        affinity: CpuAffinity,
        hart: usize,
    ) -> ExecResult<(ThreadId, usize)> {
        let thread = self.threads.create(args)?;
        if let Err(error) = self
            .scheduler
            .enqueue_on_hart(thread, args.priority, affinity, hart)
        {
            let _ = self.threads.exit(thread, -1);
            let _ = self.threads.join(thread, Some(1));
            return Err(error);
        }
        Ok((thread, hart))
    }

    pub fn create_thread_with_context(
        &mut self,
        args: ThreadCreateArgs,
        affinity: CpuAffinity,
        mut context: TaskContext,
    ) -> ExecResult<(ThreadId, usize)> {
        let (thread, hart) = self.create_thread(args, affinity)?;
        if let Err(error) = context.assign_hart(hart, self.scheduler.hart_count()) {
            let _ = self.exit(thread, -1);
            let _ = self.threads.join(thread, Some(1));
            return Err(error);
        }
        if let Err(error) = self.attach_context(thread, context) {
            let _ = self.exit(thread, -1);
            let _ = self.threads.join(thread, Some(1));
            return Err(error);
        }
        Ok((thread, hart))
    }

    /// Compose une réservation de piles virtuelle avec la création
    /// transactionnelle du thread. Le mapping physique reste à la charge du
    /// backend MMU.
    pub fn create_thread_with_stacks(
        &mut self,
        args: ThreadCreateArgs,
        affinity: CpuAffinity,
        stacks: &mut StackAllocator,
        satp: u64,
    ) -> ExecResult<(ThreadId, usize)> {
        let checkpoint = *stacks;
        let allocation = stacks.allocate(args.stack_size.0 as u64)?;
        let result = self.create_thread_with_context(
            args,
            affinity,
            TaskContext::new(args.entry, allocation.user_top, allocation.kernel_top, satp),
        );
        if result.is_err() {
            *stacks = checkpoint;
        }
        result
    }

    pub fn create_process(
        &mut self,
        process: ProcessId,
        entry: Address,
        stack_size: ByteCount,
        priority: i8,
        argument: u64,
        affinity: CpuAffinity,
    ) -> ExecResult<ProcessInfo> {
        let thread = self.threads.create(ThreadCreateArgs {
            process,
            entry,
            stack_size,
            priority,
            argument,
        })?;
        match self.scheduler.enqueue(thread, priority, affinity) {
            Ok(_) => {}
            Err(error) => {
                let _ = self.threads.exit(thread, -1);
                let _ = self.threads.join(thread, Some(1));
                return Err(error);
            }
        };
        match self.processes.create(process, thread) {
            Ok(info) => Ok(info),
            Err(error) => {
                let _ = self.scheduler.remove(thread);
                let _ = self.threads.exit(thread, -1);
                let _ = self.threads.join(thread, Some(1));
                Err(error)
            }
        }
    }

    /// Crée un processus en attribuant automatiquement un identifiant Exec.
    /// Le chargeur fournit encore le point d'entrée et les paramètres de
    /// lancement ; cette méthode ne construit pas à elle seule l'espace Sv39.
    pub fn create_process_auto(
        &mut self,
        entry: Address,
        stack_size: ByteCount,
        priority: i8,
        argument: u64,
        affinity: CpuAffinity,
    ) -> ExecResult<ProcessInfo> {
        let process = self.processes.allocate_id()?;
        self.create_process(process, entry, stack_size, priority, argument, affinity)
    }

    /// Crée un processus avec son espace d’adressage. L’opération reste
    /// transactionnelle : si le `satp` ne peut pas être enregistré, le thread
    /// principal et le processus nouvellement créés sont retirés.
    pub fn create_process_with_address_space(
        &mut self,
        process: ProcessId,
        entry: Address,
        stack_size: ByteCount,
        priority: i8,
        argument: u64,
        affinity: CpuAffinity,
        satp: u64,
    ) -> ExecResult<ProcessInfo> {
        let info = self.create_process(process, entry, stack_size, priority, argument, affinity)?;
        if let Err(error) = self.address_spaces.register(process, satp) {
            let _ = self.exit_owned(process, info.main_thread, -1);
            let _ = self.join(process, info.main_thread, None);
            let _ = self.reap_process(process);
            return Err(error);
        }
        Ok(info)
    }

    /// Publie un processus dont le chargeur a déjà préparé son plan de
    /// mappings utilisateur. Le `satp` est validé et enregistré avec le plan
    /// dans la même transaction de publication logique.
    pub fn create_process_with_address_space_plan(
        &mut self,
        process: ProcessId,
        entry: Address,
        stack_size: ByteCount,
        priority: i8,
        argument: u64,
        affinity: CpuAffinity,
        satp: u64,
        plan: AddressSpacePlan,
    ) -> ExecResult<ProcessInfo> {
        let info = self.create_process(process, entry, stack_size, priority, argument, affinity)?;
        if let Err(error) = self.address_spaces.register_plan(process, satp, plan) {
            let _ = self.exit_owned(process, info.main_thread, -1);
            let _ = self.join(process, info.main_thread, None);
            let _ = self.reap_process(process);
            return Err(error);
        }
        Ok(info)
    }

    /// Variante automatique pour le chemin de chargement ExecFS/MFFS.
    pub fn create_process_auto_with_address_space_plan(
        &mut self,
        entry: Address,
        stack_size: ByteCount,
        priority: i8,
        argument: u64,
        affinity: CpuAffinity,
        satp: u64,
        plan: AddressSpacePlan,
    ) -> ExecResult<ProcessInfo> {
        let process = self.processes.allocate_id()?;
        self.create_process_with_address_space_plan(
            process, entry, stack_size, priority, argument, affinity, satp, plan,
        )
    }

    /// Crée et publie un processus dont l'image et les frames ont déjà été
    /// préparées par ExecFS/ELF.
    ///
    /// Cette variante est le point d'entrée du chargeur matériel : elle
    /// réserve une pile utilisateur privée, l'initialise, attache les deux
    /// piles du thread principal à son contexte et enregistre le plan avec le
    /// `satp` correspondant. Toute erreur restaure l'allocateur, le plan et
    /// les réservations de piles avant la publication du processus.
    pub fn create_process_auto_loaded<W: crate::syscall_dispatch::ProcessImagePageWriter>(
        &mut self,
        entry: Address,
        stack_size: ByteCount,
        priority: i8,
        argument: u64,
        affinity: CpuAffinity,
        satp: u64,
        mut plan: AddressSpacePlan,
        allocator: &mut FrameAllocator,
        writer: &mut W,
    ) -> ExecResult<ProcessInfo> {
        let args = ThreadCreateArgs {
            process: 1,
            entry,
            stack_size,
            priority,
            argument,
        };
        args.validate()?;
        let allocator_checkpoint = *allocator;
        let stacks_checkpoint = self.thread_stacks;

        let result = (|| {
            let allocation = self.thread_stacks.allocate(stack_size.0 as u64)?;
            let stack_pages = (stack_size.0 as u64)
                .checked_add(PAGE_SIZE - 1)
                .ok_or(ExecError::OutOfMemory)?
                / PAGE_SIZE;
            let stack_frames = allocator.allocate(stack_pages as usize)?;
            for page in 0..stack_frames.count {
                writer.zero_page(Address(stack_frames.base + page as u64 * PAGE_SIZE))?;
            }
            let stack_range = UserPageRange::new(allocation.user_base, stack_size.0 as u64)?;
            plan.add(MappingPlan::new(
                stack_range,
                stack_frames.base,
                PagePermissions::USER_STACK,
            )?)?;

            let process = self.processes.allocate_id()?;
            let mut process_args = args;
            process_args.process = process;
            let context = TaskContext::new(entry, allocation.user_top, allocation.kernel_top, satp);
            self.create_process_with_context_and_plan(
                process,
                process_args,
                affinity,
                context,
                satp,
                plan,
            )
        })();

        if result.is_err() {
            *allocator = allocator_checkpoint;
            self.thread_stacks = stacks_checkpoint;
        }
        result
    }

    fn create_process_with_context_and_plan(
        &mut self,
        process: ProcessId,
        args: ThreadCreateArgs,
        affinity: CpuAffinity,
        mut context: TaskContext,
        satp: u64,
        plan: AddressSpacePlan,
    ) -> ExecResult<ProcessInfo> {
        let info = self.create_process(
            process,
            args.entry,
            args.stack_size,
            args.priority,
            args.argument,
            affinity,
        )?;
        let hart = self.scheduler.scheduling_info(info.main_thread)?.2;
        if let Err(error) = context.assign_hart(hart, self.scheduler.hart_count()) {
            let _ = self.exit(info.main_thread, -1);
            let _ = self.threads.join(info.main_thread, None);
            return Err(error);
        }
        if let Err(error) = self.attach_context(info.main_thread, context) {
            let _ = self.exit(info.main_thread, -1);
            let _ = self.threads.join(info.main_thread, None);
            return Err(error);
        }
        if let Err(error) = self.address_spaces.register_plan(process, satp, plan) {
            let _ = self.exit_owned(process, info.main_thread, -1);
            let _ = self.threads.join(info.main_thread, None);
            let _ = self.reap_process(process);
            return Err(error);
        }
        Ok(info)
    }

    pub fn register_process(
        &mut self,
        process: ProcessId,
        main_thread: ThreadId,
    ) -> ExecResult<ProcessInfo> {
        self.processes.create(process, main_thread)
    }

    pub fn process_info(&self, process: ProcessId) -> ExecResult<ProcessInfo> {
        self.processes.info(process)
    }

    pub fn register_process_address_space(
        &mut self,
        process: ProcessId,
        satp: u64,
    ) -> ExecResult<()> {
        self.processes.info(process)?;
        self.address_spaces.register(process, satp)
    }

    pub fn register_process_address_space_plan(
        &mut self,
        process: ProcessId,
        satp: u64,
        plan: AddressSpacePlan,
    ) -> ExecResult<()> {
        self.processes.info(process)?;
        self.address_spaces.register_plan(process, satp, plan)
    }

    pub fn process_satp(&self, process: ProcessId) -> ExecResult<u64> {
        self.address_spaces.satp(process)
    }

    pub fn clone_process_address_space(
        &mut self,
        parent: ProcessId,
        child: ProcessId,
        child_satp: u64,
    ) -> ExecResult<()> {
        self.processes.info(parent)?;
        self.processes.info(child)?;
        self.address_spaces.clone_plan(parent, child, child_satp)
    }

    /// Clone l’espace d’un processus avec partage COW des pages utilisateur
    /// inscriptibles. Le registre de références est fourni par le propriétaire
    /// mémoire et l’opération reste transactionnelle.
    pub fn clone_process_address_space_cow(
        &mut self,
        parent: ProcessId,
        child: ProcessId,
        child_satp: u64,
        references: &mut SharedFrameReferences,
    ) -> ExecResult<()> {
        self.processes.info(parent)?;
        self.processes.info(child)?;
        self.address_spaces
            .clone_plan_cow(parent, child, child_satp, references)
    }

    /// Crée un processus enfant puis clone l’espace du parent en partage COW.
    ///
    /// Le thread principal de l’enfant est créé par Exec, mais son contexte
    /// CPU et sa racine Sv39 matérielle doivent encore être attachés par le
    /// chargeur avec `child_satp`. Toute erreur de clonage retire le processus
    /// enfant et restitue les références déjà préparées.
    pub fn create_process_with_cow_address_space(
        &mut self,
        parent: ProcessId,
        child: ProcessId,
        entry: Address,
        stack_size: ByteCount,
        priority: i8,
        argument: u64,
        affinity: CpuAffinity,
        child_satp: u64,
        references: &mut SharedFrameReferences,
    ) -> ExecResult<ProcessInfo> {
        if parent == child {
            return Err(ExecError::InvalidArgument);
        }
        self.processes.info(parent)?;
        let info = self.create_process(child, entry, stack_size, priority, argument, affinity)?;
        if let Err(error) = self
            .address_spaces
            .clone_plan_cow(parent, child, child_satp, references)
        {
            let _ = self.exit_owned(child, info.main_thread, -1);
            let _ = self.join(child, info.main_thread, None);
            let _ = self.reap_process(child);
            return Err(error);
        }
        Ok(info)
    }

    /// Résout le fault d’écriture COW d’un processus après validation de son
    /// identité. Le propriétaire mémoire fournit l’allocateur, les références
    /// et le copieur physique du backend.
    pub fn resolve_process_cow_write_with_copy<C: PageCopier>(
        &mut self,
        process: ProcessId,
        references: &mut SharedFrameReferences,
        allocator: &mut FrameAllocator,
        virtual_address: Address,
        copier: &mut C,
    ) -> ExecResult<CowWriteResolution> {
        self.processes.info(process)?;
        self.address_spaces.resolve_cow_write_with_copy(
            process,
            references,
            allocator,
            virtual_address,
            copier,
        )
    }

    /// Libère l’enregistrement d’un processus terminé après récupération de
    /// tous ses threads. Le code de sortie des threads reste disponible via
    /// `join` jusqu’à leur récupération.
    pub fn reap_process(&mut self, process: ProcessId) -> ExecResult<()> {
        let info = self.processes.info(process)?;
        if info.state != crate::process::ProcessState::Exited
            || self.threads.has_process_threads(process)
        {
            return Err(ExecError::NotReady);
        }
        self.processes.remove(process)?;
        // Les processus historiques peuvent ne pas encore avoir d’espace
        // enregistré ; dans ce cas l’absence est normale.
        let _ = self.address_spaces.remove(process);
        Ok(())
    }

    /// Reape un processus et retire ses références COW. Les frames dont le
    /// dernier détenteur disparaît sont retournées au backend, qui décide de
    /// leur restitution effective à l’allocateur physique.
    pub fn reap_process_with_cow_references(
        &mut self,
        process: ProcessId,
        references: &mut SharedFrameReferences,
    ) -> ExecResult<ReleasedCowFrames> {
        let info = self.processes.info(process)?;
        if info.state != crate::process::ProcessState::Exited
            || self.threads.has_process_threads(process)
        {
            return Err(ExecError::NotReady);
        }
        let released = match self
            .address_spaces
            .remove_with_references(process, references)
        {
            Ok(released) => released,
            // Les processus legacy peuvent ne pas posséder d’espace enregistré.
            Err(ExecError::NotFound) => ReleasedCowFrames::new(),
            Err(error) => return Err(error),
        };
        self.processes.remove(process)?;
        Ok(released)
    }

    pub fn info(&self, thread: ThreadId) -> ExecResult<ThreadInfo> {
        self.threads.info(thread)
    }

    pub fn attach_context(&mut self, thread: ThreadId, context: TaskContext) -> ExecResult<()> {
        self.contexts.insert(thread, context)
    }

    pub fn mark_running(&mut self, thread: ThreadId) -> ExecResult<()> {
        self.threads.mark_running(thread)
    }

    pub fn mark_blocked(&mut self, thread: ThreadId) -> ExecResult<()> {
        self.threads.mark_blocked(thread)
    }

    /// Bloque un thread courant et le retire de sa file sans perdre ses
    /// attributs scheduler.
    pub fn block_thread(&mut self, thread: ThreadId) -> ExecResult<()> {
        let (_, _, hart) = self.scheduler.scheduling_info(thread)?;
        self.threads.mark_blocked(thread)?;
        if let Err(error) = self.scheduler.deschedule_on_hart(thread, hart) {
            // Le thread courant est normalement déjà absent de Ready. Cette
            // situation est valide après une activation explicite par le
            // kernel ; elle ne doit pas annuler l'enregistrement WAIT_INPUT.
            if error == ExecError::NotFound {
                return Ok(());
            }
            let _ = self.threads.mark_running(thread);
            return Err(error);
        }
        Ok(())
    }

    /// Réinsère un thread bloqué dans sa file d’origine.
    pub fn wake_thread(&mut self, thread: ThreadId) -> ExecResult<usize> {
        let (_, _, hart) = self.scheduler.scheduling_info(thread)?;
        self.threads.mark_ready(thread)?;
        match self.scheduler.enqueue_existing_on_hart(thread, hart) {
            Ok(hart) => Ok(hart),
            Err(error) => {
                let _ = self.threads.mark_blocked(thread);
                Err(error)
            }
        }
    }

    /// Bloque un thread jusqu'à l'arrivée d'un événement pour une fenêtre.
    /// L'enregistrement précède le retrait de la file Ready afin qu'une IRQ
    /// concurrente ne puisse pas perdre le réveil.
    pub fn wait_for_input(&mut self, window: u32, thread: ThreadId) -> ExecResult<()> {
        self.input_waits.register(window, thread)?;
        if let Err(error) = self.block_thread(thread) {
            let _ = self.input_waits.wake_all(window, |_| {});
            return Err(error);
        }
        Ok(())
    }

    /// Réveille les attenteurs d'une fenêtre après avoir placé l'événement
    /// dans sa FIFO. Le callback n'expose aucun détail de scheduler à Intuition.
    pub fn wake_input_waiters(&mut self, window: u32) -> usize {
        let mut woken = 0;
        let mut threads = [0; crate::wait::MAX_WAITERS];
        let mut count = 0;
        self.input_waits.wake_all(window, |thread| {
            if count < threads.len() {
                threads[count] = thread;
                count += 1;
            }
        });
        for thread in threads[..count].iter().copied() {
            if self.wake_thread(thread).is_ok() {
                woken += 1;
            }
        }
        woken
    }

    /// Déplace atomiquement un thread prêt et son contexte vers un hart moins
    /// chargé. Un thread `Running` n’est jamais retiré sous le pied du hart.
    pub fn rebalance_once(&mut self, source: usize, destination: usize) -> ExecResult<ThreadId> {
        if source == destination
            || self.scheduler.load(source)? <= self.scheduler.load(destination)?
        {
            return Err(ExecError::NotReady);
        }
        let (candidates, count) = self.scheduler.thread_ids_on_hart(source)?;
        for thread in candidates[..count].iter().copied() {
            if self.info(thread)?.state != crate::ThreadState::Ready {
                continue;
            }
            // Un candidat peut être prêt mais épinglé sur son hart. Il ne
            // doit pas empêcher l’examen des candidats suivants, notamment
            // un thread `Any` qui peut réellement contribuer à l’équilibrage.
            if let Err(error) = self.scheduler.migrate(thread, destination) {
                if error == ExecError::PermissionDenied {
                    continue;
                }
                return Err(error);
            }
            if let Err(error) = self.contexts.assign_hart(thread, destination) {
                let _ = self.scheduler.migrate(thread, source);
                return Err(error);
            }
            return Ok(thread);
        }
        Err(ExecError::NotReady)
    }

    /// Tente un rééquilibrage borné depuis le hart indiqué vers le hart en
    /// ligne le moins chargé.
    pub fn rebalance_from(&mut self, source: usize) -> ExecResult<(ThreadId, usize)> {
        let destination = self.scheduler.least_loaded_hart(source)?;
        let thread = self.rebalance_once(source, destination)?;
        Ok((thread, destination))
    }

    pub fn timer_tick(&mut self, hart: usize, current: ThreadId) -> ExecResult<ScheduleDecision> {
        let current_info = self.info(current)?;
        if current_info.state == crate::ThreadState::Running {
            self.threads.mark_ready(current)?;
        }
        let next = self
            .scheduler
            .pick_next_after(hart, current)
            .or_else(|_| self.scheduler.pick_next(hart))?;
        if next != current {
            self.threads.mark_running(next)?;
        } else if self.info(next)?.state == crate::ThreadState::Ready {
            self.threads.mark_running(next)?;
        }
        Ok(ScheduleDecision {
            previous: current,
            next,
            switched: next != current,
        })
    }

    /// Sauvegarde le frame courant et prépare le frame du thread sélectionné.
    /// La restauration effective reste effectuée par le trampoline assembleur.
    pub fn reschedule(
        &mut self,
        hart: usize,
        current: ThreadId,
        trap: SupervisorTrapFrame,
    ) -> ExecResult<(ScheduleDecision, SupervisorTrapFrame)> {
        let next = self
            .scheduler
            .pick_next_after(hart, current)
            .or_else(|_| self.scheduler.pick_next(hart))?;
        self.contexts.get(current)?;
        self.contexts.get(next)?;
        let decision = self.timer_tick(hart, current)?;
        // Quand le scheduler reconduit le même thread, le frame courant est
        // déjà celui à restaurer. Éviter un aller-retour dans le registre de
        // contextes conserve notamment les CSR et la pile interrompue tels
        // qu'ils ont été capturés par le trampoline.
        if decision.next == current {
            return Ok((decision, trap));
        }
        let next_trap = self.contexts.switch_to(current, trap, decision.next)?;
        Ok((decision, next_trap))
    }

    pub fn exit(&mut self, thread: ThreadId, code: ThreadExitCode) -> ExecResult<()> {
        self.threads.exit(thread, code)?;
        self.scheduler.remove(thread)?;
        let _ = self.contexts.remove(thread);
        // Un seul attenteur peut finalement récupérer le code de sortie ; le
        // réveil FIFO évite de réveiller inutilement des concurrents qui
        // échoueraient ensuite au reaping.
        if let Ok(waiter) = self.join_waits.wake_one(thread) {
            let _ = self.wake_thread(waiter);
        }
        Ok(())
    }

    /// Termine le thread courant après avoir contrôlé son propriétaire.
    pub fn exit_owned(
        &mut self,
        process: ProcessId,
        thread: ThreadId,
        code: ThreadExitCode,
    ) -> ExecResult<()> {
        if self.info(thread)?.process != process {
            return Err(ExecError::PermissionDenied);
        }
        let process_info = self.processes.info(process).ok();
        self.exit(thread, code)?;
        if process_info.is_some_and(|info| {
            thread == info.main_thread && info.state == crate::process::ProcessState::Running
        }) {
            self.processes.mark_exiting(process)?;
        }
        if self.threads.count_process(process) == 0
            && self
                .processes
                .info(process)
                .is_ok_and(|info| info.state == crate::process::ProcessState::Exiting)
        {
            self.processes.exit(process)?;
        }
        Ok(())
    }

    /// Retire le thread courant et retourne directement le contexte du
    /// prochain thread prêt. Contrairement à `reschedule`, aucun frame du
    /// thread sortant n’est sauvegardé puisqu’il n’est plus exécutable.
    pub fn schedule_after_exit(&mut self, hart: usize) -> ExecResult<SupervisorTrapFrame> {
        let next = self.scheduler.pick_next(hart)?;
        self.threads.mark_running(next)?;
        self.contexts.get(next).map(|context| context.trap)
    }

    pub fn join(
        &mut self,
        caller: ProcessId,
        thread: ThreadId,
        deadline: Option<Timestamp>,
    ) -> ExecResult<ThreadExitCode> {
        if self.info(thread)?.process != caller {
            return Err(ExecError::PermissionDenied);
        }
        self.threads.join(thread, deadline)
    }

    /// Attend une cible de join. Si elle est encore vivante, l’appelant est
    /// enregistré puis bloqué ; il devra retenter `join` après son réveil pour
    /// récupérer atomiquement le code de sortie.
    pub fn join_or_block(
        &mut self,
        caller: ProcessId,
        caller_thread: ThreadId,
        target: ThreadId,
        deadline: Option<Timestamp>,
    ) -> ExecResult<Option<ThreadExitCode>> {
        if caller_thread == target {
            return Err(ExecError::InvalidArgument);
        }
        if self.info(target)?.process != caller {
            return Err(ExecError::PermissionDenied);
        }
        if let Ok(code) = self.threads.join(target, Some(0)) {
            return Ok(Some(code));
        }
        if deadline == Some(0) {
            return Err(ExecError::Timeout);
        }
        self.join_waits.register(target, caller_thread)?;
        if let Err(error) = self.block_thread(caller_thread) {
            let _ = self.join_waits.cancel(target, caller_thread);
            return Err(error);
        }
        Ok(None)
    }

    pub fn pick_next(&self, hart: usize) -> ExecResult<ThreadId> {
        self.scheduler.pick_next(hart)
    }

    pub fn thread_count(&self, process: ProcessId) -> usize {
        self.threads.count_process(process)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduler::HartMask;
    use crate::{Address, ByteCount};

    fn args(process: ProcessId, priority: i8) -> ThreadCreateArgs {
        ThreadCreateArgs {
            process,
            entry: Address(0x1000),
            stack_size: ByteCount(4096),
            priority,
            argument: 0,
        }
    }

    #[test]
    fn creation_assigns_hart_and_exit_removes_runnable_thread() {
        let mut runtime = ExecRuntime::default();
        let (thread, hart) = runtime
            .create_thread(args(7, 10), CpuAffinity::Mask(HartMask::single(3)))
            .expect("create");
        assert_eq!(hart, 3);
        assert_eq!(runtime.pick_next(3), Ok(thread));
        runtime.exit(thread, 0).expect("exit");
        assert_eq!(runtime.pick_next(3), Err(ExecError::NotReady));
    }

    #[test]
    fn automatic_process_creation_publishes_a_main_thread() {
        let mut runtime = ExecRuntime::default();
        let info = runtime
            .create_process_auto(Address(0x1000), ByteCount(4096), 1, 0, CpuAffinity::Any)
            .expect("process");
        assert_eq!(info.id, 1);
        assert_eq!(info.main_thread, 1);
        assert_eq!(runtime.process_info(info.id), Ok(info));
    }

    #[test]
    fn join_cannot_reap_another_process_thread() {
        let mut runtime = ExecRuntime::default();
        let (thread, _) = runtime
            .create_thread(args(7, 1), CpuAffinity::Any)
            .expect("create");
        runtime.exit(thread, 4).expect("exit");
        assert_eq!(
            runtime.join(8, thread, None),
            Err(ExecError::PermissionDenied)
        );
        assert_eq!(runtime.join(7, thread, None), Ok(4));
    }

    #[test]
    fn owned_exit_removes_thread_and_keeps_exit_code_for_join() {
        let mut runtime = ExecRuntime::default();
        let (thread, _) = runtime
            .create_thread(args(7, 1), CpuAffinity::Any)
            .expect("create");

        assert_eq!(
            runtime.exit_owned(8, thread, 12),
            Err(ExecError::PermissionDenied)
        );
        assert_eq!(runtime.info(thread).expect("still alive").process, 7);

        runtime.exit_owned(7, thread, 12).expect("exit");
        assert_eq!(runtime.join(7, thread, None), Ok(12));
    }

    #[test]
    fn main_exit_transitions_process_until_last_thread_exits() {
        let mut runtime = ExecRuntime::qemu();
        let process = runtime
            .create_process(7, Address(0x1000), ByteCount(4096), 1, 0, CpuAffinity::Any)
            .expect("process");
        let (child, _) = runtime
            .create_thread(args(7, 2), CpuAffinity::Any)
            .expect("child");

        runtime
            .exit_owned(7, process.main_thread, 0)
            .expect("main exit");
        assert_eq!(
            runtime.process_info(7).expect("process").state,
            crate::process::ProcessState::Exiting
        );

        runtime.exit_owned(7, child, 0).expect("child exit");
        assert_eq!(
            runtime.process_info(7).expect("process").state,
            crate::process::ProcessState::Exited
        );
        assert_eq!(runtime.reap_process(7), Err(ExecError::NotReady));
        runtime
            .join(7, process.main_thread, None)
            .expect("join main");
        runtime.join(7, child, None).expect("join child");
        runtime.reap_process(7).expect("reap process");
        assert_eq!(runtime.process_info(7), Err(ExecError::NotFound));
    }

    #[test]
    fn schedule_after_exit_returns_next_context() {
        let mut runtime = ExecRuntime::default();
        let (first, _) = runtime
            .create_thread_with_context(
                args(7, 1),
                CpuAffinity::Mask(HartMask::single(0)),
                TaskContext::new(Address(0x1000), Address(0x2000), Address(0x3000), 0),
            )
            .expect("first");
        let (second, _) = runtime
            .create_thread_with_context(
                args(7, 2),
                CpuAffinity::Mask(HartMask::single(0)),
                TaskContext::new(Address(0x4000), Address(0x5000), Address(0x6000), 0),
            )
            .expect("second");
        runtime.mark_running(first).expect("running");
        runtime.exit_owned(7, first, 0).expect("exit");

        let next = runtime.schedule_after_exit(0).expect("next context");

        assert_eq!(next.sepc, 0x4000);
        assert_eq!(
            runtime.info(second).expect("second info").state,
            crate::ThreadState::Running
        );
    }

    #[test]
    fn process_creation_creates_and_schedules_its_main_thread() {
        let mut runtime = ExecRuntime::qemu();
        let process = runtime
            .create_process(7, Address(0x1000), ByteCount(4096), 10, 0, CpuAffinity::Any)
            .expect("process");
        assert_eq!(process.main_thread, 1);
        assert_eq!(runtime.process_info(7).expect("info"), process);
        assert_eq!(runtime.info(1).expect("thread").process, 7);
        assert_eq!(runtime.pick_next(0), Ok(1));
    }

    #[test]
    fn process_creation_binds_address_space_transactionally() {
        let mut runtime = ExecRuntime::qemu();
        let satp = (crate::mmu::SV39_MODE << 60) | 1;
        let process = runtime
            .create_process_with_address_space(
                7,
                Address(0x1000),
                ByteCount(4096),
                0,
                0,
                CpuAffinity::Any,
                satp,
            )
            .expect("process");
        assert_eq!(runtime.process_satp(process.id), Ok(satp));
        runtime
            .exit_owned(process.id, process.main_thread, 0)
            .expect("exit");
        assert_eq!(runtime.join(process.id, process.main_thread, None), Ok(0));
        runtime.reap_process(process.id).expect("reap");
        assert_eq!(runtime.process_satp(process.id), Err(ExecError::NotFound));

        assert_eq!(
            runtime.create_process_with_address_space(
                8,
                Address(0x2000),
                ByteCount(4096),
                0,
                0,
                CpuAffinity::Any,
                0,
            ),
            Err(ExecError::InvalidArgument)
        );
        assert_eq!(runtime.process_info(8), Err(ExecError::NotFound));
    }

    #[test]
    fn process_creation_publishes_prepared_address_space_plan() {
        let mut runtime = ExecRuntime::qemu();
        let mut plan = AddressSpacePlan::new();
        plan.add_private_page(
            Address(0x10000),
            Address(0x8000_0000),
            crate::mmu::PagePermissions {
                read: true,
                write: false,
                execute: true,
                user: true,
            },
        )
        .expect("mapping");
        let satp = (crate::mmu::SV39_MODE << 60) | 1;
        let info = runtime
            .create_process_auto_with_address_space_plan(
                Address(0x10000),
                ByteCount(96 * 1024),
                0,
                0,
                CpuAffinity::Any,
                satp,
                plan,
            )
            .expect("process");

        assert_eq!(info.id, 1);
        assert_eq!(runtime.process_satp(info.id), Ok(satp));
        assert_eq!(runtime.address_spaces.plan(info.id), Ok(plan));
    }

    #[test]
    fn process_creation_clones_parent_address_space_with_cow() {
        let mut runtime = ExecRuntime::qemu();
        let parent = runtime
            .create_process(7, Address(0x1000), ByteCount(4096), 0, 0, CpuAffinity::Any)
            .expect("parent");
        let parent_satp = (crate::mmu::SV39_MODE << 60) | 1;
        let mut plan = AddressSpacePlan::new();
        plan.add_private_page(
            Address(0x4000),
            Address(0x20_000),
            crate::mmu::PagePermissions::USER_STACK,
        )
        .expect("parent page");
        runtime
            .register_process_address_space_plan(7, parent_satp, plan)
            .expect("parent space");
        let mut references = SharedFrameReferences::new();

        let child = runtime
            .create_process_with_cow_address_space(
                7,
                8,
                Address(0x1000),
                ByteCount(4096),
                0,
                0,
                CpuAffinity::Any,
                (crate::mmu::SV39_MODE << 60) | 2,
                &mut references,
            )
            .expect("child");

        assert_ne!(parent.main_thread, child.main_thread);
        assert_eq!(
            runtime.process_satp(8),
            Ok((crate::mmu::SV39_MODE << 60) | 2)
        );
        assert_eq!(references.count(Address(0x20_000)), Ok(2));
        assert_eq!(
            runtime
                .address_spaces
                .plan(7)
                .expect("parent")
                .compile()
                .expect("parent table")
                .translate(Address(0x4000)),
            Ok((Address(0x20_000), crate::mmu::PagePermissions::COW_USER))
        );
    }

    #[test]
    fn process_creation_rolls_back_failed_cow_clone() {
        let mut runtime = ExecRuntime::qemu();
        runtime
            .create_process(7, Address(0x1000), ByteCount(4096), 0, 0, CpuAffinity::Any)
            .expect("parent");
        let mut plan = AddressSpacePlan::new();
        plan.add_private_page(
            Address(0x4000),
            Address(0x20_000),
            crate::mmu::PagePermissions::USER_STACK,
        )
        .expect("parent page");
        runtime
            .register_process_address_space_plan(7, (crate::mmu::SV39_MODE << 60) | 1, plan)
            .expect("parent space");
        let mut references = SharedFrameReferences::new();

        assert_eq!(
            runtime.create_process_with_cow_address_space(
                7,
                8,
                Address(0x1000),
                ByteCount(4096),
                0,
                0,
                CpuAffinity::Any,
                0,
                &mut references,
            ),
            Err(ExecError::InvalidArgument)
        );
        assert_eq!(runtime.process_info(8), Err(ExecError::NotFound));
        assert_eq!(
            references.count(Address(0x20_000)),
            Err(ExecError::NotFound)
        );
        assert_eq!(
            runtime
                .address_spaces
                .plan(7)
                .expect("parent")
                .compile()
                .expect("parent table")
                .translate(Address(0x4000)),
            Ok((Address(0x20_000), crate::mmu::PagePermissions::USER_STACK))
        );
    }

    #[test]
    fn process_reaping_returns_last_cow_frame_to_allocator() {
        let mut runtime = ExecRuntime::qemu();
        let parent = runtime
            .create_process(7, Address(0x1000), ByteCount(4096), 0, 0, CpuAffinity::Any)
            .expect("parent");
        let mut allocator = FrameAllocator::new(0x30_000, 1).expect("allocator");
        let frame = allocator.allocate(1).expect("page");
        let mut plan = AddressSpacePlan::new();
        plan.add_private_page(
            Address(0x4000),
            Address(frame.base),
            crate::mmu::PagePermissions::USER_STACK,
        )
        .expect("parent page");
        runtime
            .register_process_address_space_plan(7, (crate::mmu::SV39_MODE << 60) | 1, plan)
            .expect("parent space");
        let mut references = SharedFrameReferences::new();
        let child = runtime
            .create_process_with_cow_address_space(
                7,
                8,
                Address(0x1000),
                ByteCount(4096),
                0,
                0,
                CpuAffinity::Any,
                (crate::mmu::SV39_MODE << 60) | 2,
                &mut references,
            )
            .expect("child");

        runtime
            .exit_owned(8, child.main_thread, 0)
            .expect("child exit");
        runtime
            .exit_owned(7, parent.main_thread, 0)
            .expect("parent exit");
        runtime
            .join(8, child.main_thread, None)
            .expect("child join");
        runtime
            .join(7, parent.main_thread, None)
            .expect("parent join");

        let child_released = runtime
            .reap_process_with_cow_references(8, &mut references)
            .expect("child reap");
        assert!(child_released.is_empty());
        let parent_released = runtime
            .reap_process_with_cow_references(7, &mut references)
            .expect("parent reap");
        assert_eq!(parent_released.get(0), Some(Address(frame.base)));
        assert_eq!(allocator.reclaim_cow_frames(&parent_released), Ok(1));
        assert_eq!(
            allocator.allocate(1).expect("reclaimed page").base,
            frame.base
        );
    }

    #[test]
    fn timer_tick_selects_another_ready_thread_on_the_same_hart() {
        let mut runtime = ExecRuntime::default();
        let (first, _) = runtime
            .create_thread(args(7, 1), CpuAffinity::Mask(HartMask::single(0)))
            .expect("first");
        let (second, _) = runtime
            .create_thread(args(7, 2), CpuAffinity::Mask(HartMask::single(0)))
            .expect("second");
        runtime.mark_running(first).expect("running");

        let decision = runtime.timer_tick(0, first).expect("tick");

        assert_eq!(
            decision,
            ScheduleDecision {
                previous: first,
                next: second,
                switched: true
            }
        );
        assert_eq!(
            runtime.info(first).expect("first info").state,
            crate::ThreadState::Ready
        );
        assert_eq!(
            runtime.info(second).expect("second info").state,
            crate::ThreadState::Running
        );
    }

    #[test]
    fn block_and_wake_preserve_thread_scheduler_entry() {
        let mut runtime = ExecRuntime::default();
        let (thread, _) = runtime
            .create_thread(args(7, 4), CpuAffinity::Mask(HartMask::single(0)))
            .expect("thread");
        runtime.mark_running(thread).expect("running");
        runtime.block_thread(thread).expect("blocked");
        assert_eq!(
            runtime.info(thread).expect("info").state,
            crate::ThreadState::Blocked
        );
        assert_eq!(runtime.pick_next(0), Err(ExecError::NotReady));
        assert_eq!(runtime.wake_thread(thread), Ok(0));
        assert_eq!(
            runtime.info(thread).expect("info").state,
            crate::ThreadState::Ready
        );
        assert_eq!(runtime.pick_next(0), Ok(thread));
    }

    #[test]
    fn block_current_thread_already_removed_from_ready_is_valid() {
        let mut runtime = ExecRuntime::default();
        let (thread, _) = runtime
            .create_thread(args(7, 4), CpuAffinity::Mask(HartMask::single(0)))
            .expect("thread");
        runtime.mark_running(thread).expect("running");
        runtime
            .scheduler
            .deschedule(thread)
            .expect("running thread is not ready");

        runtime.block_thread(thread).expect("blocked");
        assert_eq!(runtime.wake_thread(thread), Ok(0));
        assert_eq!(runtime.pick_next(0), Ok(thread));
    }

    #[test]
    fn join_blocks_waiter_and_exit_wakes_it_for_reap() {
        let mut runtime = ExecRuntime::default();
        let (waiter, _) = runtime
            .create_thread(args(7, 2), CpuAffinity::Mask(HartMask::single(0)))
            .expect("waiter");
        let (target, _) = runtime
            .create_thread(args(7, 1), CpuAffinity::Mask(HartMask::single(0)))
            .expect("target");
        runtime.mark_running(waiter).expect("running");

        assert_eq!(runtime.join_or_block(7, waiter, target, None), Ok(None));
        assert_eq!(
            runtime.info(waiter).expect("blocked").state,
            crate::ThreadState::Blocked
        );

        runtime.exit(target, 23).expect("target exit");
        assert_eq!(
            runtime.info(waiter).expect("woken").state,
            crate::ThreadState::Ready
        );
        assert_eq!(runtime.join(7, target, None), Ok(23));
    }

    #[test]
    fn reschedule_saves_current_context_and_returns_selected_context() {
        let mut runtime = ExecRuntime::default();
        let (first, _) = runtime
            .create_thread(args(7, 1), CpuAffinity::Mask(HartMask::single(0)))
            .expect("first");
        let (second, _) = runtime
            .create_thread(args(7, 2), CpuAffinity::Mask(HartMask::single(0)))
            .expect("second");
        runtime.mark_running(first).expect("running");
        runtime
            .attach_context(
                first,
                TaskContext::new(Address(1), Address(2), Address(3), 4),
            )
            .expect("first context");
        runtime
            .attach_context(
                second,
                TaskContext::new(Address(5), Address(6), Address(7), 8),
            )
            .expect("second context");
        let mut trap = SupervisorTrapFrame::empty();
        trap.sepc = 0x4444;

        let (decision, next) = runtime.reschedule(0, first, trap).expect("reschedule");

        assert_eq!(decision.next, second);
        assert_eq!(next.sepc, 5);
        assert_eq!(
            runtime.contexts.get(first).expect("saved").trap.sepc,
            0x4444
        );
    }

    #[test]
    fn rebalance_moves_only_ready_thread_and_context_together() {
        let mut runtime = ExecRuntime::default();
        let (running, _) = runtime
            .create_thread_with_context(
                args(7, 1),
                CpuAffinity::Mask(HartMask::single(0)),
                TaskContext::new(Address(1), Address(2), Address(3), 4),
            )
            .expect("running");
        let (ready, _) = runtime
            .create_thread_with_context(
                args(7, 2),
                CpuAffinity::Any,
                TaskContext::new(Address(5), Address(6), Address(7), 8),
            )
            .expect("ready");
        runtime
            .scheduler
            .migrate(ready, 0)
            .expect("stage on source");
        runtime.mark_running(running).expect("mark running");

        assert_eq!(runtime.rebalance_once(0, 1), Ok(ready));
        assert_eq!(runtime.scheduler.load(0), Ok(1));
        assert_eq!(runtime.scheduler.load(1), Ok(1));
        assert_eq!(runtime.contexts.get(ready).expect("context").hart, Some(1));
        assert_eq!(runtime.rebalance_once(0, 1), Err(ExecError::NotReady));
    }

    #[test]
    fn rebalance_from_chooses_an_online_least_loaded_hart() {
        let mut runtime = ExecRuntime::default();
        let (first, _) = runtime
            .create_thread_with_context(
                args(7, 1),
                CpuAffinity::Mask(HartMask::single(0)),
                TaskContext::new(Address(1), Address(2), Address(3), 4),
            )
            .expect("first");
        let (ready, _) = runtime
            .create_thread_with_context(
                args(7, 2),
                CpuAffinity::Any,
                TaskContext::new(Address(5), Address(6), Address(7), 8),
            )
            .expect("ready");
        runtime.scheduler.migrate(ready, 0).expect("source");
        runtime.mark_running(first).expect("running");

        let (moved, destination) = runtime.rebalance_from(0).expect("rebalance");
        assert_eq!(moved, ready);
        assert_eq!(destination, 1);
    }

    #[test]
    fn rebalance_skips_ready_thread_pinned_to_source() {
        let mut runtime = ExecRuntime::default();
        let (pinned, _) = runtime
            .create_thread_with_context(
                args(7, 1),
                CpuAffinity::Mask(HartMask::single(0)),
                TaskContext::new(Address(1), Address(2), Address(3), 4),
            )
            .expect("pinned");
        let (movable, _) = runtime
            .create_thread_with_context(
                args(7, 2),
                CpuAffinity::Any,
                TaskContext::new(Address(5), Address(6), Address(7), 8),
            )
            .expect("movable");
        runtime.mark_running(pinned).expect("pinned running");
        runtime.scheduler.migrate(movable, 0).expect("source");

        assert_eq!(runtime.rebalance_from(0), Ok((movable, 1)));
        assert_eq!(
            runtime.contexts.get(movable).expect("context").hart,
            Some(1)
        );
    }

    #[test]
    fn thread_creation_with_context_assigns_hart_transactionally() {
        let mut runtime = ExecRuntime::default();
        let (thread, hart) = runtime
            .create_thread_with_context(
                args(7, 1),
                CpuAffinity::Mask(HartMask::single(2)),
                TaskContext::new(Address(1), Address(2), Address(3), 4),
            )
            .expect("create with context");

        assert_eq!(hart, 2);
        assert_eq!(runtime.contexts.get(thread).expect("context").hart, Some(2));
    }

    #[test]
    fn invalid_thread_context_rolls_back_thread_and_scheduler_entry() {
        let mut runtime = ExecRuntime::default();
        let result = runtime.create_thread_with_context(
            args(7, 1),
            CpuAffinity::Any,
            TaskContext::new(Address(0), Address(2), Address(3), 4),
        );

        assert_eq!(result, Err(ExecError::InvalidArgument));
        assert_eq!(runtime.thread_count(7), 0);
        assert_eq!(runtime.pick_next(0), Err(ExecError::NotReady));
    }

    #[test]
    fn thread_creation_with_stacks_builds_a_context_from_virtual_ranges() {
        let mut runtime = ExecRuntime::default();
        let mut stacks = StackAllocator::new(
            Address(0x10_000),
            Address(0x12_000),
            Address(0x20_000),
            Address(0x22_000),
        )
        .expect("stacks");

        let (thread, _) = runtime
            .create_thread_with_stacks(
                args(7, 1),
                CpuAffinity::Mask(HartMask::single(0)),
                &mut stacks,
                9,
            )
            .expect("thread");

        let context = runtime.contexts.get(thread).expect("context");
        assert_eq!(context.user_stack, Address(0x10_000 + 4096));
        assert_eq!(context.kernel_stack, Address(0x20_000 + 4096));
        assert_eq!(context.satp, 9);
    }

    #[test]
    fn failed_stack_thread_creation_rolls_back_allocator() {
        let mut runtime = ExecRuntime::default();
        let mut stacks = StackAllocator::new(
            Address(0x10_000),
            Address(0x12_000),
            Address(0x20_000),
            Address(0x22_000),
        )
        .expect("stacks");

        assert_eq!(
            runtime.create_thread_with_stacks(
                args(7, 1),
                CpuAffinity::Mask(HartMask::single(63)),
                &mut stacks,
                0,
            ),
            Err(ExecError::PermissionDenied)
        );
        let allocation = stacks.allocate(4096).expect("reused slot");
        assert_eq!(allocation.user_base, Address(0x10_000));
        assert_eq!(allocation.kernel_base, Address(0x20_000));
    }
}
