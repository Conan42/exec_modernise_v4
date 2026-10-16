#![no_std]
#![forbid(unsafe_code)]

//! Validation portable des images ELF utilisées par ExecFS/MFFS.
//!
//! Le lecteur de volume, le stockage et le chargeur mémoire restent des
//! couches distinctes. Ce module ne conserve donc que les métadonnées validées
//! d'une image et ne déréférence jamais une adresse fournie par l'utilisateur.

pub const PAGE_SIZE: u64 = 4096;
pub const MAX_ELF_FILE_SIZE: usize = 4 * 1024 * 1024;
/// Taille maximale d'une image MFFS embarquée, catalogue et alignement inclus.
pub const MAX_MFFS_IMAGE_SIZE: usize = MAX_ELF_FILE_SIZE + 512;
pub const MAX_PROGRAM_HEADERS: usize = 32;
pub const ELF64_HEADER_SIZE: usize = 64;
pub const ELF64_PROGRAM_HEADER_SIZE: usize = 56;
pub const ET_EXEC: u16 = 2;
pub const EM_RISCV: u16 = 243;
pub const PT_LOAD: u32 = 1;
pub const PT_DYNAMIC: u32 = 2;
pub const PT_INTERP: u32 = 3;
pub const PT_TLS: u32 = 7;
pub const PT_NULL: u32 = 0;
pub const PT_GNU_STACK: u32 = 0x6474_e551;
pub const PT_RISCV_ATTRIBUTES: u32 = 0x7000_0003;
pub const PF_X: u32 = 1;
pub const PF_W: u32 = 2;
pub const PF_R: u32 = 4;
pub const APP_ENTRY_POINT: u64 = 0x10000;
pub const MFFS_SIGNATURE: &[u8; 4] = b"EXFS";
pub const MFFS_VERSION: u16 = 1;
pub const MFFS_ENTRY_SIZE: usize = 64;
pub const MFFS_MAX_ENTRIES: usize = 1024;
pub const FILE_TYPE_LIBRARY: u8 = 1;
pub const FILE_TYPE_DATA: u8 = 2;
pub const FILE_TYPE_APP: u8 = 3;
pub const FILE_TYPE_TEST: u8 = 4;
pub const FILE_TYPE_FONT: u8 = 5;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ElfError {
    TooLarge,
    Truncated,
    BadMagic,
    UnsupportedClass,
    UnsupportedEncoding,
    UnsupportedType,
    UnsupportedMachine,
    InvalidHeader,
    InvalidEntryPoint,
    TooManyProgramHeaders,
    InvalidProgramHeader,
    ForbiddenSegment,
    NoLoadSegment,
    WritableExecutable,
    SegmentOverlap,
    EntryNotExecutable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ElfSegment {
    pub file_offset: u64,
    pub virtual_address: u64,
    pub file_size: u64,
    pub memory_size: u64,
    pub flags: u32,
}

impl ElfSegment {
    pub const fn is_executable(self) -> bool {
        self.flags & PF_X != 0
    }

    pub const fn is_writable(self) -> bool {
        self.flags & PF_W != 0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ElfImage {
    pub entry: u64,
    pub file_size: usize,
    segments: [Option<ElfSegment>; MAX_PROGRAM_HEADERS],
    segment_count: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MffsError {
    Truncated,
    BadSignature,
    UnsupportedVersion,
    InvalidHeader,
    InvalidCatalog,
    InvalidName,
    InvalidPayload,
    ChecksumMismatch,
    NotFound,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MffsMetadata {
    pub kind: u8,
    pub flags: u8,
    pub mode: u16,
    pub owner: u32,
    pub group: u32,
    pub version: u16,
    pub offset: usize,
    pub size: usize,
    pub checksum: u32,
}

impl MffsMetadata {
    /// Vérifie les droits de lecture et d'exécution pour une identité V1.
    pub const fn can_read_execute(self, user: u32, group: u32) -> bool {
        let bits = if user == self.owner {
            (self.mode >> 6) & 7
        } else if group == self.group {
            (self.mode >> 3) & 7
        } else {
            self.mode & 7
        };
        bits & 5 == 5
    }
}

pub struct MffsFile<'a> {
    pub metadata: MffsMetadata,
    pub data: &'a [u8],
}

pub struct MffsImage<'a> {
    bytes: &'a [u8],
    catalog: usize,
    count: usize,
    image_size: usize,
}

impl<'a> MffsImage<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, MffsError> {
        if bytes.len() < 32 {
            return Err(MffsError::Truncated);
        }
        if &bytes[..4] != MFFS_SIGNATURE {
            return Err(MffsError::BadSignature);
        }
        if u16_le(bytes, 4)? != MFFS_VERSION {
            return Err(MffsError::UnsupportedVersion);
        }
        let block_size = usize::from(u16_le(bytes, 6)?);
        let count = usize::from(u16_le(bytes, 8)?);
        let entry_size = usize::from(u16_le(bytes, 10)?);
        let catalog = usize::try_from(u32_le(bytes, 12)?).map_err(|_| MffsError::InvalidHeader)?;
        let image_size =
            usize::try_from(u32_le(bytes, 20)?).map_err(|_| MffsError::InvalidHeader)?;
        if !block_size.is_power_of_two()
            || !(512..=4096).contains(&block_size)
            || count > MFFS_MAX_ENTRIES
            || entry_size != MFFS_ENTRY_SIZE
            || image_size > bytes.len()
            || image_size > MAX_MFFS_IMAGE_SIZE
        {
            return Err(MffsError::InvalidHeader);
        }
        let catalog_size = count
            .checked_mul(entry_size)
            .ok_or(MffsError::InvalidCatalog)?;
        let catalog_end = catalog
            .checked_add(catalog_size)
            .ok_or(MffsError::InvalidCatalog)?;
        if catalog < 32 || catalog_end > image_size || catalog_end > bytes.len() {
            return Err(MffsError::InvalidCatalog);
        }
        let image = Self {
            bytes,
            catalog,
            count,
            image_size,
        };
        for index in 0..count {
            image.metadata(index)?;
        }
        Ok(image)
    }

    pub const fn count(&self) -> usize {
        self.count
    }

    pub fn metadata(&self, index: usize) -> Result<MffsMetadata, MffsError> {
        if index >= self.count {
            return Err(MffsError::NotFound);
        }
        let entry = self
            .catalog
            .checked_add(
                index
                    .checked_mul(MFFS_ENTRY_SIZE)
                    .ok_or(MffsError::InvalidCatalog)?,
            )
            .ok_or(MffsError::InvalidCatalog)?;
        let name = self
            .bytes
            .get(entry..entry + 32)
            .ok_or(MffsError::Truncated)?;
        if name.iter().position(|byte| *byte == 0).is_none() {
            return Err(MffsError::InvalidName);
        }
        let offset = usize::try_from(u32_le(self.bytes, entry + 44)?)
            .map_err(|_| MffsError::InvalidPayload)?;
        let size = usize::try_from(u32_le(self.bytes, entry + 48)?)
            .map_err(|_| MffsError::InvalidPayload)?;
        let catalog_end = self
            .catalog
            .checked_add(self.count * MFFS_ENTRY_SIZE)
            .ok_or(MffsError::InvalidCatalog)?;
        let end = offset.checked_add(size).ok_or(MffsError::InvalidPayload)?;
        if offset < catalog_end || end > self.image_size || end > self.bytes.len() {
            return Err(MffsError::InvalidPayload);
        }
        Ok(MffsMetadata {
            kind: self.bytes[entry + 32],
            flags: self.bytes[entry + 33],
            mode: u16_le(self.bytes, entry + 34)?,
            owner: u32_le(self.bytes, entry + 36)?,
            group: u32_le(self.bytes, entry + 40)?,
            version: u16_le(self.bytes, entry + 52)?,
            offset,
            size,
            checksum: u32_le(self.bytes, entry + 56)?,
        })
    }

    pub fn name(&self, index: usize, output: &mut [u8]) -> Result<usize, MffsError> {
        if index >= self.count {
            return Err(MffsError::NotFound);
        }
        let entry = self
            .catalog
            .checked_add(
                index
                    .checked_mul(MFFS_ENTRY_SIZE)
                    .ok_or(MffsError::InvalidCatalog)?,
            )
            .ok_or(MffsError::InvalidCatalog)?;
        let stored = self
            .bytes
            .get(entry..entry + 32)
            .ok_or(MffsError::Truncated)?;
        let length = stored
            .iter()
            .position(|byte| *byte == 0)
            .ok_or(MffsError::InvalidName)?
            + 1;
        if output.len() < length {
            return Err(MffsError::InvalidName);
        }
        output[..length].copy_from_slice(&stored[..length]);
        Ok(length)
    }

    pub fn lookup(&self, name: &[u8]) -> Result<MffsFile<'a>, MffsError> {
        let mut stored = [0u8; 32];
        for index in 0..self.count {
            let length = self.name(index, &mut stored)?;
            if stored[..length] != *name {
                continue;
            }
            let metadata = self.metadata(index)?;
            let data = &self.bytes[metadata.offset..metadata.offset + metadata.size];
            if fnv1a(data) != metadata.checksum {
                return Err(MffsError::ChecksumMismatch);
            }
            return Ok(MffsFile { metadata, data });
        }
        Err(MffsError::NotFound)
    }
}

