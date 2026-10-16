//! Dispatcher syscall portable, avant son raccordement au trampoline assembleur.
//!
//! La convention RISC-V utilisée ici place le numéro dans `a7` (`x17`) et le
//! résultat dans `a0` (`x10`). Le dispatcher ne lit encore aucun pointeur
//! U-Mode et ne remplace donc pas la validation matérielle à venir.

use crate::context::SupervisorTrapFrame;
use crate::mmu::{
    AddressSpacePlan, FrameAllocator, MappingPlan, PAGE_SIZE, PagePermissions, UserPageRange,
};
use crate::runtime::ExecRuntime;
use crate::scheduler::CpuAffinity;
use crate::{
    Address, ByteCount, ExecError, ExecResult, INPUT_EVENT_WIRE_BYTES, InputEventWire, ProcessId,
    SyscallNumber, ThreadCreateArgs, ThreadId, Timestamp,
};
use filesystem_v4::{
    ElfImage, ElfSegment, FILE_TYPE_APP, MAX_PROGRAM_HEADERS, MffsError, MffsImage, PF_R, PF_W,
    PF_X, parse_elf64,
};

pub const ABI_VERSION: u64 = 25;
pub const SCAUSE_INTERRUPT_BIT: u64 = 1 << 63;
pub const SCAUSE_USER_ECALL: u64 = 8;
pub const SCAUSE_INSTRUCTION_PAGE_FAULT: u64 = 12;
pub const SCAUSE_LOAD_PAGE_FAULT: u64 = 13;
pub const SCAUSE_STORE_PAGE_FAULT: u64 = 15;
pub const SCAUSE_SUPERVISOR_TIMER: u64 = SCAUSE_INTERRUPT_BIT | 5;
pub const SCAUSE_SUPERVISOR_EXTERNAL: u64 = SCAUSE_INTERRUPT_BIT | 9;
pub const SCAUSE_SUPERVISOR_SOFTWARE: u64 = SCAUSE_INTERRUPT_BIT | 1;
pub const USER_ADDRESS_MIN: u64 = 0x1000;
/// Limite supérieure de la moitié utilisateur canonique Sv39.
pub const USER_ADDRESS_MAX: u64 = (1 << 38) - 1;
pub const SSTATUS_SPP: u64 = 1 << 8;
/// Taille maximale du nom transmis à `CreateProcess`, NUL terminal compris.
pub const MAX_PROCESS_NAME: usize = 24;
/// Taille maximale d'un paquet Graphics transmis par un appel U-Mode.
/// Capacité commune du paquet PresentFrame : environ 160 commandes de scène,
/// le wallpaper éventuel et les trois commandes de pass. Cette limite de
/// transport est indépendante de la capacité logique de la scène GadTools.
pub const MAX_PRESENT_PACKET_COMMANDS: usize = 164;
pub const MAX_PRESENT_PACKET_BYTES: usize = MAX_PRESENT_PACKET_COMMANDS * 96;
pub const PRESENT_COMMAND_BYTES: usize = 96;
/// Marque une primitive Graphics dont les coordonnées sont celles du Screen.
pub const PRESENT_FLAG_SCREEN_SPACE: u8 = 0x80;
pub const PRESENT_FRAME_WIRE_BYTES: usize = 24;
pub const PRESENT_CURSOR_OVERLAY_WIRE_BYTES: usize = 24;
pub const MAX_FILE_PATH_BYTES: usize = 256;
pub const MAX_FILE_IO_BYTES: usize = 4096;
pub const FILE_OPEN_READ: u64 = 0;
pub const FILE_OPEN_WRITE_CREATE: u64 = 1;

/// Valeurs retournées par `POLL_PRESENT_FRAME`.
///
/// Le statut est volontairement scalaire : le propriétaire de la surface
/// conserve son `PresentationToken` et ne le libère qu'après `RETIRED`.
pub const PRESENT_STATUS_IDLE: u64 = 0;
pub const PRESENT_STATUS_SUBMITTED: u64 = 1;
pub const PRESENT_STATUS_PRESENTED: u64 = 2;
pub const PRESENT_STATUS_RETIRED: u64 = 3;

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PresentCursorOverlay {
    pub x: i32,
    pub y: i32,
    pub cursor_id: u32,
    pub phase: u32,
    pub visible: u32,
    pub reserved: u32,
}

impl PresentCursorOverlay {
    pub const NUMBER: u16 = crate::syscall::PRESENT_CURSOR_OVERLAY;

    pub const fn validate(self) -> ExecResult<()> {
        if self.cursor_id > 46 || self.visible > 1 || self.reserved != 0 {
            Err(ExecError::InvalidArgument)
        } else {
            Ok(())
        }
    }
}

/// Description ABI d'une frame Graphics remise à Exec.
///
/// Les adresses sont des adresses virtuelles du processus appelant. Exec les
/// valide et copie le paquet avant de le remettre au worker Graphics ; aucune
/// adresse utilisateur ne franchit cette frontière. La structure est décrite
/// en champs scalaires plutôt qu'en pointeurs Rust afin de rester stable entre
/// l'application, le trampoline et le kernel.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PresentFrame {
    pub packet_address: u64,
    pub packet_length: u32,
    pub surface_index: u16,
    pub generation: u64,
}

impl PresentFrame {
    pub const NUMBER: u16 = crate::syscall::PRESENT_FRAME;

    pub const fn new(
        packet_address: u64,
        packet_length: u32,
        surface_index: u16,
        generation: u64,
    ) -> Self {
        Self {
            packet_address,
            packet_length,
            surface_index,
            generation,
        }
    }

    pub const fn validate(self) -> ExecResult<()> {
        if self.generation == 0
            || self.surface_index >= 3
            || self.packet_length == 0
            || self.packet_length as usize > MAX_PRESENT_PACKET_BYTES
            || !(self.packet_length as usize).is_multiple_of(PRESENT_COMMAND_BYTES)
        {
            return Err(ExecError::InvalidArgument);
        }
        match validate_user_range(self.packet_address, self.packet_length as u64) {
            Ok(_) => Ok(()),
            Err(error) => Err(error),
        }
    }
}

/// Destination du paquet après validation et copie depuis l'U-Mode.
///
/// Le sink est fourni par le worker Graphics ou par l'adaptateur de plateforme
/// et ne reçoit jamais un pointeur utilisateur. Il peut donc mettre la frame
/// en file, la décoder ou la soumettre au GPU selon son état matériel.
pub trait PresentFrameSink {
    fn submit_present_frame(
        &mut self,
        process: ProcessId,
        frame: PresentFrame,
        packet: &[u8],
    ) -> ExecResult<u64>;

    /// Avance d'un seul état la frame en vol sans attendre le GPU.
    ///
    /// Le défaut conserve la compatibilité des sinks qui ne proposent pas
    /// encore la présentation asynchrone.
    fn poll_present_frame(&mut self, _process: ProcessId) -> ExecResult<u64> {
        Err(ExecError::NotReady)
    }
}

/// Destination du sprite après validation de l'ABI U-Mode.
pub trait CursorOverlaySink {
    fn submit_cursor_overlay(
        &mut self,
        process: ProcessId,
        overlay: PresentCursorOverlay,
    ) -> ExecResult<u64>;
}

/// Source noyau abstraite pour les événements provenant des Devices.
pub trait InputEventSource {
    fn pop_input_event(&mut self) -> Option<InputEventWire>;

    /// Dépile un événement destiné à une fenêtre logique.
    ///
    /// Le profil global conserve la compatibilité avec la source historique;
    /// Intuition pourra remplacer cette implémentation par une FIFO par
    /// `WindowHandle` sans modifier le dispatcher.
    fn pop_input_event_for(&mut self, _window: u32) -> Option<InputEventWire> {
        self.pop_input_event()
    }
}

/// Backend DOS borné utilisé par Exec pour les fichiers de préférence.
///
/// Le backend reste propriétaire du stockage ; Exec ne fait que valider les
/// pointeurs U-Mode et copier les octets à la frontière ABI.
pub trait FileService {
    fn open(&mut self, process: ProcessId, path: &[u8], mode: u64) -> ExecResult<u64>;

    fn read(
        &mut self,
        process: ProcessId,
        handle: u64,
        offset: u64,
        destination: &mut [u8],
    ) -> ExecResult<usize>;

    fn write(
        &mut self,
        process: ProcessId,
        handle: u64,
        offset: u64,
        source: &[u8],
    ) -> ExecResult<usize>;

    fn close(&mut self, process: ProcessId, handle: u64) -> ExecResult<()>;
}

/// Indique si un trap peut remettre le scheduler en mouvement.
///
/// Un trap provenant du noyau (`SPP=1`) ou reçu avant la publication du thread
/// courant ne doit jamais restaurer un contexte U-Mode.
pub const fn may_reschedule(sstatus: u64, current_thread: ThreadId) -> bool {
    current_thread != 0 && sstatus & SSTATUS_SPP == 0
}

/// Un fault de stockage utilisateur peut correspondre à une écriture sur une
/// page COW. Les faults provenant du supervisor restent des erreurs noyau.
pub const fn is_user_store_page_fault(scause: u64, sstatus: u64) -> bool {
    scause == SCAUSE_STORE_PAGE_FAULT && sstatus & SSTATUS_SPP == 0
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UserRange {
    pub address: u64,
    pub length: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UserAccess {
    Read,
    Write,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessName {
    bytes: [u8; MAX_PROCESS_NAME],
    length: usize,
}

impl ProcessName {
    pub const fn length(self) -> usize {
        self.length
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.length]
    }
}

/// Paramètres résolus par ExecFS/ELF avant publication du processus.
///
/// Le `ProcessLaunchSpec` reste volontairement indépendant du plan mémoire :
/// `MffsProcessImageResolver::load_into()` prépare ce plan séparément, puis un
/// backend Sv39 peut le compiler et publier le `satp` approprié.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessLaunchSpec {
    pub entry: Address,
    pub stack_size: ByteCount,
    pub priority: i8,
    pub argument: u64,
    pub affinity: CpuAffinity,
}

impl ProcessLaunchSpec {
    pub fn validate(self) -> ExecResult<()> {
        ThreadCreateArgs {
            process: 1,
            entry: self.entry,
            stack_size: self.stack_size,
            priority: self.priority,
            argument: self.argument,
        }
        .validate()
    }
}

/// Frontière entre le syscall `CreateProcess` et ExecFS/ELF.
///
/// La V1 recevait un nom de fichier et effectuait la résolution dans ExecFS.
/// La V3 conserve cette sémantique : le résolveur reçoit le nom NUL-terminal
/// et le processus appelant, puis renvoie une description validée du
/// lancement. Le chargement des pages et l'espace privé restent du backend.
pub trait ProcessImageResolver {
    fn resolve(&mut self, parent: ProcessId, name: &[u8]) -> ExecResult<ProcessLaunchSpec>;
}

/// Backend minimal chargé d'écrire les pages physiques préparées par le
/// chargeur ELF. Le backend kernel fera le mapping direct ou temporaire de la
/// frame ; Exec ne déréférence jamais une adresse physique lui-même.
pub trait ProcessImagePageWriter {
    fn zero_page(&mut self, physical: Address) -> ExecResult<()>;
    fn write_page(&mut self, physical: Address, bytes: &[u8]) -> ExecResult<()>;
}

/// Chargeur d'image utilisé par le chemin matériel de `CreateProcess`.
///
/// Le résolveur de nom et le chargeur sont séparés pour laisser au kernel le
/// choix de son backend de stockage. Un backend MFFS charge les segments dans
/// les frames fournies par Exec, sans jamais déréférencer une adresse physique
/// de son propre chef.
pub trait ProcessImageLoader {
    fn load_into<W: ProcessImagePageWriter>(
        &self,
        name: &[u8],
        allocator: &mut FrameAllocator,
        plan: &mut AddressSpacePlan,
        writer: &mut W,
    ) -> ExecResult<ProcessLaunchSpec>;
}

/// Résolveur ExecFS/MFFS utilisé par le chemin `CreateProcess`.
///
/// L'image MFFS est empruntée par le résolveur : aucun stockage n'est copié et
/// aucun pointeur fourni par l'UMode n'est déréférencé. La construction valide
/// le catalogue ; chaque résolution vérifie ensuite le checksum du payload et
/// valide l'ELF avant de publier quoi que ce soit dans Exec.
pub struct MffsProcessImageResolver<'a> {
    image: MffsImage<'a>,
    stack_size: ByteCount,
    priority: i8,
    affinity: CpuAffinity,
    user: u32,
    group: u32,
}

