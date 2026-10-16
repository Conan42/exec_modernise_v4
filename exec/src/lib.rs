#![no_std]
#![forbid(unsafe_code)]

//! Contrats Exec V3 : types stables, sans dépendance matériel.

#[cfg(test)]
extern crate std;

pub mod context;
pub mod ipi;
pub mod mmu;
pub mod platform;
pub mod process;
pub mod runtime;
pub mod sbi;
pub mod scheduler;
pub mod smp;
#[path = "syscall.rs"]
pub mod syscall_dispatch;
pub mod trampoline;
pub mod wait;

pub type TaskId = u64;
pub type ProcessId = u64;
pub type ThreadId = u64;
pub type Handle = u32;
pub type Timestamp = u64;
pub type ThreadExitCode = i32;

/// Taille stable du paquet retourné par le service d'entrée.
pub const INPUT_EVENT_WIRE_BYTES: usize = 32;

/// Événement d'entrée copié par Exec vers un buffer U-Mode validé.
///
/// Les champs sont scalaires et l'encodage est documenté en little-endian ;
/// aucune adresse ni représentation Rust d'une Library ne franchit l'ABI.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InputEventWire {
    pub kind: u8,
    pub phase: u8,
    pub flags: u16,
    pub code: u16,
    pub touch_id: u16,
    pub x: i32,
    pub y: i32,
    pub value: i32,
    pub pressure: u16,
    pub size: u16,
    pub timestamp: u64,
}

impl InputEventWire {
    pub const KIND_KEY: u8 = 1;
    pub const KIND_POINTER: u8 = 2;
    pub const KIND_TOUCH: u8 = 3;
    pub const PHASE_DOWN: u8 = 1;
    pub const PHASE_UP: u8 = 2;
    pub const PHASE_MOVE: u8 = 3;
    pub const PHASE_WHEEL: u8 = 4;
    pub const PHASE_RIGHT_DOWN: u8 = 10;
    pub const PHASE_RIGHT_UP: u8 = 11;
    pub const PHASE_RIGHT_MOVE: u8 = 12;
    pub const PHASE_BEGAN: u8 = 5;
    pub const PHASE_MOVED: u8 = 6;
    pub const PHASE_STATIONARY: u8 = 7;
    pub const PHASE_ENDED: u8 = 8;
    pub const PHASE_CANCELLED: u8 = 9;
    pub const FLAG_SHIFT: u16 = 1;

    pub fn to_le_bytes(self) -> [u8; INPUT_EVENT_WIRE_BYTES] {
        let mut bytes = [0; INPUT_EVENT_WIRE_BYTES];
        bytes[0] = self.kind;
        bytes[1] = self.phase;
        bytes[2..4].copy_from_slice(&self.flags.to_le_bytes());
        bytes[4..6].copy_from_slice(&self.code.to_le_bytes());
        bytes[6..8].copy_from_slice(&self.touch_id.to_le_bytes());
        bytes[8..12].copy_from_slice(&self.x.to_le_bytes());
        bytes[12..16].copy_from_slice(&self.y.to_le_bytes());
        bytes[16..20].copy_from_slice(&self.value.to_le_bytes());
        bytes[20..22].copy_from_slice(&self.pressure.to_le_bytes());
        bytes[22..24].copy_from_slice(&self.size.to_le_bytes());
        bytes[24..32].copy_from_slice(&self.timestamp.to_le_bytes());
        bytes
    }
}