fn u16_le(data: &[u8], offset: usize) -> Result<u16, MffsError> {
    let end = offset.checked_add(2).ok_or(MffsError::Truncated)?;
    Ok(u16::from_le_bytes(
        data.get(offset..end)
            .ok_or(MffsError::Truncated)?
            .try_into()
            .unwrap(),
    ))
}

fn u32_le(data: &[u8], offset: usize) -> Result<u32, MffsError> {
    let end = offset.checked_add(4).ok_or(MffsError::Truncated)?;
    Ok(u32::from_le_bytes(
        data.get(offset..end)
            .ok_or(MffsError::Truncated)?
            .try_into()
            .unwrap(),
    ))
}

pub const fn fnv1a(bytes: &[u8]) -> u32 {
    let mut hash = 0x811c_9dc5u32;
    let mut index = 0;
    while index < bytes.len() {
        hash = (hash ^ bytes[index] as u32).wrapping_mul(0x0100_0193);
        index += 1;
    }
    hash
}

impl ElfImage {
    pub const fn segment_count(self) -> usize {
        self.segment_count
    }

    pub fn segment(&self, index: usize) -> Option<ElfSegment> {
        if index < self.segment_count {
            self.segments[index]
        } else {
            None
        }
    }
}

fn bytes(data: &[u8], offset: usize, size: usize) -> Result<&[u8], ElfError> {
    let end = offset.checked_add(size).ok_or(ElfError::Truncated)?;
    data.get(offset..end).ok_or(ElfError::Truncated)
}