impl<'a> MffsProcessImageResolver<'a> {
    pub const DEFAULT_STACK_SIZE: usize = 96 * 1024;
    pub const DEFAULT_PRIORITY: i8 = 0;

    pub fn new(bytes: &'a [u8]) -> ExecResult<Self> {
        let image = MffsImage::parse(bytes).map_err(map_mffs_error)?;
        Ok(Self {
            image,
            stack_size: ByteCount(Self::DEFAULT_STACK_SIZE),
            priority: Self::DEFAULT_PRIORITY,
            affinity: CpuAffinity::Any,
            user: 0,
            group: 0,
        })
    }

    pub fn with_credentials(bytes: &'a [u8], user: u32, group: u32) -> ExecResult<Self> {
        let mut resolver = Self::new(bytes)?;
        resolver.user = user;
        resolver.group = group;
        Ok(resolver)
    }

    pub fn with_policy(
        bytes: &'a [u8],
        stack_size: ByteCount,
        priority: i8,
        affinity: CpuAffinity,
    ) -> ExecResult<Self> {
        let mut resolver = Self::new(bytes)?;
        resolver.stack_size = stack_size;
        resolver.priority = priority;
        resolver.affinity = affinity;
        Ok(resolver)
    }

    /// Charge les segments ELF dans des frames privées et ajoute leurs
    /// mappings au plan d'espace utilisateur.
    ///
    /// Cette étape ne construit pas encore la table Sv39 et ne publie aucun
    /// processus. En cas d'erreur, le plan est restauré et toutes les frames
    /// réservées par cet appel sont rendues à l'allocateur.
    pub fn load_into<W: ProcessImagePageWriter>(
        &self,
        name: &[u8],
        allocator: &mut FrameAllocator,
        plan: &mut AddressSpacePlan,
        writer: &mut W,
    ) -> ExecResult<ProcessLaunchSpec> {
        let file = self.image.lookup(name).map_err(map_mffs_error)?;
        if file.metadata.kind != FILE_TYPE_APP
            || !file.metadata.can_read_execute(self.user, self.group)
        {
            return Err(ExecError::PermissionDenied);
        }
        let elf = parse_elf64(file.data).map_err(|_| ExecError::InvalidArgument)?;
        let spec = self.spec_from_elf(&elf)?;
        let plan_checkpoint = *plan;
        let mut allocated = [None; MAX_PROGRAM_HEADERS];
        let result = (|| {
            for index in 0..elf.segment_count() {
                let segment = elf.segment(index).ok_or(ExecError::Fault)?;
                let memory_size =
                    usize::try_from(segment.memory_size).map_err(|_| ExecError::OutOfMemory)?;
                let page_count = memory_size
                    .checked_add(PAGE_SIZE as usize - 1)
                    .ok_or(ExecError::OutOfMemory)?
                    / PAGE_SIZE as usize;
                let frame = allocator.allocate(page_count)?;
                allocated[index] = Some(frame);
                for page in 0..page_count {
                    let physical = Address(frame.base + page as u64 * PAGE_SIZE);
                    writer.zero_page(physical)?;
                    let page_offset = page
                        .checked_mul(PAGE_SIZE as usize)
                        .ok_or(ExecError::Fault)?;
                    let file_size =
                        usize::try_from(segment.file_size).map_err(|_| ExecError::Fault)?;
                    if page_offset >= file_size {
                        continue;
                    }
                    let bytes_to_write = (file_size - page_offset).min(PAGE_SIZE as usize);
                    let file_offset = usize::try_from(segment.file_offset)
                        .ok()
                        .and_then(|offset| offset.checked_add(page_offset))
                        .ok_or(ExecError::Fault)?;
                    let source = file
                        .data
                        .get(file_offset..file_offset + bytes_to_write)
                        .ok_or(ExecError::Fault)?;
                    writer.write_page(physical, source)?;
                }
                let range =
                    UserPageRange::new(Address(segment.virtual_address), segment.memory_size)?;
                plan.add(MappingPlan::new(
                    range,
                    frame.base,
                    page_permissions(segment),
                )?)?;
            }
            Ok(spec)
        })();
        if result.is_err() {
            *plan = plan_checkpoint;
            for frame in allocated.iter().flatten() {
                let _ = allocator.free(*frame);
            }
        }
        result
    }

    fn spec_from_elf(&self, elf: &ElfImage) -> ExecResult<ProcessLaunchSpec> {
        let spec = ProcessLaunchSpec {
            entry: Address(elf.entry),
            stack_size: self.stack_size,
            priority: self.priority,
            argument: 0,
            affinity: self.affinity,
        };
        spec.validate()?;
        Ok(spec)
    }
}

impl ProcessImageLoader for MffsProcessImageResolver<'_> {
    fn load_into<W: ProcessImagePageWriter>(
        &self,
        name: &[u8],
        allocator: &mut FrameAllocator,
        plan: &mut AddressSpacePlan,
        writer: &mut W,
    ) -> ExecResult<ProcessLaunchSpec> {
        MffsProcessImageResolver::load_into(self, name, allocator, plan, writer)
    }
}

impl ProcessImageResolver for MffsProcessImageResolver<'_> {
    fn resolve(&mut self, _parent: ProcessId, name: &[u8]) -> ExecResult<ProcessLaunchSpec> {
        let file = self.image.lookup(name).map_err(map_mffs_error)?;
        if file.metadata.kind != FILE_TYPE_APP
            || !file.metadata.can_read_execute(self.user, self.group)
        {
            return Err(ExecError::PermissionDenied);
        }
        let elf = parse_elf64(file.data).map_err(|_| ExecError::InvalidArgument)?;
        self.spec_from_elf(&elf)
    }
}

fn page_permissions(segment: ElfSegment) -> PagePermissions {
    PagePermissions {
        read: segment.flags & PF_R != 0,
        write: segment.flags & PF_W != 0,
        execute: segment.flags & PF_X != 0,
        user: true,
    }
}

fn map_mffs_error(error: MffsError) -> ExecError {
    match error {
        MffsError::NotFound => ExecError::NotFound,
        MffsError::ChecksumMismatch => ExecError::Fault,
        _ => ExecError::InvalidArgument,
    }
}

pub fn copy_process_name<M: UserMemory>(
    memory: &M,
    address: u64,
    length: u64,
) -> ExecResult<ProcessName> {
    let length = usize::try_from(length).map_err(|_| ExecError::InvalidArgument)?;
    if !(2..=MAX_PROCESS_NAME).contains(&length) {
        return Err(ExecError::InvalidArgument);
    }
    let mut bytes = [0u8; MAX_PROCESS_NAME];
    copy_from_user(memory, address, &mut bytes[..length])?;
    if bytes[length - 1] != 0 || bytes[..length - 1].contains(&0) {
        return Err(ExecError::InvalidArgument);
    }
    Ok(ProcessName { bytes, length })
}

/// Frontière entre le dispatcher et les tables de pages U-Mode.
///
/// L’implémentation Sv39 vérifiera les PTE et les permissions. Un backend de
/// test peut fournir la même garantie sans exécuter de code privilégié.
pub trait UserMemory {
    fn validate(&self, range: UserRange, access: UserAccess) -> ExecResult<()>;
    fn read(&self, range: UserRange, destination: &mut [u8]) -> ExecResult<usize>;
    fn write(&mut self, range: UserRange, source: &[u8]) -> ExecResult<usize>;
}

/// Valide une plage virtuelle utilisateur sans accéder à la mémoire.
pub const fn validate_user_range(address: u64, length: u64) -> Result<UserRange, ExecError> {
    if length == 0 {
        return Ok(UserRange { address, length });
    }
    if address < USER_ADDRESS_MIN || address > USER_ADDRESS_MAX {
        return Err(ExecError::InvalidArgument);
    }
    let Some(last) = address.checked_add(length - 1) else {
        return Err(ExecError::InvalidArgument);
    };
    if last > USER_ADDRESS_MAX {
        return Err(ExecError::InvalidArgument);
    }
    Ok(UserRange { address, length })
}

pub fn copy_from_user<M: UserMemory + ?Sized>(
    memory: &M,
    address: u64,
    destination: &mut [u8],
) -> ExecResult<usize> {
    let range = validate_user_range(address, destination.len() as u64)?;
    memory.validate(range, UserAccess::Read)?;
    let copied = memory.read(range, destination)?;
    if copied != destination.len() {
        return Err(ExecError::Fault);
    }
    Ok(copied)
}

pub fn copy_to_user<M: UserMemory + ?Sized>(
    memory: &mut M,
    address: u64,
    source: &[u8],
) -> ExecResult<usize> {
    let range = validate_user_range(address, source.len() as u64)?;
    memory.validate(range, UserAccess::Write)?;
    let copied = memory.write(range, source)?;
    if copied != source.len() {
        return Err(ExecError::Fault);
    }
    Ok(copied)
}