/// Numéros ABI retenus par la V1 et conservés pour la compatibilité V3.
pub mod syscall {
    pub const YIELD: u16 = 0;
    pub const GET_ABI_VERSION: u16 = 1;
    pub const GET_TASK_ID: u16 = 2;
    pub const ALLOC_PAGE: u16 = 3;
    pub const FREE_PAGE: u16 = 4;
    pub const SEND_MESSAGE: u16 = 5;
    pub const RECEIVE_MESSAGE: u16 = 6;
    pub const OPEN_DEVICE: u16 = 7;
    pub const CLOSE_DEVICE: u16 = 8;
    pub const SEND_IO: u16 = 9;
    pub const CHECK_IO: u16 = 10;
    pub const WAIT_IO: u16 = 11;
    pub const ABORT_IO: u16 = 12;
    pub const DO_IO: u16 = 13;
    pub const OPEN_LIBRARY: u16 = 14;
    pub const CLOSE_LIBRARY: u16 = 15;
    pub const OPEN_FILE: u16 = 16;
    pub const READ_FILE: u16 = 17;
    pub const CLOSE_FILE: u16 = 18;
    /// Soumet une frame Graphics au worker de présentation.
    pub const PRESENT_FRAME: u16 = 19;
    pub const PRESENT_CURSOR_OVERLAY: u16 = 20;
    pub const WAIT_INPUT: u16 = 21;
    pub const POLL_INPUT: u16 = 22;
    /// Lit l'horloge monotone du système en ticks matériels.
    pub const GET_TIME: u16 = 23;
    /// Sonde l'état de la dernière frame Graphics soumise.
    pub const POLL_PRESENT_FRAME: u16 = 24;
    /// Trace bornée destinée à la console de bring-up U-Mode.
    pub const DEBUG_LOG: u16 = 25;
    /// Accès borné aux fichiers de préférence DOS.
    pub const WRITE_FILE: u16 = 26;
    pub const CREATE_PROCESS: u16 = 49;
    pub const EXIT_PROCESS: u16 = 50;
    pub const WAIT_PROCESS: u16 = 51;
    pub const THREAD_CREATE: u16 = 80;
    pub const THREAD_EXIT: u16 = 81;
    pub const THREAD_JOIN: u16 = 82;
}

/// Entrées ABI de la V1 formalisées dans V3.
///
/// Cette table fixe les numéros ; elle ne constitue pas encore le dispatcher
/// matériel des `ecall`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum SyscallNumber {
    Yield = syscall::YIELD,
    GetAbiVersion = syscall::GET_ABI_VERSION,
    GetTaskId = syscall::GET_TASK_ID,
    AllocPage = syscall::ALLOC_PAGE,
    FreePage = syscall::FREE_PAGE,
    SendMessage = syscall::SEND_MESSAGE,
    ReceiveMessage = syscall::RECEIVE_MESSAGE,
    OpenDevice = syscall::OPEN_DEVICE,
    CloseDevice = syscall::CLOSE_DEVICE,
    SendIo = syscall::SEND_IO,
    CheckIo = syscall::CHECK_IO,
    WaitIo = syscall::WAIT_IO,
    AbortIo = syscall::ABORT_IO,
    DoIo = syscall::DO_IO,
    OpenLibrary = syscall::OPEN_LIBRARY,
    CloseLibrary = syscall::CLOSE_LIBRARY,
    OpenFile = syscall::OPEN_FILE,
    ReadFile = syscall::READ_FILE,
    CloseFile = syscall::CLOSE_FILE,
    PresentFrame = syscall::PRESENT_FRAME,
    PollPresentFrame = syscall::POLL_PRESENT_FRAME,
    PresentCursorOverlay = syscall::PRESENT_CURSOR_OVERLAY,
    WaitInput = syscall::WAIT_INPUT,
    PollInput = syscall::POLL_INPUT,
    GetTime = syscall::GET_TIME,
    DebugLog = syscall::DEBUG_LOG,
    WriteFile = syscall::WRITE_FILE,
    CreateProcess = syscall::CREATE_PROCESS,
    ExitProcess = syscall::EXIT_PROCESS,
    WaitProcess = syscall::WAIT_PROCESS,
    ThreadCreate = syscall::THREAD_CREATE,
    ThreadExit = syscall::THREAD_EXIT,
    ThreadJoin = syscall::THREAD_JOIN,
}