fn u16_at(data: &[u8], offset: usize) -> Result<u16, ElfError> {
    Ok(u16::from_le_bytes(
        bytes(data, offset, 2)?.try_into().unwrap(),
    ))
}

fn u32_at(data: &[u8], offset: usize) -> Result<u32, ElfError> {
    Ok(u32::from_le_bytes(
        bytes(data, offset, 4)?.try_into().unwrap(),
    ))
}

fn u64_at(data: &[u8], offset: usize) -> Result<u64, ElfError> {
    Ok(u64::from_le_bytes(
        bytes(data, offset, 8)?.try_into().unwrap(),
    ))
}

fn range_end(start: u64, length: u64) -> Result<u64, ElfError> {
    start
        .checked_add(length)
        .ok_or(ElfError::InvalidProgramHeader)
}

/// Valide une image ELF64 `ET_EXEC` statique pour RISC-V.
///
/// Les règles reprennent le validateur V1 : little-endian, machine RISC-V,
/// segments `PT_LOAD` bornés, W^X, absence de `PT_INTERP`/`PT_DYNAMIC`/TLS et
/// point d'entrée dans un segment exécutable. L'image n'est pas chargée.
pub fn parse_elf64(data: &[u8]) -> Result<ElfImage, ElfError> {
    if data.len() > MAX_ELF_FILE_SIZE {
        return Err(ElfError::TooLarge);
    }
    if bytes(data, 0, ELF64_HEADER_SIZE)?[..4] != *b"\x7fELF" {
        return Err(ElfError::BadMagic);
    }
    if data[4] != 2 {
        return Err(ElfError::UnsupportedClass);
    }
    if data[5] != 1 || data[6] != 1 {
        return Err(ElfError::UnsupportedEncoding);
    }
    if u16_at(data, 16)? != ET_EXEC {
        return Err(ElfError::UnsupportedType);
    }
    if u16_at(data, 18)? != EM_RISCV {
        return Err(ElfError::UnsupportedMachine);
    }
    if u32_at(data, 20)? != 1 || u16_at(data, 52)? != ELF64_HEADER_SIZE as u16 {
        return Err(ElfError::InvalidHeader);
    }

    let entry = u64_at(data, 24)?;
    if entry != APP_ENTRY_POINT {
        return Err(ElfError::InvalidEntryPoint);
    }
    let program_offset = usize::try_from(u64_at(data, 32)?).map_err(|_| ElfError::InvalidHeader)?;
    let program_size = usize::from(u16_at(data, 54)?);
    let program_count = usize::from(u16_at(data, 56)?);
    if program_size != ELF64_PROGRAM_HEADER_SIZE {
        return Err(ElfError::InvalidHeader);
    }
    if program_count > MAX_PROGRAM_HEADERS {
        return Err(ElfError::TooManyProgramHeaders);
    }
    let table_size = program_size
        .checked_mul(program_count)
        .ok_or(ElfError::Truncated)?;
    bytes(data, program_offset, table_size)?;

    let mut segments: [Option<ElfSegment>; MAX_PROGRAM_HEADERS] = [None; MAX_PROGRAM_HEADERS];
    let mut segment_count = 0usize;
    let mut entry_is_executable = false;
    for index in 0..program_count {
        let offset = program_offset
            .checked_add(index.checked_mul(program_size).ok_or(ElfError::Truncated)?)
            .ok_or(ElfError::Truncated)?;
        let kind = u32_at(data, offset)?;
        if matches!(kind, PT_DYNAMIC | PT_INTERP | PT_TLS) {
            return Err(ElfError::ForbiddenSegment);
        }
        if matches!(kind, PT_NULL | PT_GNU_STACK | PT_RISCV_ATTRIBUTES) {
            continue;
        }
        if kind != PT_LOAD {
            continue;
        }
        let flags = u32_at(data, offset + 4)?;
        let file_offset = u64_at(data, offset + 8)?;
        let virtual_address = u64_at(data, offset + 16)?;
        let file_size = u64_at(data, offset + 32)?;
        let memory_size = u64_at(data, offset + 40)?;
        let alignment = u64_at(data, offset + 48)?;
        if segment_count == MAX_PROGRAM_HEADERS {
            return Err(ElfError::TooManyProgramHeaders);
        }
        if memory_size == 0
            || memory_size < file_size
            || alignment != PAGE_SIZE
            || file_offset % PAGE_SIZE != virtual_address % PAGE_SIZE
            || virtual_address % PAGE_SIZE != 0
            || flags & (PF_R | PF_W | PF_X) == 0
            || flags & PF_W != 0 && flags & PF_X != 0
        {
            return Err(if flags & PF_W != 0 && flags & PF_X != 0 {
                ElfError::WritableExecutable
            } else {
                ElfError::InvalidProgramHeader
            });
        }
        let file_end = range_end(file_offset, file_size)?;
        if file_end > data.len() as u64 {
            return Err(ElfError::Truncated);
        }
        let memory_end = range_end(virtual_address, memory_size)?;
        // La moitié utilisateur Sv39 est la seule plage chargeable par une
        // APP ; les adresses basses nulles restent interdites.
        if virtual_address < 0x1000 || memory_end > (1u64 << 38) {
            return Err(ElfError::InvalidProgramHeader);
        }
        for previous in segments[..segment_count].iter().flatten() {
            let previous_end = range_end(previous.virtual_address, previous.memory_size)?;
            if virtual_address < previous_end && previous.virtual_address < memory_end {
                return Err(ElfError::SegmentOverlap);
            }
        }
        let segment = ElfSegment {
            file_offset,
            virtual_address,
            file_size,
            memory_size,
            flags,
        };
        entry_is_executable |=
            entry >= virtual_address && entry < memory_end && segment.is_executable();
        segments[segment_count] = Some(segment);
        segment_count += 1;
    }

    if segment_count == 0 {
        return Err(ElfError::NoLoadSegment);
    }
    if !entry_is_executable {
        return Err(ElfError::EntryNotExecutable);
    }
    Ok(ElfImage {
        entry,
        file_size: data.len(),
        segments,
        segment_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put16(data: &mut [u8], offset: usize, value: u16) {
        data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn put32(data: &mut [u8], offset: usize, value: u32) {
        data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn put64(data: &mut [u8], offset: usize, value: u64) {
        data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }

    fn mffs_image() -> [u8; 100] {
        let mut data = [0; 100];
        data[..4].copy_from_slice(MFFS_SIGNATURE);
        put16(&mut data, 4, MFFS_VERSION);
        put16(&mut data, 6, 512);
        put16(&mut data, 8, 1);
        put16(&mut data, 10, MFFS_ENTRY_SIZE as u16);
        put32(&mut data, 12, 32);
        put32(&mut data, 20, 100);
        data[32..41].copy_from_slice(b"Demo.app\0");
        data[64] = FILE_TYPE_APP;
        put16(&mut data, 66, 0o555);
        put32(&mut data, 76, 96);
        put32(&mut data, 80, 4);
        put16(&mut data, 84, 1);
        data[96..100].copy_from_slice(b"ELF!");
        let checksum = fnv1a(&data[96..100]);
        put32(&mut data, 88, checksum);
        data
    }

    fn valid_image() -> [u8; 120] {
        let mut data = [0; 120];
        data[..4].copy_from_slice(b"\x7fELF");
        data[4] = 2;
        data[5] = 1;
        data[6] = 1;
        put16(&mut data, 16, ET_EXEC);
        put16(&mut data, 18, EM_RISCV);
        put32(&mut data, 20, 1);
        put64(&mut data, 24, 0x10000);
        put64(&mut data, 32, 64);
        put16(&mut data, 52, 64);
        put16(&mut data, 54, 56);
        put16(&mut data, 56, 1);
        put32(&mut data, 64, PT_LOAD);
        put32(&mut data, 68, PF_R | PF_X);
        put64(&mut data, 72, 0);
        put64(&mut data, 80, 0x10000);
        put64(&mut data, 96, 120);
        put64(&mut data, 104, PAGE_SIZE);
        put64(&mut data, 112, PAGE_SIZE);
        data
    }

    #[test]
    fn validates_static_riscv_load_image() {
        let image = parse_elf64(&valid_image()).expect("ELF");
        assert_eq!(image.entry, 0x10000);
        assert_eq!(image.segment_count(), 1);
        assert!(image.segment(0).expect("segment").is_executable());
    }

    #[test]
    fn rejects_writable_executable_segments_and_forbidden_types() {
        let mut data = valid_image();
        put32(&mut data, 68, PF_R | PF_W | PF_X);
        assert_eq!(parse_elf64(&data), Err(ElfError::WritableExecutable));

        let mut data = valid_image();
        put32(&mut data, 64, PT_DYNAMIC);
        assert_eq!(parse_elf64(&data), Err(ElfError::ForbiddenSegment));
    }

    #[test]
    fn rejects_truncation_overlap_and_bad_entry() {
        let mut data = valid_image();
        put64(&mut data, 96, 121);
        assert_eq!(parse_elf64(&data), Err(ElfError::Truncated));

        let mut data = valid_image();
        put64(&mut data, 24, 0x20000);
        assert_eq!(parse_elf64(&data), Err(ElfError::InvalidEntryPoint));

        let mut data = [0; 176];
        data[..120].copy_from_slice(&valid_image());
        put16(&mut data, 56, 2);
        put32(&mut data, 120, PT_LOAD);
        put32(&mut data, 124, PF_R);
        put64(&mut data, 128, 0);
        put64(&mut data, 136, 0x10000);
        put64(&mut data, 152, 1);
        put64(&mut data, 160, PAGE_SIZE);
        put64(&mut data, 168, PAGE_SIZE);
        assert_eq!(parse_elf64(&data), Err(ElfError::SegmentOverlap));
    }

    #[test]
    fn rejects_non_elf_and_too_many_headers() {
        assert_eq!(parse_elf64(b"not an elf"), Err(ElfError::Truncated));
        let mut data = valid_image();
        put16(&mut data, 56, (MAX_PROGRAM_HEADERS + 1) as u16);
        assert_eq!(parse_elf64(&data), Err(ElfError::TooManyProgramHeaders));
    }

    #[test]
    fn validates_mffs_catalog_and_checksum_before_returning_payload() {
        let bytes = mffs_image();
        let image = MffsImage::parse(&bytes).expect("MFFS");
        assert_eq!(image.count(), 1);
        assert!(image.metadata(0).expect("metadata").can_read_execute(0, 0));
        let file = image.lookup(b"Demo.app\0").expect("entry");
        assert_eq!(file.metadata.kind, FILE_TYPE_APP);
        assert_eq!(file.data, b"ELF!");
        assert!(matches!(
            image.lookup(b"Missing\0"),
            Err(MffsError::NotFound)
        ));
    }

    #[test]
    fn rejects_mffs_bad_checksum_and_invalid_payload_bounds() {
        let mut data = mffs_image();
        data[96] ^= 1;
        let image = MffsImage::parse(&data).expect("catalog");
        assert!(matches!(
            image.lookup(b"Demo.app\0"),
            Err(MffsError::ChecksumMismatch)
        ));

        let mut data = mffs_image();
        put32(&mut data, 80, 8);
        assert!(matches!(
            MffsImage::parse(&data),
            Err(MffsError::InvalidPayload)
        ));
    }

    #[test]
    fn mffs_permissions_require_read_and_execute_for_selected_identity() {
        let mut data = mffs_image();
        put16(&mut data, 66, 0o400);
        let image = MffsImage::parse(&data).expect("catalog");
        assert!(!image.metadata(0).expect("metadata").can_read_execute(0, 0));

        put16(&mut data, 66, 0o100);
        let image = MffsImage::parse(&data).expect("catalog");
        assert!(!image.metadata(0).expect("metadata").can_read_execute(0, 0));
    }
}