fn file_read_outcome<M: UserMemory, F: FileService>(
    files: &mut F,
    memory: &mut M,
    process: ProcessId,
    frame: &SupervisorTrapFrame,
    number: Option<SyscallNumber>,
) -> SyscallOutcome {
    let length = usize::try_from(frame.x[12]).unwrap_or(usize::MAX);
    let offset = frame.x[13];
    if length > MAX_FILE_IO_BYTES {
        return SyscallOutcome::failure(number, ExecError::InvalidArgument);
    }
    let mut bytes = [0u8; MAX_FILE_IO_BYTES];
    match files.read(process, frame.x[10], offset, &mut bytes[..length]) {
        Ok(count) if count <= length => match copy_to_user(memory, frame.x[11], &bytes[..count]) {
            Ok(_) => SyscallOutcome::success(SyscallNumber::ReadFile, count as u64),
            Err(error) => SyscallOutcome::failure(number, error),
        },
        Ok(_) => SyscallOutcome::failure(number, ExecError::Fault),
        Err(error) => SyscallOutcome::failure(number, error),
    }
}

fn file_write_outcome<M: UserMemory, F: FileService>(
    files: &mut F,
    memory: &mut M,
    process: ProcessId,
    frame: &SupervisorTrapFrame,
    number: Option<SyscallNumber>,
) -> SyscallOutcome {
    let length = usize::try_from(frame.x[12]).unwrap_or(usize::MAX);
    let offset = frame.x[13];
    if length > MAX_FILE_IO_BYTES {
        return SyscallOutcome::failure(number, ExecError::InvalidArgument);
    }
    let mut bytes = [0u8; MAX_FILE_IO_BYTES];
    if let Err(error) = copy_from_user(memory, frame.x[11], &mut bytes[..length]) {
        return SyscallOutcome::failure(number, error);
    }
    match files.write(process, frame.x[10], offset, &bytes[..length]) {
        Ok(count) if count <= length => {
            SyscallOutcome::success(SyscallNumber::WriteFile, count as u64)
        }
        Ok(_) => SyscallOutcome::failure(number, ExecError::Fault),
        Err(error) => SyscallOutcome::failure(number, error),
    }
}

fn decode_present_frame(bytes: &[u8; PRESENT_FRAME_WIRE_BYTES]) -> PresentFrame {
    PresentFrame {
        packet_address: u64::from_le_bytes(bytes[0..8].try_into().unwrap_or([0; 8])),
        packet_length: u32::from_le_bytes(bytes[8..12].try_into().unwrap_or([0; 4])),
        surface_index: u16::from_le_bytes(bytes[12..14].try_into().unwrap_or([0; 2])),
        generation: u64::from_le_bytes(bytes[16..24].try_into().unwrap_or([0; 8])),
    }
}

fn decode_cursor_overlay(bytes: &[u8; PRESENT_CURSOR_OVERLAY_WIRE_BYTES]) -> PresentCursorOverlay {
    PresentCursorOverlay {
        x: i32::from_le_bytes(bytes[0..4].try_into().unwrap_or([0; 4])),
        y: i32::from_le_bytes(bytes[4..8].try_into().unwrap_or([0; 4])),
        cursor_id: u32::from_le_bytes(bytes[8..12].try_into().unwrap_or([0; 4])),
        phase: u32::from_le_bytes(bytes[12..16].try_into().unwrap_or([0; 4])),
        visible: u32::from_le_bytes(bytes[16..20].try_into().unwrap_or([0; 4])),
        reserved: u32::from_le_bytes(bytes[20..24].try_into().unwrap_or([0; 4])),
    }
}