impl SyscallNumber {
    pub const fn from_raw(value: u16) -> Option<Self> {
        Some(match value {
            syscall::YIELD => Self::Yield,
            syscall::GET_ABI_VERSION => Self::GetAbiVersion,
            syscall::GET_TASK_ID => Self::GetTaskId,
            syscall::ALLOC_PAGE => Self::AllocPage,
            syscall::FREE_PAGE => Self::FreePage,
            syscall::SEND_MESSAGE => Self::SendMessage,
            syscall::RECEIVE_MESSAGE => Self::ReceiveMessage,
            syscall::OPEN_DEVICE => Self::OpenDevice,
            syscall::CLOSE_DEVICE => Self::CloseDevice,
            syscall::SEND_IO => Self::SendIo,
            syscall::CHECK_IO => Self::CheckIo,
            syscall::WAIT_IO => Self::WaitIo,
            syscall::ABORT_IO => Self::AbortIo,
            syscall::DO_IO => Self::DoIo,
            syscall::OPEN_LIBRARY => Self::OpenLibrary,
            syscall::CLOSE_LIBRARY => Self::CloseLibrary,
            syscall::OPEN_FILE => Self::OpenFile,
            syscall::READ_FILE => Self::ReadFile,
            syscall::CLOSE_FILE => Self::CloseFile,
            syscall::PRESENT_FRAME => Self::PresentFrame,
            syscall::POLL_PRESENT_FRAME => Self::PollPresentFrame,
            syscall::PRESENT_CURSOR_OVERLAY => Self::PresentCursorOverlay,
            syscall::WAIT_INPUT => Self::WaitInput,
            syscall::POLL_INPUT => Self::PollInput,
            syscall::GET_TIME => Self::GetTime,
            syscall::DEBUG_LOG => Self::DebugLog,
            syscall::WRITE_FILE => Self::WriteFile,
            syscall::CREATE_PROCESS => Self::CreateProcess,
            syscall::EXIT_PROCESS => Self::ExitProcess,
            syscall::WAIT_PROCESS => Self::WaitProcess,
            syscall::THREAD_CREATE => Self::ThreadCreate,
            syscall::THREAD_EXIT => Self::ThreadExit,
            syscall::THREAD_JOIN => Self::ThreadJoin,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Address(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ByteCount(pub usize);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecError {
    InvalidArgument,
    InvalidHandle,
    NotFound,
    PermissionDenied,
    OutOfMemory,
    QueueFull,
    Timeout,
    NotReady,
    Fault,
}

pub type ExecResult<T> = Result<T, ExecError>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThreadState {
    Ready,
    Running,
    Blocked,
    Exited,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ThreadCreateArgs {
    pub process: ProcessId,
    pub entry: Address,
    pub stack_size: ByteCount,
    pub priority: i8,
    pub argument: u64,
}

impl ThreadCreateArgs {
    pub const MIN_STACK_SIZE: usize = 4096;
    pub const MAX_STACK_SIZE: usize = 8 * 1024 * 1024;
    pub const MAX_THREADS_PER_PROCESS: usize = 32;

    pub fn validate(self) -> ExecResult<()> {
        if self.process == 0
            || self.entry.0 == 0
            || !self.stack_size.0.is_multiple_of(16)
            || !(Self::MIN_STACK_SIZE..=Self::MAX_STACK_SIZE).contains(&self.stack_size.0)
        {
            return Err(ExecError::InvalidArgument);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ThreadInfo {
    pub id: ThreadId,
    pub process: ProcessId,
    pub state: ThreadState,
    pub exit_code: Option<ThreadExitCode>,
}

/// Contrat abstrait des primitives de processus léger fournies par Exec.
pub trait ThreadService {
    fn thread_create(&mut self, args: ThreadCreateArgs) -> ExecResult<ThreadId>;
    fn thread_join(
        &mut self,
        thread: ThreadId,
        deadline: Option<Timestamp>,
    ) -> ExecResult<ThreadExitCode>;
    fn thread_exit(&mut self, code: ThreadExitCode) -> !;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_args() -> ThreadCreateArgs {
        ThreadCreateArgs {
            process: 1,
            entry: Address(0x1000),
            stack_size: ByteCount(ThreadCreateArgs::MIN_STACK_SIZE),
            priority: 10,
            argument: 0,
        }
    }

    #[test]
    fn thread_create_args_accept_bounded_aligned_stack() {
        assert_eq!(valid_args().validate(), Ok(()));
    }

    #[test]
    fn thread_create_args_reject_invalid_stack() {
        let mut args = valid_args();
        args.stack_size = ByteCount(4097);
        assert_eq!(args.validate(), Err(ExecError::InvalidArgument));
    }

    #[test]
    fn v1_syscall_numbers_are_decoded() {
        assert_eq!(SyscallNumber::from_raw(16), Some(SyscallNumber::OpenFile));
        assert_eq!(
            SyscallNumber::from_raw(49),
            Some(SyscallNumber::CreateProcess)
        );
        assert_eq!(
            SyscallNumber::from_raw(80),
            Some(SyscallNumber::ThreadCreate)
        );
        assert_eq!(SyscallNumber::from_raw(81), Some(SyscallNumber::ThreadExit));
        assert_eq!(SyscallNumber::from_raw(82), Some(SyscallNumber::ThreadJoin));
        assert_eq!(SyscallNumber::from_raw(79), None);
    }
}