fn validate_present_packet(frame: PresentFrame, packet: &[u8]) -> ExecResult<()> {
    frame.validate()?;
    if packet.len() != frame.packet_length as usize {
        return Err(ExecError::Fault);
    }
    for command in packet.chunks_exact(PRESENT_COMMAND_BYTES) {
        // Les opcodes 13..15 sont les états de passe V3 (viewport, scissor,
        // damage). L'octet 2 transporte le RenderMaterial validé ; Exec ne
        // fait ici qu'autoriser son enveloppe wire bornée.
        let allowed_flags = match command[0] {
            6 => 0xff,
            2 | 3 | 7 | 8 => 0x0f | PRESENT_FLAG_SCREEN_SPACE,
            _ => 0x0f,
        };
        if !(1..=18).contains(&command[0])
            || command[1] & !allowed_flags != 0
            || command[2] > 15
            || command[3] != 0
        {
            return Err(ExecError::InvalidArgument);
        }
        let carries_text = matches!(command[0], 2 | 5 | 6 | 7 | 8 | 11);
        if carries_text {
            if command[12] == 13 {
                let length = u32::from_le_bytes(command[24..28].try_into().unwrap()) as usize;
                if length == 0
                    || length > 64
                    || 28 + length > command.len()
                    || core::str::from_utf8(&command[28..28 + length]).is_err()
                    || command[28 + length..].iter().any(|byte| *byte != 0)
                {
                    return Err(ExecError::InvalidArgument);
                }
            } else if command[12] > 13 {
                return Err(ExecError::InvalidArgument);
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyscallOutcome {
    pub number: Option<SyscallNumber>,
    pub value: u64,
    pub error: Option<ExecError>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrapDisposition {
    Syscall(SyscallOutcome),
    TimerInterrupt,
    ExternalInterrupt,
    SoftwareInterrupt,
    UserStorePageFault(Address),
    Fault,
}

impl SyscallOutcome {
    pub const fn success(number: SyscallNumber, value: u64) -> Self {
        Self {
            number: Some(number),
            value,
            error: None,
        }
    }

    pub const fn failure(number: Option<SyscallNumber>, error: ExecError) -> Self {
        Self {
            number,
            value: 0,
            error: Some(error),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SyscallDispatcher;

impl SyscallDispatcher {
    /// Dispatch une requête contenue dans un trap frame et avance `sepc`.
    ///
    /// Seuls les appels d’introspection sont exécutables à cette étape. Les
    /// autres numéros connus renvoient `NotReady` jusqu’à l’ajout de leurs
    /// services et de leur validation des arguments U-Mode.
    pub fn dispatch(
        &self,
        frame: &mut SupervisorTrapFrame,
        caller_thread: ThreadId,
    ) -> SyscallOutcome {
        let number = u16::try_from(frame.x[17])
            .ok()
            .and_then(SyscallNumber::from_raw);
        let outcome = match number {
            Some(SyscallNumber::GetAbiVersion) => {
                SyscallOutcome::success(SyscallNumber::GetAbiVersion, ABI_VERSION)
            }
            Some(SyscallNumber::GetTaskId) => {
                SyscallOutcome::success(SyscallNumber::GetTaskId, caller_thread)
            }
            Some(number) => SyscallOutcome::failure(Some(number), ExecError::NotReady),
            None => SyscallOutcome::failure(None, ExecError::InvalidArgument),
        };

        frame.x[10] = outcome.value;
        frame.x[0] = 0;
        frame.sepc = frame.sepc.wrapping_add(4);
        outcome
    }

    /// Variante raccordée au runtime pour les primitives de threads.
    ///
    /// Convention des arguments : `a0=entry`, `a1=stack_size`, `a2=priority`,
    /// `a3=argument` pour `ThreadCreate`, puis `a0=thread`, `a1=deadline`
    /// (`0` signifiant aucune échéance) pour `ThreadJoin`.
    pub fn dispatch_runtime(
        &self,
        runtime: &mut ExecRuntime,
        frame: &mut SupervisorTrapFrame,
        caller_process: ProcessId,
        caller_thread: ThreadId,
    ) -> SyscallOutcome {
        let number = u16::try_from(frame.x[17])
            .ok()
            .and_then(SyscallNumber::from_raw);
        let outcome = match number {
            Some(SyscallNumber::Yield) => SyscallOutcome::success(SyscallNumber::Yield, 0),
            Some(SyscallNumber::ThreadCreate) => {
                let stack_size = match usize::try_from(frame.x[11]) {
                    Ok(value) => value,
                    Err(_) => {
                        return self.finish(
                            frame,
                            SyscallOutcome::failure(number, ExecError::InvalidArgument),
                        );
                    }
                };
                let priority = match i8::try_from(frame.x[12] as i64) {
                    Ok(value) => value,
                    Err(_) => {
                        return self.finish(
                            frame,
                            SyscallOutcome::failure(number, ExecError::InvalidArgument),
                        );
                    }
                };
                if validate_user_range(frame.x[10], 4).is_err() {
                    return self.finish(
                        frame,
                        SyscallOutcome::failure(number, ExecError::InvalidArgument),
                    );
                }
                if runtime
                    .info(caller_thread)
                    .map_or(true, |info| info.process != caller_process)
                {
                    SyscallOutcome::failure(number, ExecError::PermissionDenied)
                } else {
                    match runtime.create_thread_for_process(
                        ThreadCreateArgs {
                            process: caller_process,
                            entry: Address(frame.x[10]),
                            stack_size: ByteCount(stack_size),
                            priority,
                            argument: frame.x[13],
                        },
                        CpuAffinity::Any,
                        0,
                    ) {
                        Ok((thread, _hart)) => {
                            SyscallOutcome::success(SyscallNumber::ThreadCreate, thread)
                        }
                        Err(error) => SyscallOutcome::failure(number, error),
                    }
                }
            }
            Some(SyscallNumber::ThreadJoin) => {
                if frame.x[10] == caller_thread {
                    return self.finish(
                        frame,
                        SyscallOutcome::failure(number, ExecError::InvalidArgument),
                    );
                }
                let deadline = if frame.x[11] == 0 {
                    None
                } else {
                    Some(frame.x[11] as Timestamp)
                };
                match runtime.join_or_block(caller_process, caller_thread, frame.x[10], deadline) {
                    Ok(Some(code)) => {
                        SyscallOutcome::success(SyscallNumber::ThreadJoin, code as u64)
                    }
                    Ok(None) => SyscallOutcome::failure(number, ExecError::NotReady),
                    Err(error) => SyscallOutcome::failure(number, error),
                }
            }
            Some(SyscallNumber::ThreadExit) => {
                // Le code de sortie est signé dans l'ABI et transporté dans
                // le registre RV64 sous forme complément à deux.
                let code = frame.x[10] as i32;
                match runtime.exit_owned(caller_process, caller_thread, code) {
                    Ok(()) => SyscallOutcome::success(SyscallNumber::ThreadExit, 0),
                    Err(error) => SyscallOutcome::failure(number, error),
                }
            }
            _ => return self.dispatch(frame, caller_thread),
        };
        self.finish(frame, outcome)
    }

    /// Retourne au processus appelant un événement d'entrée déjà normalisé
    /// par le Device. `a0` est le buffer de sortie et `a1` sa taille.
    pub fn dispatch_runtime_with_input<M: UserMemory, S: InputEventSource>(
        &self,
        runtime: &mut ExecRuntime,
        frame: &mut SupervisorTrapFrame,
        caller_process: ProcessId,
        caller_thread: ThreadId,
        memory: &mut M,
        source: &mut S,
    ) -> SyscallOutcome {
        let number = u16::try_from(frame.x[17])
            .ok()
            .and_then(SyscallNumber::from_raw);
        if number != Some(SyscallNumber::PollInput) && number != Some(SyscallNumber::WaitInput) {
            return self.dispatch_runtime(runtime, frame, caller_process, caller_thread);
        }
        let is_wait = number == Some(SyscallNumber::WaitInput);
        let syscall = number.expect("input syscall number was checked");
        let window = u32::try_from(frame.x[12]).unwrap_or(0);
        let outcome = if runtime
            .info(caller_thread)
            .map_or(true, |info| info.process != caller_process)
        {
            SyscallOutcome::failure(number, ExecError::PermissionDenied)
        } else if frame.x[11] != INPUT_EVENT_WIRE_BYTES as u64 {
            SyscallOutcome::failure(number, ExecError::InvalidArgument)
        } else if is_wait && window == 0 {
            SyscallOutcome::failure(number, ExecError::InvalidArgument)
        } else if let Some(event) = if is_wait {
            source.pop_input_event_for(window)
        } else {
            source.pop_input_event()
        } {
            let bytes = event.to_le_bytes();
            match copy_to_user(memory, frame.x[10], &bytes) {
                Ok(_) => SyscallOutcome::success(syscall, 1),
                Err(error) => SyscallOutcome::failure(number, error),
            }
        } else if is_wait {
            match runtime.wait_for_input(window, caller_thread) {
                Ok(()) => SyscallOutcome::failure(number, ExecError::NotReady),
                Err(error) => SyscallOutcome::failure(number, error),
            }
        } else {
            SyscallOutcome::success(SyscallNumber::PollInput, 0)
        };
        self.finish(frame, outcome)
    }

    /// Raccorde les quatre opérations DOS nécessaires aux fichiers de
    /// préférence. Les buffers sont copiés dans des tableaux bornés avant
    /// d'atteindre le backend de stockage.
    pub fn dispatch_runtime_with_files<M: UserMemory, F: FileService>(
        &self,
        runtime: &mut ExecRuntime,
        frame: &mut SupervisorTrapFrame,
        caller_process: ProcessId,
        caller_thread: ThreadId,
        memory: &mut M,
        files: &mut F,
    ) -> SyscallOutcome {
        let number = u16::try_from(frame.x[17])
            .ok()
            .and_then(SyscallNumber::from_raw);
        if !matches!(
            number,
            Some(
                SyscallNumber::OpenFile
                    | SyscallNumber::ReadFile
                    | SyscallNumber::WriteFile
                    | SyscallNumber::CloseFile
            )
        ) {
            return self.dispatch_runtime(runtime, frame, caller_process, caller_thread);
        }
        if runtime
            .info(caller_thread)
            .map_or(true, |info| info.process != caller_process)
        {
            return self.finish(
                frame,
                SyscallOutcome::failure(number, ExecError::PermissionDenied),
            );
        }

        let outcome = match number {
            Some(SyscallNumber::OpenFile) => {
                let path_length = usize::try_from(frame.x[11]).unwrap_or(usize::MAX);
                if path_length == 0 || path_length > MAX_FILE_PATH_BYTES {
                    SyscallOutcome::failure(number, ExecError::InvalidArgument)
                } else {
                    let mut path = [0u8; MAX_FILE_PATH_BYTES];
                    match copy_from_user(memory, frame.x[10], &mut path[..path_length]) {
                        Ok(_) => {
                            match files.open(caller_process, &path[..path_length], frame.x[12]) {
                                Ok(handle) => {
                                    SyscallOutcome::success(SyscallNumber::OpenFile, handle)
                                }
                                Err(error) => SyscallOutcome::failure(number, error),
                            }
                        }
                        Err(error) => SyscallOutcome::failure(number, error),
                    }
                }
            }
            Some(SyscallNumber::ReadFile) => {
                file_read_outcome(files, memory, caller_process, frame, number)
            }
            Some(SyscallNumber::WriteFile) => {
                file_write_outcome(files, memory, caller_process, frame, number)
            }
            Some(SyscallNumber::CloseFile) => match files.close(caller_process, frame.x[10]) {
                Ok(()) => SyscallOutcome::success(SyscallNumber::CloseFile, 0),
                Err(error) => SyscallOutcome::failure(number, error),
            },
            _ => SyscallOutcome::failure(number, ExecError::InvalidArgument),
        };
        self.finish(frame, outcome)
    }

    /// Variante du dispatcher qui ouvre la frontière U-Mode → worker
    /// Graphics pour `PresentFrame` et `PollPresentFrame`.
    ///
    /// ABI : `a0=adresse de PresentFrame`, `a1=24` (taille wire fixe). Le
    /// paquet pointé par `PresentFrame.packet_address` est copié dans un
    /// tampon borné du kernel avant l'appel du sink. Les autres syscalls
    /// suivent exactement le chemin runtime habituel.
    pub fn dispatch_runtime_with_presenter<M: UserMemory, S: PresentFrameSink>(
        &self,
        runtime: &mut ExecRuntime,
        frame: &mut SupervisorTrapFrame,
        caller_process: ProcessId,
        caller_thread: ThreadId,
        memory: &M,
        sink: &mut S,
    ) -> SyscallOutcome {
        let number = u16::try_from(frame.x[17])
            .ok()
            .and_then(SyscallNumber::from_raw);
        if !matches!(
            number,
            Some(SyscallNumber::PresentFrame | SyscallNumber::PollPresentFrame)
        ) {
            return self.dispatch_runtime(runtime, frame, caller_process, caller_thread);
        }

        let outcome = if runtime
            .info(caller_thread)
            .map_or(true, |info| info.process != caller_process)
        {
            SyscallOutcome::failure(number, ExecError::PermissionDenied)
        } else if number == Some(SyscallNumber::PollPresentFrame) {
            if frame.x[10] != 0 || frame.x[11] != 0 {
                SyscallOutcome::failure(number, ExecError::InvalidArgument)
            } else {
                match sink.poll_present_frame(caller_process) {
                    Ok(status) => SyscallOutcome::success(SyscallNumber::PollPresentFrame, status),
                    Err(error) => SyscallOutcome::failure(number, error),
                }
            }
        } else if frame.x[11] != PRESENT_FRAME_WIRE_BYTES as u64 {
            SyscallOutcome::failure(number, ExecError::InvalidArgument)
        } else {
            let mut request_bytes = [0u8; PRESENT_FRAME_WIRE_BYTES];
            match copy_from_user(memory, frame.x[10], &mut request_bytes) {
                Ok(_) => {
                    let request = decode_present_frame(&request_bytes);
                    let packet_length = request.packet_length as usize;
                    let mut packet = [0u8; MAX_PRESENT_PACKET_BYTES];
                    match request.validate().and_then(|_| {
                        copy_from_user(memory, request.packet_address, &mut packet[..packet_length])
                            .map(|_| ())
                    }) {
                        Ok(()) => {
                            match validate_present_packet(request, &packet[..packet_length]) {
                                Ok(()) => match sink.submit_present_frame(
                                    caller_process,
                                    request,
                                    &packet[..packet_length],
                                ) {
                                    Ok(sequence) => SyscallOutcome::success(
                                        SyscallNumber::PresentFrame,
                                        sequence,
                                    ),
                                    Err(error) => SyscallOutcome::failure(number, error),
                                },
                                Err(error) => SyscallOutcome::failure(number, error),
                            }
                        }
                        Err(error) => SyscallOutcome::failure(number, error),
                    }
                }
                Err(error) => SyscallOutcome::failure(number, error),
            }
        };
        self.finish(frame, outcome)
    }

    /// Transporte uniquement l'état du sprite vers le worker d'affichage.
    /// Aucun paquet de scène ni surface applicative n'est copié par cet ABI.
    pub fn dispatch_runtime_with_cursor_overlay<S: CursorOverlaySink>(
        &self,
        runtime: &ExecRuntime,
        frame: &mut SupervisorTrapFrame,
        caller_process: ProcessId,
        caller_thread: ThreadId,
        memory: &mut dyn UserMemory,
        sink: &mut S,
    ) -> SyscallOutcome {
        let number = u16::try_from(frame.x[17])
            .ok()
            .and_then(SyscallNumber::from_raw);
        if number != Some(SyscallNumber::PresentCursorOverlay) {
            return self.dispatch(frame, caller_thread);
        }
        let outcome = if runtime
            .info(caller_thread)
            .map_or(true, |info| info.process != caller_process)
        {
            SyscallOutcome::failure(number, ExecError::PermissionDenied)
        } else if frame.x[11] != PRESENT_CURSOR_OVERLAY_WIRE_BYTES as u64 {
            SyscallOutcome::failure(number, ExecError::InvalidArgument)
        } else {
            let mut bytes = [0u8; PRESENT_CURSOR_OVERLAY_WIRE_BYTES];
            match copy_from_user(memory, frame.x[10], &mut bytes) {
                Ok(_) => {
                    let overlay = decode_cursor_overlay(&bytes);
                    match overlay
                        .validate()
                        .and_then(|_| sink.submit_cursor_overlay(caller_process, overlay))
                    {
                        Ok(sequence) => {
                            SyscallOutcome::success(SyscallNumber::PresentCursorOverlay, sequence)
                        }
                        Err(error) => SyscallOutcome::failure(number, error),
                    }
                }
                Err(error) => SyscallOutcome::failure(number, error),
            }
        };
        self.finish(frame, outcome)
    }

    /// Variante du dispatcher qui raccorde `CreateProcess` à un résolveur
    /// ExecFS/ELF et à une mémoire utilisateur validée.
    ///
    /// ABI V1 conservée : `a0=adresse du nom`, `a1=longueur NUL comprise`.
    /// Le retour V3 est l'identifiant `ProcessId` nouvellement publié ; la
    /// façade `dos.library` pourra ensuite le convertir en handle avec
    /// génération lorsqu'elle sera implémentée.
    pub fn dispatch_runtime_with_process_resolver<M: UserMemory, L: ProcessImageResolver>(
        &self,
        runtime: &mut ExecRuntime,
        frame: &mut SupervisorTrapFrame,
        caller_process: ProcessId,
        caller_thread: ThreadId,
        memory: &M,
        resolver: &mut L,
    ) -> SyscallOutcome {
        let number = u16::try_from(frame.x[17])
            .ok()
            .and_then(SyscallNumber::from_raw);
        if number != Some(SyscallNumber::CreateProcess) {
            return self.dispatch_runtime(runtime, frame, caller_process, caller_thread);
        }
        let outcome = if runtime
            .info(caller_thread)
            .map_or(true, |info| info.process != caller_process)
        {
            SyscallOutcome::failure(number, ExecError::PermissionDenied)
        } else {
            match copy_process_name(memory, frame.x[10], frame.x[11]) {
                Ok(name) => match resolver.resolve(caller_process, name.as_slice()) {
                    Ok(spec) => match spec.validate() {
                        Ok(()) => match runtime.create_process_auto(
                            spec.entry,
                            spec.stack_size,
                            spec.priority,
                            spec.argument,
                            spec.affinity,
                        ) {
                            Ok(process) => {
                                SyscallOutcome::success(SyscallNumber::CreateProcess, process.id)
                            }
                            Err(error) => SyscallOutcome::failure(number, error),
                        },
                        Err(error) => SyscallOutcome::failure(number, error),
                    },
                    Err(error) => SyscallOutcome::failure(number, error),
                },
                Err(error) => SyscallOutcome::failure(number, error),
            }
        };
        self.finish(frame, outcome)
    }

    /// Variante complète de `CreateProcess` : résolution du nom utilisateur,
    /// chargement ELF/MFFS, pile privée et publication atomique du processus.
    ///
    /// `base_plan` contient les mappings supervisor communs (et éventuellement
    /// les mappings de services). Le chargeur y ajoute les segments de l'image;
    /// Exec ajoute ensuite la pile privée avant de construire le contexte.
    pub fn dispatch_runtime_with_process_image<
        M: UserMemory,
        L: ProcessImageLoader,
        W: ProcessImagePageWriter,
    >(
        &self,
        runtime: &mut ExecRuntime,
        frame: &mut SupervisorTrapFrame,
        caller_process: ProcessId,
        caller_thread: ThreadId,
        memory: &M,
        loader: &L,
        allocator: &mut FrameAllocator,
        base_plan: AddressSpacePlan,
        satp: u64,
        writer: &mut W,
    ) -> SyscallOutcome {
        let number = u16::try_from(frame.x[17])
            .ok()
            .and_then(SyscallNumber::from_raw);
        if number != Some(SyscallNumber::CreateProcess) {
            return self.dispatch_runtime(runtime, frame, caller_process, caller_thread);
        }
        let outcome = if runtime
            .info(caller_thread)
            .map_or(true, |info| info.process != caller_process)
        {
            SyscallOutcome::failure(number, ExecError::PermissionDenied)
        } else {
            match copy_process_name(memory, frame.x[10], frame.x[11]) {
                Ok(name) => {
                    let mut plan = base_plan;
                    let allocator_checkpoint = *allocator;
                    let loaded = loader.load_into(name.as_slice(), allocator, &mut plan, writer);
                    match loaded {
                        Ok(spec) => match runtime.create_process_auto_loaded(
                            spec.entry,
                            spec.stack_size,
                            spec.priority,
                            spec.argument,
                            spec.affinity,
                            satp,
                            plan,
                            allocator,
                            writer,
                        ) {
                            Ok(process) => {
                                SyscallOutcome::success(SyscallNumber::CreateProcess, process.id)
                            }
                            Err(error) => {
                                *allocator = allocator_checkpoint;
                                SyscallOutcome::failure(number, error)
                            }
                        },
                        Err(error) => {
                            // Le résolveur MFFS restaure déjà ses propres
                            // réservations ; cette affectation couvre aussi
                            // les chargeurs tiers conformes au contrat.
                            *allocator = allocator_checkpoint;
                            SyscallOutcome::failure(number, error)
                        }
                    }
                }
                Err(error) => SyscallOutcome::failure(number, error),
            }
        };
        self.finish(frame, outcome)
    }

    fn finish(&self, frame: &mut SupervisorTrapFrame, outcome: SyscallOutcome) -> SyscallOutcome {
        frame.x[10] = outcome.value;
        frame.x[0] = 0;
        frame.sepc = frame.sepc.wrapping_add(4);
        outcome
    }

    /// Décode le `scause` produit par le trap handler avant le retour assembleur.
    pub fn handle_trap(
        &self,
        frame: &mut SupervisorTrapFrame,
        scause: u64,
        caller_thread: ThreadId,
    ) -> TrapDisposition {
        match scause {
            SCAUSE_USER_ECALL => TrapDisposition::Syscall(self.dispatch(frame, caller_thread)),
            SCAUSE_SUPERVISOR_TIMER => TrapDisposition::TimerInterrupt,
            SCAUSE_SUPERVISOR_EXTERNAL => TrapDisposition::ExternalInterrupt,
            SCAUSE_SUPERVISOR_SOFTWARE => TrapDisposition::SoftwareInterrupt,
            SCAUSE_STORE_PAGE_FAULT if is_user_store_page_fault(scause, frame.sstatus) => {
                TrapDisposition::UserStorePageFault(Address(frame.stval))
            }
            _ => TrapDisposition::Fault,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestMemory {
        bytes: [u8; 16],
        readable: bool,
        writable: bool,
        short_copy: bool,
    }

    struct PresentMemory {
        bytes: [u8; 2048],
    }

    struct InputMemory {
        bytes: [u8; 64],
    }

    impl UserMemory for PresentMemory {
        fn validate(&self, range: UserRange, _access: UserAccess) -> ExecResult<()> {
            let start = range
                .address
                .checked_sub(USER_ADDRESS_MIN)
                .ok_or(ExecError::InvalidArgument)? as usize;
            let end = start
                .checked_add(range.length as usize)
                .ok_or(ExecError::InvalidArgument)?;
            if end <= self.bytes.len() {
                Ok(())
            } else {
                Err(ExecError::Fault)
            }
        }

        fn read(&self, range: UserRange, destination: &mut [u8]) -> ExecResult<usize> {
            let start = (range.address - USER_ADDRESS_MIN) as usize;
            destination.copy_from_slice(&self.bytes[start..start + destination.len()]);
            Ok(destination.len())
        }

        fn write(&mut self, _range: UserRange, _source: &[u8]) -> ExecResult<usize> {
            Err(ExecError::PermissionDenied)
        }
    }

    impl UserMemory for InputMemory {
        fn validate(&self, range: UserRange, access: UserAccess) -> ExecResult<()> {
            if access != UserAccess::Write {
                return Err(ExecError::PermissionDenied);
            }
            let start = range
                .address
                .checked_sub(USER_ADDRESS_MIN)
                .ok_or(ExecError::InvalidArgument)? as usize;
            let end = start
                .checked_add(range.length as usize)
                .ok_or(ExecError::InvalidArgument)?;
            if end <= self.bytes.len() {
                Ok(())
            } else {
                Err(ExecError::Fault)
            }
        }

        fn read(&self, _range: UserRange, _destination: &mut [u8]) -> ExecResult<usize> {
            Err(ExecError::PermissionDenied)
        }

        fn write(&mut self, range: UserRange, source: &[u8]) -> ExecResult<usize> {
            let start = (range.address - USER_ADDRESS_MIN) as usize;
            self.bytes[start..start + source.len()].copy_from_slice(source);
            Ok(source.len())
        }
    }

    struct TestInputSource {
        event: Option<InputEventWire>,
    }

    struct TestFileService {
        data: [u8; 16],
        opened: bool,
    }

    impl FileService for TestFileService {
        fn open(&mut self, _process: ProcessId, _path: &[u8], _mode: u64) -> ExecResult<u64> {
            self.opened = true;
            Ok(7)
        }

        fn read(
            &mut self,
            _process: ProcessId,
            handle: u64,
            offset: u64,
            destination: &mut [u8],
        ) -> ExecResult<usize> {
            if !self.opened || handle != 7 {
                return Err(ExecError::InvalidHandle);
            }
            let offset = usize::try_from(offset).map_err(|_| ExecError::InvalidArgument)?;
            let end = offset
                .checked_add(destination.len())
                .ok_or(ExecError::InvalidArgument)?;
            destination.copy_from_slice(self.data.get(offset..end).ok_or(ExecError::Fault)?);
            Ok(destination.len())
        }

        fn write(
            &mut self,
            _process: ProcessId,
            handle: u64,
            offset: u64,
            source: &[u8],
        ) -> ExecResult<usize> {
            if !self.opened || handle != 7 {
                return Err(ExecError::InvalidHandle);
            }
            let offset = usize::try_from(offset).map_err(|_| ExecError::InvalidArgument)?;
            let end = offset
                .checked_add(source.len())
                .ok_or(ExecError::InvalidArgument)?;
            self.data
                .get_mut(offset..end)
                .ok_or(ExecError::Fault)?
                .copy_from_slice(source);
            Ok(source.len())
        }

        fn close(&mut self, _process: ProcessId, handle: u64) -> ExecResult<()> {
            if handle != 7 {
                return Err(ExecError::InvalidHandle);
            }
            self.opened = false;
            Ok(())
        }
    }

    impl InputEventSource for TestInputSource {
        fn pop_input_event(&mut self) -> Option<InputEventWire> {
            self.event.take()
        }
    }

    struct TestPresentSink {
        process: ProcessId,
        frame: Option<PresentFrame>,
        packet: [u8; MAX_PRESENT_PACKET_BYTES],
        packet_length: usize,
    }

    impl PresentFrameSink for TestPresentSink {
        fn submit_present_frame(
            &mut self,
            process: ProcessId,
            frame: PresentFrame,
            packet: &[u8],
        ) -> ExecResult<u64> {
            self.process = process;
            self.frame = Some(frame);
            self.packet_length = packet.len();
            self.packet[..packet.len()].copy_from_slice(packet);
            Ok(41)
        }
    }

    impl UserMemory for TestMemory {
        fn validate(&self, _range: UserRange, access: UserAccess) -> ExecResult<()> {
            match access {
                UserAccess::Read if self.readable => Ok(()),
                UserAccess::Write if self.writable => Ok(()),
                _ => Err(ExecError::PermissionDenied),
            }
        }

        fn read(&self, _range: UserRange, destination: &mut [u8]) -> ExecResult<usize> {
            let count = if self.short_copy {
                destination.len().saturating_sub(1)
            } else {
                destination.len()
            };
            destination[..count].copy_from_slice(&self.bytes[..count]);
            Ok(count)
        }

        fn write(&mut self, _range: UserRange, source: &[u8]) -> ExecResult<usize> {
            let count = if self.short_copy {
                source.len().saturating_sub(1)
            } else {
                source.len()
            };
            self.bytes[..count].copy_from_slice(&source[..count]);
            Ok(count)
        }
    }

    struct TestResolver {
        expected: &'static [u8],
    }

    impl ProcessImageResolver for TestResolver {
        fn resolve(&mut self, _parent: ProcessId, name: &[u8]) -> ExecResult<ProcessLaunchSpec> {
            if name != self.expected {
                return Err(ExecError::NotFound);
            }
            Ok(ProcessLaunchSpec {
                entry: Address(0x4000),
                stack_size: ByteCount(4096),
                priority: 2,
                argument: 0,
                affinity: CpuAffinity::Any,
            })
        }
    }

    struct TestPageWriter {
        fail: bool,
        zeroed: usize,
        written: usize,
        first_bytes: [u8; 4],
    }

    impl ProcessImagePageWriter for TestPageWriter {
        fn zero_page(&mut self, _physical: Address) -> ExecResult<()> {
            if self.fail {
                return Err(ExecError::Fault);
            }
            self.zeroed += 1;
            Ok(())
        }

        fn write_page(&mut self, _physical: Address, bytes: &[u8]) -> ExecResult<()> {
            if self.fail {
                return Err(ExecError::Fault);
            }
            self.written += bytes.len();
            let count = bytes.len().min(self.first_bytes.len());
            self.first_bytes[..count].copy_from_slice(&bytes[..count]);
            Ok(())
        }
    }

    fn put16(data: &mut [u8], offset: usize, value: u16) {
        data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn put32(data: &mut [u8], offset: usize, value: u32) {
        data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn put64(data: &mut [u8], offset: usize, value: u64) {
        data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }

    fn mffs_with_valid_app() -> [u8; 216] {
        let mut image = [0; 216];
        image[..4].copy_from_slice(b"EXFS");
        put16(&mut image, 4, 1);
        put16(&mut image, 6, 512);
        put16(&mut image, 8, 1);
        put16(&mut image, 10, 64);
        put32(&mut image, 12, 32);
        let image_size = image.len() as u32;
        put32(&mut image, 20, image_size);
        image[32..41].copy_from_slice(b"Demo.app\0");
        image[64] = FILE_TYPE_APP;
        put16(&mut image, 66, 0o555);
        put32(&mut image, 76, 96);
        put32(&mut image, 80, 120);
        put16(&mut image, 84, 1);

        let elf = &mut image[96..];
        elf[..4].copy_from_slice(b"\x7fELF");
        elf[4] = 2;
        elf[5] = 1;
        elf[6] = 1;
        put16(elf, 16, 2);
        put16(elf, 18, 243);
        put32(elf, 20, 1);
        put64(elf, 24, 0x10000);
        put64(elf, 32, 64);
        put16(elf, 52, 64);
        put16(elf, 54, 56);
        put16(elf, 56, 1);
        put32(elf, 64, 1);
        put32(elf, 68, 5);
        put64(elf, 72, 0);
        put64(elf, 80, 0x10000);
        put64(elf, 96, 120);
        put64(elf, 104, 4096);
        put64(elf, 112, 4096);
        let checksum = filesystem_v4::fnv1a(elf);
        put32(&mut image, 88, checksum);
        image
    }

    #[test]
    fn dispatches_abi_introspection_and_advances_ecall() {
        let mut frame = SupervisorTrapFrame::empty();
        frame.sepc = 0x1000;
        frame.x[17] = SyscallNumber::GetTaskId as u64;
        frame.x[0] = u64::MAX;

        let outcome = SyscallDispatcher.dispatch(&mut frame, 42);

        assert_eq!(
            outcome,
            SyscallOutcome::success(SyscallNumber::GetTaskId, 42)
        );
        assert_eq!(frame.x[10], 42);
        assert_eq!(frame.x[0], 0);
        assert_eq!(frame.sepc, 0x1004);
    }

    #[test]
    fn known_but_unimplemented_syscall_is_explicitly_not_ready() {
        let mut frame = SupervisorTrapFrame::empty();
        frame.x[17] = SyscallNumber::ThreadCreate as u64;

        let outcome = SyscallDispatcher.dispatch(&mut frame, 1);

        assert_eq!(outcome.number, Some(SyscallNumber::ThreadCreate));
        assert_eq!(outcome.error, Some(ExecError::NotReady));
        assert_eq!(frame.sepc, 4);
    }

    #[test]
    fn present_frame_copies_and_submits_a_bounded_user_packet() {
        assert_eq!(
            core::mem::size_of::<PresentFrame>(),
            PRESENT_FRAME_WIRE_BYTES
        );
        let mut memory = PresentMemory { bytes: [0; 2048] };
        let packet_address = USER_ADDRESS_MIN + 64;
        // Les états de passe V3 traversent Exec sans être interprétés ici.
        memory.bytes[64] = 13;
        memory.bytes[64 + 1] = 4;
        memory.bytes[64 + 24..64 + PRESENT_COMMAND_BYTES].fill(0);
        memory.bytes[0..8].copy_from_slice(&packet_address.to_le_bytes());
        memory.bytes[8..12].copy_from_slice(&(PRESENT_COMMAND_BYTES as u32).to_le_bytes());
        memory.bytes[12..14].copy_from_slice(&(1u16).to_le_bytes());
        memory.bytes[16..24].copy_from_slice(&(9u64).to_le_bytes());

        let mut runtime = ExecRuntime::qemu();
        let process = runtime
            .create_process(7, Address(0x1000), ByteCount(4096), 1, 0, CpuAffinity::Any)
            .expect("process");
        runtime.mark_running(process.main_thread).expect("running");
        let mut sink = TestPresentSink {
            process: 0,
            frame: None,
            packet: [0; MAX_PRESENT_PACKET_BYTES],
            packet_length: 0,
        };
        let mut trap = SupervisorTrapFrame::empty();
        trap.sepc = 0x4000;
        trap.x[10] = USER_ADDRESS_MIN;
        trap.x[11] = PRESENT_FRAME_WIRE_BYTES as u64;
        trap.x[17] = SyscallNumber::PresentFrame as u64;

        let outcome = SyscallDispatcher.dispatch_runtime_with_presenter(
            &mut runtime,
            &mut trap,
            7,
            process.main_thread,
            &memory,
            &mut sink,
        );

        assert_eq!(
            outcome,
            SyscallOutcome::success(SyscallNumber::PresentFrame, 41)
        );
        assert_eq!(trap.sepc, 0x4004);
        assert_eq!(sink.process, 7);
        assert_eq!(sink.frame.expect("frame").generation, 9);
        assert_eq!(sink.packet_length, PRESENT_COMMAND_BYTES);
        assert_eq!(sink.packet[0], 13);
    }

    #[test]
    fn file_syscalls_validate_user_buffers_and_route_to_dos_service() {
        let mut memory = TestMemory {
            bytes: [0; 16],
            readable: true,
            writable: true,
            short_copy: false,
        };
        memory.bytes[..3].copy_from_slice(b"cfg");
        let mut files = TestFileService {
            data: [0; 16],
            opened: false,
        };
        let mut runtime = ExecRuntime::qemu();
        let process = runtime
            .create_process(7, Address(0x1000), ByteCount(4096), 1, 0, CpuAffinity::Any)
            .expect("process");
        runtime.mark_running(process.main_thread).expect("running");
        let mut trap = SupervisorTrapFrame::empty();
        trap.sepc = 0x4000;
        trap.x[10] = USER_ADDRESS_MIN;
        trap.x[11] = 3;
        trap.x[12] = FILE_OPEN_WRITE_CREATE;
        trap.x[17] = SyscallNumber::OpenFile as u64;

        let opened = SyscallDispatcher.dispatch_runtime_with_files(
            &mut runtime,
            &mut trap,
            7,
            process.main_thread,
            &mut memory,
            &mut files,
        );
        assert_eq!(opened, SyscallOutcome::success(SyscallNumber::OpenFile, 7));

        memory.bytes[..4].copy_from_slice(b"data");
        trap.x[10] = 7;
        trap.x[11] = USER_ADDRESS_MIN;
        trap.x[12] = 4;
        trap.x[13] = 2;
        trap.x[17] = SyscallNumber::WriteFile as u64;
        let written = SyscallDispatcher.dispatch_runtime_with_files(
            &mut runtime,
            &mut trap,
            7,
            process.main_thread,
            &mut memory,
            &mut files,
        );
        assert_eq!(
            written,
            SyscallOutcome::success(SyscallNumber::WriteFile, 4)
        );
        assert_eq!(&files.data[2..6], b"data");

        trap.x[10] = 7;
        trap.x[11] = USER_ADDRESS_MIN;
        trap.x[12] = 4;
        trap.x[13] = 2;
        trap.x[17] = SyscallNumber::ReadFile as u64;
        let read = SyscallDispatcher.dispatch_runtime_with_files(
            &mut runtime,
            &mut trap,
            7,
            process.main_thread,
            &mut memory,
            &mut files,
        );
        assert_eq!(read, SyscallOutcome::success(SyscallNumber::ReadFile, 4));
        assert_eq!(&memory.bytes[..4], b"data");
    }

    #[test]
    fn present_packet_accepts_fader_values_above_text_id_range() {
        let mut command = [0u8; PRESENT_COMMAND_BYTES];
        command[0] = 9;
        command[12..16].copy_from_slice(&62u32.to_le_bytes());
        command[16..20].copy_from_slice(&0u32.to_le_bytes());
        command[20..24].copy_from_slice(&100u32.to_le_bytes());
        let frame = PresentFrame::new(USER_ADDRESS_MIN, PRESENT_COMMAND_BYTES as u32, 1, 1);

        assert_eq!(validate_present_packet(frame, &command), Ok(()));
    }

    #[test]
    fn present_packet_accepts_led_commands() {
        let mut command = [0u8; PRESENT_COMMAND_BYTES];
        command[0] = 17;
        command[1] = 8;
        command[12..16].copy_from_slice(&0xFFDC900Cu32.to_le_bytes());
        command[16..20].copy_from_slice(&18u32.to_le_bytes());
        let frame = PresentFrame::new(USER_ADDRESS_MIN, PRESENT_COMMAND_BYTES as u32, 1, 1);

        assert_eq!(validate_present_packet(frame, &command), Ok(()));
    }

    #[test]
    fn present_packet_accepts_scene3d_rotation_bits() {
        let mut command = [0u8; PRESENT_COMMAND_BYTES];
        command[0] = 18;
        command[4..12].copy_from_slice(&[0, 0, 0, 0, 64, 0, 64, 0]);
        command[12..16].copy_from_slice(&1.25f32.to_bits().to_le_bytes());
        let frame = PresentFrame::new(USER_ADDRESS_MIN, PRESENT_COMMAND_BYTES as u32, 1, 1);

        assert_eq!(validate_present_packet(frame, &command), Ok(()));
    }

    #[test]
    fn present_packet_accepts_screen_space_widget_flags_only_on_supported_widgets() {
        let mut command = [0u8; PRESENT_COMMAND_BYTES];
        command[0] = 7;
        command[1] = PRESENT_FLAG_SCREEN_SPACE;
        let frame = PresentFrame::new(USER_ADDRESS_MIN, PRESENT_COMMAND_BYTES as u32, 1, 1);
        assert_eq!(validate_present_packet(frame, &command), Ok(()));

        command[0] = 9;
        assert_eq!(
            validate_present_packet(frame, &command),
            Err(ExecError::InvalidArgument)
        );
    }

    #[test]
    fn poll_input_copies_one_wire_event_and_returns_empty_afterwards() {
        assert_eq!(
            core::mem::size_of::<InputEventWire>(),
            INPUT_EVENT_WIRE_BYTES
        );
        let event = InputEventWire {
            kind: InputEventWire::KIND_KEY,
            phase: InputEventWire::PHASE_DOWN,
            flags: InputEventWire::FLAG_SHIFT,
            code: 1,
            timestamp: 77,
            ..InputEventWire::default()
        };
        let mut memory = InputMemory { bytes: [0; 64] };
        let mut source = TestInputSource { event: Some(event) };
        let mut runtime = ExecRuntime::qemu();
        let process = runtime
            .create_process(7, Address(0x1000), ByteCount(4096), 1, 0, CpuAffinity::Any)
            .expect("process");
        let mut trap = SupervisorTrapFrame::empty();
        trap.sepc = 0x4000;
        trap.x[10] = USER_ADDRESS_MIN;
        trap.x[11] = INPUT_EVENT_WIRE_BYTES as u64;
        trap.x[17] = SyscallNumber::PollInput as u64;

        let outcome = SyscallDispatcher.dispatch_runtime_with_input(
            &mut runtime,
            &mut trap,
            7,
            process.main_thread,
            &mut memory,
            &mut source,
        );
        assert_eq!(
            outcome,
            SyscallOutcome::success(SyscallNumber::PollInput, 1)
        );
        assert_eq!(trap.sepc, 0x4004);
        assert_eq!(memory.bytes[..INPUT_EVENT_WIRE_BYTES], event.to_le_bytes());

        let empty = SyscallDispatcher.dispatch_runtime_with_input(
            &mut runtime,
            &mut trap,
            7,
            process.main_thread,
            &mut memory,
            &mut source,
        );
        assert_eq!(empty, SyscallOutcome::success(SyscallNumber::PollInput, 0));
    }

    #[test]
    fn wait_input_blocks_and_is_retryable_when_source_is_empty() {
        let mut memory = InputMemory { bytes: [0; 64] };
        let mut source = TestInputSource { event: None };
        let mut runtime = ExecRuntime::qemu();
        let process = runtime
            .create_process(7, Address(0x1000), ByteCount(4096), 1, 0, CpuAffinity::Any)
            .expect("process");
        runtime.mark_running(process.main_thread).expect("running");
        let mut trap = SupervisorTrapFrame::empty();
        trap.sepc = 0x5000;
        trap.x[10] = USER_ADDRESS_MIN;
        trap.x[11] = INPUT_EVENT_WIRE_BYTES as u64;
        trap.x[12] = 1;
        trap.x[17] = SyscallNumber::WaitInput as u64;

        let outcome = SyscallDispatcher.dispatch_runtime_with_input(
            &mut runtime,
            &mut trap,
            7,
            process.main_thread,
            &mut memory,
            &mut source,
        );

        assert_eq!(
            outcome,
            SyscallOutcome::failure(Some(SyscallNumber::WaitInput), ExecError::NotReady)
        );
        assert_eq!(trap.sepc, 0x5004);
        assert_eq!(
            runtime.info(process.main_thread).unwrap().state,
            crate::ThreadState::Blocked
        );
        assert_eq!(runtime.input_waits.len(1), 1);
    }

    #[test]
    fn present_frame_rejects_invalid_opcode_before_reaching_the_sink() {
        let mut memory = PresentMemory { bytes: [0; 2048] };
        let packet_address = USER_ADDRESS_MIN + 64;
        memory.bytes[64] = 19;
        memory.bytes[0..8].copy_from_slice(&packet_address.to_le_bytes());
        memory.bytes[8..12].copy_from_slice(&(24u32).to_le_bytes());
        memory.bytes[16..24].copy_from_slice(&(1u64).to_le_bytes());

        let mut runtime = ExecRuntime::qemu();
        let process = runtime
            .create_process(7, Address(0x1000), ByteCount(4096), 1, 0, CpuAffinity::Any)
            .expect("process");
        let mut sink = TestPresentSink {
            process: 0,
            frame: None,
            packet: [0; MAX_PRESENT_PACKET_BYTES],
            packet_length: 0,
        };
        let mut trap = SupervisorTrapFrame::empty();
        trap.x[10] = USER_ADDRESS_MIN;
        trap.x[11] = PRESENT_FRAME_WIRE_BYTES as u64;
        trap.x[17] = SyscallNumber::PresentFrame as u64;

        let outcome = SyscallDispatcher.dispatch_runtime_with_presenter(
            &mut runtime,
            &mut trap,
            7,
            process.main_thread,
            &memory,
            &mut sink,
        );

        assert_eq!(outcome.error, Some(ExecError::InvalidArgument));
        assert!(sink.frame.is_none());
    }

    #[test]
    fn runtime_yield_is_a_successful_scheduler_boundary() {
        let mut runtime = ExecRuntime::qemu();
        let process = runtime
            .create_process(7, Address(0x1000), ByteCount(4096), 1, 0, CpuAffinity::Any)
            .expect("process");
        let mut frame = SupervisorTrapFrame::empty();
        frame.sepc = 0x2000;
        frame.x[17] = SyscallNumber::Yield as u64;

        let outcome =
            SyscallDispatcher.dispatch_runtime(&mut runtime, &mut frame, 7, process.main_thread);

        assert_eq!(outcome, SyscallOutcome::success(SyscallNumber::Yield, 0));
        assert_eq!(frame.sepc, 0x2004);
        assert_eq!(frame.x[10], 0);
    }

    #[test]
    fn runtime_thread_exit_uses_current_thread_and_signed_code() {
        let mut runtime = ExecRuntime::qemu();
        let (thread, _) = runtime
            .create_thread(
                ThreadCreateArgs {
                    process: 7,
                    entry: Address(0x1000),
                    stack_size: ByteCount(4096),
                    priority: 1,
                    argument: 0,
                },
                CpuAffinity::Any,
            )
            .expect("create");
        let mut frame = SupervisorTrapFrame::empty();
        frame.x[17] = SyscallNumber::ThreadExit as u64;
        frame.x[10] = (-3i32) as u64;

        let outcome = SyscallDispatcher.dispatch_runtime(&mut runtime, &mut frame, 7, thread);

        assert_eq!(
            outcome,
            SyscallOutcome::success(SyscallNumber::ThreadExit, 0)
        );
        assert_eq!(runtime.join(7, thread, None), Ok(-3));
    }

    #[test]
    fn unknown_syscall_is_rejected_without_truncating_the_number() {
        let mut frame = SupervisorTrapFrame::empty();
        frame.x[17] = u64::from(u16::MAX) + 1;

        let outcome = SyscallDispatcher.dispatch(&mut frame, 1);

        assert_eq!(
            outcome,
            SyscallOutcome::failure(None, ExecError::InvalidArgument)
        );
    }

    #[test]
    fn user_range_rejects_kernel_space_and_overflow() {
        assert_eq!(
            validate_user_range(USER_ADDRESS_MIN, 4096),
            Ok(UserRange {
                address: USER_ADDRESS_MIN,
                length: 4096
            })
        );
        assert_eq!(
            validate_user_range(USER_ADDRESS_MAX, 2),
            Err(ExecError::InvalidArgument)
        );
        assert_eq!(
            validate_user_range(USER_ADDRESS_MAX + 1, 1),
            Err(ExecError::InvalidArgument)
        );
        assert_eq!(
            validate_user_range(u64::MAX, u64::MAX),
            Err(ExecError::InvalidArgument)
        );
    }

    #[test]
    fn empty_user_range_is_safe_without_dereferencing_zero() {
        assert_eq!(
            validate_user_range(0, 0),
            Ok(UserRange {
                address: 0,
                length: 0
            })
        );
    }

    #[test]
    fn copies_only_after_read_permission_validation() {
        let memory = TestMemory {
            bytes: *b"exec-user-memory",
            readable: true,
            writable: false,
            short_copy: false,
        };
        let mut destination = [0; 16];
        assert_eq!(
            copy_from_user(&memory, USER_ADDRESS_MIN, &mut destination),
            Ok(16)
        );
        assert_eq!(&destination, b"exec-user-memory");
    }

    #[test]
    fn rejects_write_without_permission_and_short_copy() {
        let mut read_only = TestMemory {
            bytes: [0; 16],
            readable: true,
            writable: false,
            short_copy: false,
        };
        assert_eq!(
            copy_to_user(&mut read_only, USER_ADDRESS_MIN, b"0123456789abcdef"),
            Err(ExecError::PermissionDenied)
        );

        let mut short = TestMemory {
            bytes: [0; 16],
            readable: false,
            writable: true,
            short_copy: true,
        };
        assert_eq!(
            copy_to_user(&mut short, USER_ADDRESS_MIN, b"0123456789abcdef"),
            Err(ExecError::Fault)
        );
    }

    #[test]
    fn trap_handler_routes_ecall_and_distinguishes_interrupts() {
        let mut frame = SupervisorTrapFrame::empty();
        frame.x[17] = SyscallNumber::GetAbiVersion as u64;
        assert_eq!(
            SyscallDispatcher.handle_trap(&mut frame, SCAUSE_USER_ECALL, 7),
            TrapDisposition::Syscall(SyscallOutcome::success(
                SyscallNumber::GetAbiVersion,
                ABI_VERSION
            ))
        );
        assert_eq!(
            SyscallDispatcher.handle_trap(&mut frame, SCAUSE_SUPERVISOR_TIMER, 7),
            TrapDisposition::TimerInterrupt
        );
        assert_eq!(
            SyscallDispatcher.handle_trap(&mut frame, 2, 7),
            TrapDisposition::Fault
        );
    }

    #[test]
    fn trap_handler_exposes_user_store_page_fault_address() {
        let mut frame = SupervisorTrapFrame::empty();
        frame.stval = 0x8123;
        assert!(is_user_store_page_fault(
            SCAUSE_STORE_PAGE_FAULT,
            frame.sstatus
        ));
        assert_eq!(
            SyscallDispatcher.handle_trap(&mut frame, SCAUSE_STORE_PAGE_FAULT, 7),
            TrapDisposition::UserStorePageFault(Address(0x8123))
        );
    }

    #[test]
    fn supervisor_store_page_fault_is_not_a_user_cow_fault() {
        let mut frame = SupervisorTrapFrame::empty();
        frame.sstatus = SSTATUS_SPP;
        frame.stval = 0x8123;
        assert!(!is_user_store_page_fault(
            SCAUSE_STORE_PAGE_FAULT,
            frame.sstatus
        ));
        assert_eq!(
            SyscallDispatcher.handle_trap(&mut frame, SCAUSE_STORE_PAGE_FAULT, 7),
            TrapDisposition::Fault
        );
    }

    #[test]
    fn runtime_dispatch_creates_and_joins_a_thread() {
        let mut runtime = ExecRuntime::qemu();
        let process = runtime
            .create_process(7, Address(0x1000), ByteCount(4096), 10, 0, CpuAffinity::Any)
            .expect("process");
        let mut frame = SupervisorTrapFrame::empty();
        frame.sepc = 0x2000;
        frame.x[17] = SyscallNumber::ThreadCreate as u64;
        frame.x[10] = 0x3000;
        frame.x[11] = 4096;
        frame.x[12] = 4;
        frame.x[13] = 99;

        let created =
            SyscallDispatcher.dispatch_runtime(&mut runtime, &mut frame, 7, process.main_thread);
        assert_eq!(created.error, None);
        let thread = created.value;
        assert_eq!(frame.sepc, 0x2004);

        runtime.exit(thread, 23).expect("exit");
        frame.x[17] = SyscallNumber::ThreadJoin as u64;
        frame.x[10] = thread;
        frame.x[11] = 0;
        let joined =
            SyscallDispatcher.dispatch_runtime(&mut runtime, &mut frame, 7, process.main_thread);
        assert_eq!(
            joined,
            SyscallOutcome::success(SyscallNumber::ThreadJoin, 23)
        );
    }

    #[test]
    fn runtime_thread_create_checks_caller_ownership() {
        let mut runtime = ExecRuntime::qemu();
        let process = runtime
            .create_process(7, Address(0x1000), ByteCount(4096), 10, 0, CpuAffinity::Any)
            .expect("process");
        let mut frame = SupervisorTrapFrame::empty();
        frame.x[17] = SyscallNumber::ThreadCreate as u64;
        frame.x[10] = 0x3000;
        frame.x[11] = 4096;

        let outcome =
            SyscallDispatcher.dispatch_runtime(&mut runtime, &mut frame, 8, process.main_thread);
        assert_eq!(outcome.error, Some(ExecError::PermissionDenied));
    }

    #[test]
    fn create_process_keeps_v1_filename_abi_and_allocates_a_process_id() {
        let mut runtime = ExecRuntime::qemu();
        let parent = runtime
            .create_process(7, Address(0x1000), ByteCount(4096), 10, 0, CpuAffinity::Any)
            .expect("parent");
        let memory = TestMemory {
            bytes: *b"Demo.app\0       ",
            readable: true,
            writable: false,
            short_copy: false,
        };
        let mut resolver = TestResolver {
            expected: b"Demo.app\0",
        };
        let mut frame = SupervisorTrapFrame::empty();
        frame.sepc = 0x2000;
        frame.x[17] = SyscallNumber::CreateProcess as u64;
        frame.x[10] = USER_ADDRESS_MIN;
        frame.x[11] = 9;

        let outcome = SyscallDispatcher.dispatch_runtime_with_process_resolver(
            &mut runtime,
            &mut frame,
            7,
            parent.main_thread,
            &memory,
            &mut resolver,
        );

        assert_eq!(
            outcome,
            SyscallOutcome::success(SyscallNumber::CreateProcess, 1)
        );
        assert_eq!(frame.sepc, 0x2004);
        assert_eq!(runtime.process_info(1).expect("child").main_thread, 2);
    }

    #[test]
    fn mffs_resolver_validates_app_and_feeds_create_process() {
        let image = mffs_with_valid_app();
        let mut resolver = MffsProcessImageResolver::new(&image).expect("catalogue");
        let spec = resolver.resolve(7, b"Demo.app\0").expect("app");
        assert_eq!(spec.entry, Address(0x10000));
        assert_eq!(spec.stack_size, ByteCount(96 * 1024));

        let mut runtime = ExecRuntime::qemu();
        let parent = runtime
            .create_process(7, Address(0x1000), ByteCount(4096), 10, 0, CpuAffinity::Any)
            .expect("parent");
        let memory = TestMemory {
            bytes: *b"Demo.app\0       ",
            readable: true,
            writable: false,
            short_copy: false,
        };
        let mut frame = SupervisorTrapFrame::empty();
        frame.x[17] = SyscallNumber::CreateProcess as u64;
        frame.x[10] = USER_ADDRESS_MIN;
        frame.x[11] = 9;

        let outcome = SyscallDispatcher.dispatch_runtime_with_process_resolver(
            &mut runtime,
            &mut frame,
            7,
            parent.main_thread,
            &memory,
            &mut resolver,
        );

        assert_eq!(
            outcome,
            SyscallOutcome::success(SyscallNumber::CreateProcess, 1)
        );
        assert_eq!(runtime.process_info(1).expect("child").main_thread, 2);
    }

    #[test]
    fn mffs_resolver_rejects_payload_checksum_failure() {
        let mut image = mffs_with_valid_app();
        image[96] ^= 1;
        let mut resolver = MffsProcessImageResolver::new(&image).expect("catalogue");
        assert_eq!(resolver.resolve(7, b"Demo.app\0"), Err(ExecError::Fault));
    }

    #[test]
    fn mffs_loader_populates_segment_plan_and_rolls_back_frames() {
        let image = mffs_with_valid_app();
        let resolver = MffsProcessImageResolver::new(&image).expect("catalogue");
        let mut allocator = FrameAllocator::new(0x8000_0000, 4).expect("frames");
        let mut plan = AddressSpacePlan::new();
        let mut writer = TestPageWriter {
            fail: false,
            zeroed: 0,
            written: 0,
            first_bytes: [0; 4],
        };

        let spec = resolver
            .load_into(b"Demo.app\0", &mut allocator, &mut plan, &mut writer)
            .expect("load");
        assert_eq!(spec.entry, Address(0x10000));
        assert_eq!(writer.zeroed, 1);
        assert_eq!(writer.written, 120);
        assert_eq!(writer.first_bytes, *b"\x7fELF");
        let table = plan.compile().expect("plan");
        let (_, permissions) = table.translate(Address(0x10000)).expect("entry mapping");
        assert!(permissions.read);
        assert!(permissions.execute);
        assert!(!permissions.write);

        let mut allocator = FrameAllocator::new(0x8000_0000, 4).expect("frames");
        let checkpoint = plan;
        let mut failed_plan = AddressSpacePlan::new();
        let mut writer = TestPageWriter {
            fail: true,
            zeroed: 0,
            written: 0,
            first_bytes: [0; 4],
        };
        assert_eq!(
            resolver.load_into(b"Demo.app\0", &mut allocator, &mut failed_plan, &mut writer),
            Err(ExecError::Fault)
        );
        assert_eq!(failed_plan, AddressSpacePlan::new());
        let recycled = allocator.allocate(1).expect("rollback");
        assert_eq!(recycled.base, 0x8000_0000);
        assert_eq!(
            checkpoint
                .compile()
                .expect("checkpoint")
                .translate(Address(0x10000)),
            Ok((Address(0x8000_0000), permissions))
        );
    }

    #[test]
    fn complete_process_image_path_publishes_private_stack_and_context() {
        use crate::mmu::SatpValue;

        let image = mffs_with_valid_app();
        let loader = MffsProcessImageResolver::new(&image).expect("catalogue");
        let mut runtime = ExecRuntime::qemu();
        let parent = runtime
            .create_process(7, Address(0x2000), ByteCount(4096), 1, 0, CpuAffinity::Any)
            .expect("parent");
        let memory = TestMemory {
            bytes: *b"Demo.app\0       ",
            readable: true,
            writable: false,
            short_copy: false,
        };
        let mut frame = SupervisorTrapFrame::empty();
        frame.sepc = 0x3000;
        frame.x[17] = SyscallNumber::CreateProcess as u64;
        frame.x[10] = USER_ADDRESS_MIN;
        frame.x[11] = 9;
        let mut allocator = FrameAllocator::new(0x8000_0000, 64).expect("frames");
        let satp = SatpValue::sv39(Address(0x9000)).expect("satp").raw();
        let mut writer = TestPageWriter {
            fail: false,
            zeroed: 0,
            written: 0,
            first_bytes: [0; 4],
        };

        let outcome = SyscallDispatcher.dispatch_runtime_with_process_image(
            &mut runtime,
            &mut frame,
            7,
            parent.main_thread,
            &memory,
            &loader,
            &mut allocator,
            AddressSpacePlan::new(),
            satp,
            &mut writer,
        );

        assert_eq!(
            outcome,
            SyscallOutcome::success(SyscallNumber::CreateProcess, 1)
        );
        let child = runtime.process_info(1).expect("child");
        assert_eq!(child.main_thread, 2);
        let context = runtime.contexts.get(child.main_thread).expect("context");
        assert_eq!(context.trap.sepc, 0x10000);
        assert_eq!(context.satp, satp);
        assert_eq!(runtime.process_satp(1), Ok(satp));
        let plan = runtime.address_spaces.plan(1).expect("plan");
        assert!(
            plan.compile()
                .expect("page table")
                .translate(Address(0x10000))
                .is_ok()
        );
        assert!(
            plan.compile()
                .expect("page table")
                .translate(Address(0x4000_0000))
                .is_ok()
        );
        assert_eq!(writer.written, 120);
        assert_eq!(writer.zeroed, 1 + 96 * 1024 / PAGE_SIZE as usize);
    }

    #[test]
    fn create_process_rejects_non_terminated_or_oversized_filename() {
        let memory = TestMemory {
            bytes: *b"Demo.appxxxxxxxx",
            readable: true,
            writable: false,
            short_copy: false,
        };
        assert_eq!(
            copy_process_name(&memory, USER_ADDRESS_MIN, 9),
            Err(ExecError::InvalidArgument)
        );
        assert_eq!(
            copy_process_name(&memory, USER_ADDRESS_MIN, (MAX_PROCESS_NAME + 1) as u64),
            Err(ExecError::InvalidArgument)
        );
    }

    #[test]
    fn runtime_thread_create_rejects_non_user_entry() {
        let mut runtime = ExecRuntime::qemu();
        let process = runtime
            .create_process(7, Address(0x1000), ByteCount(4096), 10, 0, CpuAffinity::Any)
            .expect("process");
        let mut frame = SupervisorTrapFrame::empty();
        frame.x[17] = SyscallNumber::ThreadCreate as u64;
        frame.x[10] = USER_ADDRESS_MAX + 1;
        frame.x[11] = 4096;

        let outcome =
            SyscallDispatcher.dispatch_runtime(&mut runtime, &mut frame, 7, process.main_thread);

        assert_eq!(outcome.error, Some(ExecError::InvalidArgument));
        assert_eq!(runtime.thread_count(7), 1);
    }

    #[test]
    fn runtime_thread_join_rejects_self_join() {
        let mut runtime = ExecRuntime::qemu();
        let process = runtime
            .create_process(7, Address(0x1000), ByteCount(4096), 10, 0, CpuAffinity::Any)
            .expect("process");
        let mut frame = SupervisorTrapFrame::empty();
        frame.x[17] = SyscallNumber::ThreadJoin as u64;
        frame.x[10] = process.main_thread;

        let outcome =
            SyscallDispatcher.dispatch_runtime(&mut runtime, &mut frame, 7, process.main_thread);

        assert_eq!(outcome.error, Some(ExecError::InvalidArgument));
    }

    #[test]
    fn reschedule_guard_requires_user_origin_and_current_thread() {
        assert!(may_reschedule(0, 7));
        assert!(!may_reschedule(SSTATUS_SPP, 7));
        assert!(!may_reschedule(0, 0));
    }
}
