//! Contrats d’adresses virtuelles et de pages pour Sv39.

use crate::{Address, ExecError, ExecResult};

pub const PAGE_SIZE: u64 = 4096;
pub const SV39_USER_MIN: u64 = 0x1000;
pub const SV39_USER_MAX: u64 = (1 << 38) - 1;
pub const SV39_SIGN_EXTENSION: u64 = !((1 << 39) - 1);
pub const MAX_PAGE_MAPPINGS: usize = 256;
pub const MAX_ADDRESS_SPACE_PLANS: usize = 32;
pub const MAX_PROCESS_ADDRESS_SPACES: usize = 16;
pub const MAX_SHARED_FRAMES: usize = 256;
/// Capacité d’un pool physique borné : 256 Mio en pages de 4 Kio.
///
/// Le profil QEMU/Venus réserve 256 Mio ; conserver une limite à 128 Mio
/// faisait échouer `FrameAllocator::from_memory_map` avant même le lancement
/// d'UILab, donc avant toute possibilité de recevoir un événement.
pub const MAX_PHYSICAL_FRAMES: usize = 64 * 1024;
pub const MAX_MEMORY_REGIONS: usize = 16;
pub const SV39_MODE: u64 = 8;
pub const PTE_VALID: u64 = 1 << 0;
pub const PTE_READ: u64 = 1 << 1;
pub const PTE_WRITE: u64 = 1 << 2;
pub const PTE_EXECUTE: u64 = 1 << 3;
pub const PTE_USER: u64 = 1 << 4;
const SV39_TABLE_ENTRIES: usize = 512;
const SV39_MAX_CHILD_TABLES: usize = 96;

pub const fn is_sv39_canonical(address: u64) -> bool {
    let upper = address & SV39_SIGN_EXTENSION;
    upper == 0 || upper == SV39_SIGN_EXTENSION
}

pub const fn validate_user_address(address: Address) -> ExecResult<Address> {
    if address.0 < SV39_USER_MIN || address.0 > SV39_USER_MAX || !is_sv39_canonical(address.0) {
        Err(ExecError::InvalidArgument)
    } else {
        Ok(address)
    }
}

pub const fn validate_sv39_address(address: Address) -> ExecResult<Address> {
    if !is_sv39_canonical(address.0) {
        Err(ExecError::InvalidArgument)
    } else {
        Ok(address)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UserPageRange {
    pub first_page: u64,
    pub page_count: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PagePermissions {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
    pub user: bool,
}

impl PagePermissions {
    pub const fn pte_flags(self) -> u64 {
        PTE_VALID
            | if self.read { PTE_READ } else { 0 }
            | if self.write { PTE_WRITE } else { 0 }
            | if self.execute { PTE_EXECUTE } else { 0 }
            | if self.user { PTE_USER } else { 0 }
    }
}

impl PagePermissions {
    pub const USER_STACK: Self = Self {
        read: true,
        write: true,
        execute: false,
        user: true,
    };

    pub const COW_USER: Self = Self {
        read: true,
        write: false,
        execute: false,
        user: true,
    };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MappingPlan {
    pub virtual_range: UserPageRange,
    pub physical_base: u64,
    pub permissions: PagePermissions,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PageMapping {
    pub virtual_page: u64,
    pub physical_page: u64,
    pub permissions: PagePermissions,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhysicalFrameRange {
    pub base: u64,
    pub count: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CowPage {
    pub virtual_address: Address,
    pub physical_address: Address,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CowWriteResolution {
    pub virtual_address: Address,
    pub old_physical_address: Address,
    pub new_physical_address: Address,
    pub copied: bool,
}

/// Frames dont le dernier détenteur COW a été retiré lors d’un reaping.
/// L’allocateur physique du backend peut rendre ces frames disponibles.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReleasedCowFrames {
    frames: [Option<Address>; MAX_SHARED_FRAMES],
    count: usize,
}

impl ReleasedCowFrames {
    pub const fn new() -> Self {
        Self {
            frames: [None; MAX_SHARED_FRAMES],
            count: 0,
        }
    }

    pub const fn len(&self) -> usize {
        self.count
    }

    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub const fn get(&self, index: usize) -> Option<Address> {
        if index >= self.count {
            None
        } else {
            self.frames[index]
        }
    }

    fn push_unique(&mut self, frame: Address) -> ExecResult<()> {
        if self.frames[..self.count]
            .iter()
            .any(|entry| *entry == Some(frame))
        {
            return Ok(());
        }
        if self.count == self.frames.len() {
            return Err(ExecError::OutOfMemory);
        }
        self.frames[self.count] = Some(frame);
        self.count += 1;
        Ok(())
    }
}

/// Backend chargé de recopier une page physique lors de la résolution d’un
/// fault d’écriture COW.
///
/// Exec ne connaît ni le mapping direct de la mémoire physique ni les
/// primitives de copie propres à la plateforme. Le backend doit donc
/// implémenter cette opération et retourner une erreur avant publication si
/// la copie ne peut pas être menée à bien.
pub trait PageCopier {
    fn copy_page(&mut self, source: Address, destination: Address) -> ExecResult<()>;
}

impl CowPage {
    pub fn new(virtual_address: Address, physical_address: Address) -> ExecResult<Self> {
        validate_user_address(virtual_address)?;
        if virtual_address.0 % PAGE_SIZE != 0
            || physical_address.0 == 0
            || physical_address.0 % PAGE_SIZE != 0
        {
            return Err(ExecError::InvalidArgument);
        }
        Ok(Self {
            virtual_address,
            physical_address,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryRegionKind {
    Usable,
    Reserved,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemoryRegion {
    pub base: u64,
    pub length: u64,
    pub kind: MemoryRegionKind,
}

impl MemoryRegion {
    pub const fn new(base: u64, length: u64, kind: MemoryRegionKind) -> ExecResult<Self> {
        if base % PAGE_SIZE != 0
            || length == 0
            || length % PAGE_SIZE != 0
            || base.checked_add(length).is_none()
        {
            return Err(ExecError::InvalidArgument);
        }
        Ok(Self { base, length, kind })
    }

    pub const fn end(&self) -> u64 {
        self.base + self.length
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemoryMap {
    regions: [Option<MemoryRegion>; MAX_MEMORY_REGIONS],
}

impl Default for MemoryMap {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryMap {
    pub const fn new() -> Self {
        Self {
            regions: [None; MAX_MEMORY_REGIONS],
        }
    }

    pub fn from_usable_ranges(ranges: &[(u64, u64)]) -> ExecResult<Self> {
        let mut map = Self::new();
        for &(base, length) in ranges {
            map.add(MemoryRegion::new(base, length, MemoryRegionKind::Usable)?)?;
        }
        Ok(map)
    }

    pub fn from_usable_ranges_excluding(
        ranges: &[(u64, u64)],
        exclusions: &[(u64, u64)],
    ) -> ExecResult<Self> {
        let mut map = Self::new();
        for &(base, length) in ranges {
            let end = base.checked_add(length).ok_or(ExecError::InvalidArgument)?;
            let mut cursor = base;
            for &(excluded_base, excluded_length) in exclusions {
                let excluded_end = excluded_base
                    .checked_add(excluded_length)
                    .ok_or(ExecError::InvalidArgument)?;
                let overlap_start = cursor.max(excluded_base);
                let overlap_end = end.min(excluded_end);
                if overlap_start >= overlap_end {
                    continue;
                }
                if cursor < overlap_start {
                    map.add(MemoryRegion::new(
                        cursor,
                        overlap_start - cursor,
                        MemoryRegionKind::Usable,
                    )?)?;
                }
                cursor = overlap_end;
            }
            if cursor < end {
                map.add(MemoryRegion::new(
                    cursor,
                    end - cursor,
                    MemoryRegionKind::Usable,
                )?)?;
            }
        }
        if map.usable_frames() == 0 {
            return Err(ExecError::OutOfMemory);
        }
        Ok(map)
    }

    pub fn add(&mut self, region: MemoryRegion) -> ExecResult<()> {
        if self.regions.iter().any(|entry| {
            entry.is_some_and(|existing| {
                region.base < existing.end() && existing.base < region.end()
            })
        }) {
            return Err(ExecError::InvalidArgument);
        }
        let slot = self
            .regions
            .iter()
            .position(Option::is_none)
            .ok_or(ExecError::OutOfMemory)?;
        self.regions[slot] = Some(region);
        Ok(())
    }

    pub fn usable_frames(&self) -> usize {
        self.regions
            .iter()
            .filter_map(|entry| entry.filter(|region| region.kind == MemoryRegionKind::Usable))
            .map(|region| (region.length / PAGE_SIZE) as usize)
            .sum()
    }

    pub fn usable_ranges(&self) -> ([(u64, u64); MAX_MEMORY_REGIONS], usize) {
        let mut ranges = [(0, 0); MAX_MEMORY_REGIONS];
        let mut count = 0;
        for region in self.regions.iter().flatten() {
            if region.kind == MemoryRegionKind::Usable {
                ranges[count] = (region.base, region.length);
                count += 1;
            }
        }
        (ranges, count)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameAllocator {
    ranges: [Option<(u64, usize)>; MAX_MEMORY_REGIONS],
    range_count: usize,
    used: [[u64; MAX_PHYSICAL_FRAMES / 64]; MAX_MEMORY_REGIONS],
}

impl FrameAllocator {
    pub const fn new(base: u64, frame_count: usize) -> ExecResult<Self> {
        let bytes = (frame_count as u64).saturating_mul(PAGE_SIZE);
        if base % PAGE_SIZE != 0
            || frame_count == 0
            || frame_count > MAX_PHYSICAL_FRAMES
            || base.checked_add(bytes).is_none()
        {
            return Err(ExecError::InvalidArgument);
        }
        let mut allocator = Self {
            ranges: [const { None }; MAX_MEMORY_REGIONS],
            range_count: 1,
            used: [[0; MAX_PHYSICAL_FRAMES / 64]; MAX_MEMORY_REGIONS],
        };
        allocator.ranges[0] = Some((base, frame_count));
        Ok(allocator)
    }

    pub fn from_memory_map(map: &MemoryMap) -> ExecResult<Self> {
        let (ranges, count) = map.usable_ranges();
        Self::from_ranges(&ranges[..count])
    }

    pub fn from_ranges(ranges: &[(u64, u64)]) -> ExecResult<Self> {
        if ranges.is_empty() || ranges.len() > MAX_MEMORY_REGIONS {
            return Err(ExecError::InvalidArgument);
        }
        let mut allocator = Self {
            ranges: [const { None }; MAX_MEMORY_REGIONS],
            range_count: 0,
            used: [[0; MAX_PHYSICAL_FRAMES / 64]; MAX_MEMORY_REGIONS],
        };
        for &(base, length) in ranges {
            if base % PAGE_SIZE != 0 || length == 0 || length % PAGE_SIZE != 0 {
                return Err(ExecError::InvalidArgument);
            }
            let count = (length / PAGE_SIZE) as usize;
            if count > MAX_PHYSICAL_FRAMES {
                return Err(ExecError::InvalidArgument);
            }
            allocator.ranges[allocator.range_count] = Some((base, count));
            allocator.range_count += 1;
        }
        Ok(allocator)
    }

    pub fn allocate(&mut self, count: usize) -> ExecResult<PhysicalFrameRange> {
        if count == 0 || count > MAX_PHYSICAL_FRAMES {
            return Err(ExecError::InvalidArgument);
        }
        for range_index in 0..self.range_count {
            let Some((base, frame_count)) = self.ranges[range_index] else {
                continue;
            };
            if count > frame_count {
                continue;
            }
            for first in 0..=frame_count - count {
                if (first..first + count).all(|index| !self.is_used(range_index, index)) {
                    for index in first..first + count {
                        self.set_used(range_index, index, true);
                    }
                    return Ok(PhysicalFrameRange {
                        base: base + first as u64 * PAGE_SIZE,
                        count,
                    });
                }
            }
        }
        Err(ExecError::OutOfMemory)
    }

    pub fn free(&mut self, range: PhysicalFrameRange) -> ExecResult<()> {
        if range.count == 0 || range.base % PAGE_SIZE != 0 {
            return Err(ExecError::InvalidArgument);
        }
        for range_index in 0..self.range_count {
            let Some((base, frame_count)) = self.ranges[range_index] else {
                continue;
            };
            if range.base < base || range.base >= base + frame_count as u64 * PAGE_SIZE {
                continue;
            }
            let first = ((range.base - base) / PAGE_SIZE) as usize;
            if first + range.count > frame_count
                || !(first..first + range.count).all(|index| self.is_used(range_index, index))
            {
                return Err(ExecError::InvalidArgument);
            }
            for index in first..first + range.count {
                self.set_used(range_index, index, false);
            }
            return Ok(());
        }
        Err(ExecError::InvalidArgument)
    }

    /// Rend une frame COW au pool si ce pool en est propriétaire.
    ///
    /// `Ok(false)` signifie que la frame appartient à un autre domaine
    /// physique ; le backend concerné doit alors la traiter lui-même.
    pub fn free_if_owned(&mut self, frame: Address) -> ExecResult<bool> {
        if frame.0 == 0 || frame.0 % PAGE_SIZE != 0 {
            return Err(ExecError::InvalidArgument);
        }
        if !self.owns(frame) {
            return Ok(false);
        }
        self.free(PhysicalFrameRange {
            base: frame.0,
            count: 1,
        })?;
        Ok(true)
    }

    /// Rend au pool toutes les frames signalées par un reaping COW.
    /// L’opération vérifie d’abord que chaque frame appartenant au pool est
    /// encore allouée, afin d’éviter une restitution partielle.
    pub fn reclaim_cow_frames(&mut self, frames: &ReleasedCowFrames) -> ExecResult<usize> {
        for index in 0..frames.len() {
            let frame = frames.get(index).ok_or(ExecError::Fault)?;
            if self.owns(frame) && !self.is_allocated(frame)? {
                return Err(ExecError::InvalidArgument);
            }
        }
        let mut reclaimed = 0;
        for index in 0..frames.len() {
            let frame = frames.get(index).ok_or(ExecError::Fault)?;
            if self.free_if_owned(frame)? {
                reclaimed += 1;
            }
        }
        Ok(reclaimed)
    }

    fn owns(&self, frame: Address) -> bool {
        self.ranges
            .iter()
            .take(self.range_count)
            .flatten()
            .any(|(base, count)| frame.0 >= *base && frame.0 < *base + *count as u64 * PAGE_SIZE)
    }

    fn is_allocated(&self, frame: Address) -> ExecResult<bool> {
        for range_index in 0..self.range_count {
            let Some((base, frame_count)) = self.ranges[range_index] else {
                continue;
            };
            if frame.0 < base || frame.0 >= base + frame_count as u64 * PAGE_SIZE {
                continue;
            }
            let index = ((frame.0 - base) / PAGE_SIZE) as usize;
            return Ok(self.is_used(range_index, index));
        }
        Err(ExecError::InvalidArgument)
    }

    fn is_used(&self, range: usize, index: usize) -> bool {
        self.used[range][index / 64] & (1u64 << (index % 64)) != 0
    }

    fn set_used(&mut self, range: usize, index: usize, used: bool) {
        let bit = 1u64 << (index % 64);
        if used {
            self.used[range][index / 64] |= bit;
        } else {
            self.used[range][index / 64] &= !bit;
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PageTablePlan {
    entries: [Option<PageMapping>; MAX_PAGE_MAPPINGS],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AddressSpacePlan {
    mappings: [Option<MappingPlan>; MAX_ADDRESS_SPACE_PLANS],
    count: usize,
}

impl AddressSpacePlan {
    pub const fn new() -> Self {
        Self {
            mappings: [const { None }; MAX_ADDRESS_SPACE_PLANS],
            count: 0,
        }
    }

    pub fn add(&mut self, mapping: MappingPlan) -> ExecResult<()> {
        if self.count == MAX_ADDRESS_SPACE_PLANS {
            return Err(ExecError::OutOfMemory);
        }
        let first = mapping.virtual_range.first_page;
        let last = first
            .checked_add(mapping.virtual_range.page_count)
            .ok_or(ExecError::InvalidArgument)?;
        if self.mappings.iter().flatten().any(|existing| {
            let existing_first = existing.virtual_range.first_page;
            let existing_last = existing_first + existing.virtual_range.page_count;
            first < existing_last && existing_first < last
        }) {
            return Err(ExecError::InvalidArgument);
        }
        self.mappings[self.count] = Some(mapping);
        self.count += 1;
        Ok(())
    }

    /// Ajoute un mapping utilisateur d’une seule page vers une frame privée.
    /// L’allocation de la frame appartient au propriétaire de l’espace.
    pub fn add_private_page(
        &mut self,
        virtual_address: Address,
        physical_address: Address,
        permissions: PagePermissions,
    ) -> ExecResult<()> {
        validate_user_address(virtual_address)?;
        let mapping = MappingPlan::new(
            UserPageRange {
                first_page: virtual_address.0 / PAGE_SIZE,
                page_count: 1,
            },
            physical_address.0,
            permissions,
        )?;
        self.add(mapping)
    }

    pub fn add_cow_page(&mut self, page: CowPage) -> ExecResult<()> {
        self.add_private_page(
            page.virtual_address,
            page.physical_address,
            PagePermissions::COW_USER,
        )
    }

    pub fn replace_private_page(
        &mut self,
        virtual_address: Address,
        physical_address: Address,
        permissions: PagePermissions,
    ) -> ExecResult<()> {
        validate_user_address(virtual_address)?;
        if virtual_address.0 % PAGE_SIZE != 0 {
            return Err(ExecError::InvalidArgument);
        }
        if physical_address.0 == 0 || physical_address.0 % PAGE_SIZE != 0 {
            return Err(ExecError::InvalidArgument);
        }
        let page = virtual_address.0 / PAGE_SIZE;
        let mapping = self
            .mappings
            .iter_mut()
            .flatten()
            .find(|mapping| {
                mapping.virtual_range.first_page == page && mapping.virtual_range.page_count == 1
            })
            .ok_or(ExecError::NotFound)?;
        mapping.physical_base = physical_address.0;
        mapping.permissions = permissions;
        Ok(())
    }

    pub fn resolve_cow_write(
        &mut self,
        references: &mut SharedFrameReferences,
        allocator: &mut FrameAllocator,
        virtual_address: Address,
    ) -> ExecResult<CowWriteResolution> {
        validate_user_address(virtual_address)?;
        // `stval` contient l’adresse fautive, pas nécessairement le début de
        // la page. Le mapping COW reste page-granulaire.
        let page = virtual_address.0 / PAGE_SIZE;
        let page_address = Address(page * PAGE_SIZE);
        let old_physical = self
            .mappings
            .iter()
            .flatten()
            .find(|mapping| {
                mapping.virtual_range.first_page == page
                    && mapping.virtual_range.page_count == 1
                    && mapping.permissions == PagePermissions::COW_USER
            })
            .map(|mapping| mapping.physical_base)
            .ok_or(ExecError::NotFound)?;
        let replacement = references.prepare_write(allocator, Address(old_physical))?;
        let copied = replacement.base != old_physical;
        if let Err(error) = self.replace_private_page(
            page_address,
            Address(replacement.base),
            PagePermissions::USER_STACK,
        ) {
            if copied {
                let _ = references.release(Address(replacement.base));
                let _ = allocator.free(replacement);
            }
            return Err(error);
        }
        Ok(CowWriteResolution {
            virtual_address: page_address,
            old_physical_address: Address(old_physical),
            new_physical_address: Address(replacement.base),
            copied,
        })
    }

    /// Résout une écriture COW et demande au backend de recopier les octets.
    ///
    /// La résolution est transactionnelle : si la copie échoue, le mapping,
    /// les références de frames et l’état de l’allocateur sont restaurés. Une
    /// page qui n’a plus qu’un détenteur est simplement rendue inscriptible et
    /// ne déclenche aucune copie.
    pub fn resolve_cow_write_with_copy<C: PageCopier>(
        &mut self,
        references: &mut SharedFrameReferences,
        allocator: &mut FrameAllocator,
        virtual_address: Address,
        copier: &mut C,
    ) -> ExecResult<CowWriteResolution> {
        let plan_checkpoint = *self;
        let references_checkpoint = *references;
        let resolution = self.resolve_cow_write(references, allocator, virtual_address)?;

        if resolution.copied {
            if let Err(error) = copier.copy_page(
                resolution.old_physical_address,
                resolution.new_physical_address,
            ) {
                *self = plan_checkpoint;
                *references = references_checkpoint;
                // `resolve_cow_write()` n’alloue qu’une frame dans ce cas.
                // La libérer explicitement évite de recopier le gros bitmap
                // de `FrameAllocator` sur la pile du kernel.
                let _ = allocator.free(PhysicalFrameRange {
                    base: resolution.new_physical_address.0,
                    count: 1,
                });
                return Err(error);
            }
        }

        Ok(resolution)
    }

    /// Réserve une frame physique puis l’ajoute au plan. Toute erreur de
    /// validation ou de chevauchement restitue immédiatement la frame.
    pub fn allocate_private_page(
        &mut self,
        allocator: &mut FrameAllocator,
        virtual_address: Address,
        permissions: PagePermissions,
    ) -> ExecResult<PhysicalFrameRange> {
        let frame = allocator.allocate(1)?;
        if let Err(error) = self.add_private_page(virtual_address, Address(frame.base), permissions)
        {
            let _ = allocator.free(frame);
            return Err(error);
        }
        Ok(frame)
    }

    pub fn compile(self) -> ExecResult<PageTablePlan> {
        let mut table = PageTablePlan::new();
        for mapping in self.mappings.iter().flatten() {
            table.map(*mapping)?;
        }
        Ok(table)
    }

    /// Traduit une adresse virtuelle à partir des plages logiques du plan.
    ///
    /// Cette vue est utile aux backends qui doivent valider une adresse U-Mode
    /// sans développer chaque page dans un `PageTablePlan` borné. La table
    /// Sv39 matérielle reste construite séparément par `compile_view()`.
    pub fn translate(&self, virtual_address: Address) -> ExecResult<(Address, PagePermissions)> {
        let page = virtual_address.0 / PAGE_SIZE;
        let offset = virtual_address.0 % PAGE_SIZE;
        self.mappings
            .iter()
            .flatten()
            .find_map(|mapping| {
                let first = mapping.virtual_range.first_page;
                let last = first.checked_add(mapping.virtual_range.page_count)?;
                if page < first || page >= last {
                    return None;
                }
                let page_offset = page.checked_sub(first)?;
                let physical = mapping
                    .physical_base
                    .checked_add(page_offset.checked_mul(PAGE_SIZE)?)?
                    .checked_add(offset)?;
                Some((Address(physical), mapping.permissions))
            })
            .ok_or(ExecError::NotFound)
    }

    pub fn compile_sv39(
        &self,
        root_physical: Address,
        table_physical_base: Address,
    ) -> ExecResult<Sv39PageTable> {
        let mut table = Sv39PageTable::new(root_physical, table_physical_base)?;
        for mapping in self.mappings.iter().flatten() {
            for offset in 0..mapping.virtual_range.page_count {
                table.map_page(
                    Address((mapping.virtual_range.first_page + offset) * PAGE_SIZE),
                    Address(mapping.physical_base + offset * PAGE_SIZE),
                    mapping.permissions,
                )?;
            }
        }
        Ok(table)
    }

    pub fn validate(&self) -> ExecResult<()> {
        for mapping in self.mappings.iter().flatten() {
            if mapping.virtual_range.page_count == 0 {
                return Err(ExecError::InvalidArgument);
            }
            PageTablePlan::encode_pte(PageMapping {
                virtual_page: mapping.virtual_range.first_page,
                physical_page: mapping.physical_base,
                permissions: mapping.permissions,
            })?;
        }
        Ok(())
    }

    pub fn compile_view<'a>(
        &self,
        root_physical: Address,
        table_physical_base: Address,
        root: &'a mut [u64; SV39_TABLE_ENTRIES],
        children: &'a mut [[u64; SV39_TABLE_ENTRIES]; SV39_MAX_CHILD_TABLES],
    ) -> ExecResult<Sv39PageTableView<'a>> {
        let mut table = Sv39PageTableView::new(root_physical, table_physical_base, root, children)?;
        for mapping in self.mappings.iter().flatten() {
            for offset in 0..mapping.virtual_range.page_count {
                table.map_page(
                    Address((mapping.virtual_range.first_page + offset) * PAGE_SIZE),
                    Address(mapping.physical_base + offset * PAGE_SIZE),
                    mapping.permissions,
                )?;
            }
        }
        Ok(table)
    }
}

impl Default for PageTablePlan {
    fn default() -> Self {
        Self::new()
    }
}

impl PageTablePlan {
    pub const fn new() -> Self {
        Self {
            entries: [None; MAX_PAGE_MAPPINGS],
        }
    }

    pub fn map(&mut self, plan: MappingPlan) -> ExecResult<()> {
        let free = self.entries.iter().filter(|entry| entry.is_none()).count();
        if free < plan.virtual_range.page_count as usize {
            return Err(ExecError::OutOfMemory);
        }
        for offset in 0..plan.virtual_range.page_count {
            let virtual_page = plan.virtual_range.first_page + offset;
            if self
                .entries
                .iter()
                .any(|entry| entry.is_some_and(|entry| entry.virtual_page == virtual_page))
            {
                return Err(ExecError::InvalidArgument);
            }
        }
        for offset in 0..plan.virtual_range.page_count {
            let virtual_page = plan.virtual_range.first_page + offset;
            let physical_page = plan.physical_base + offset * PAGE_SIZE;
            let slot = self
                .entries
                .iter()
                .position(Option::is_none)
                .ok_or(ExecError::OutOfMemory)?;
            self.entries[slot] = Some(PageMapping {
                virtual_page,
                physical_page,
                permissions: plan.permissions,
            });
        }
        Ok(())
    }

    pub fn translate(&self, virtual_address: Address) -> ExecResult<(Address, PagePermissions)> {
        let page = virtual_address.0 / PAGE_SIZE;
        let offset = virtual_address.0 % PAGE_SIZE;
        self.entries
            .iter()
            .find_map(|entry| entry.filter(|entry| entry.virtual_page == page))
            .map(|entry| (Address(entry.physical_page + offset), entry.permissions))
            .ok_or(ExecError::NotFound)
    }

    pub fn encode_pte(mapping: PageMapping) -> ExecResult<u64> {
        if mapping.physical_page % PAGE_SIZE != 0
            || mapping.physical_page >> 56 != 0
            || mapping.permissions.pte_flags() & (PTE_READ | PTE_WRITE | PTE_EXECUTE) == 0
        {
            return Err(ExecError::InvalidArgument);
        }
        Ok((mapping.physical_page >> 12) << 10 | mapping.permissions.pte_flags())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SatpValue(u64);

impl SatpValue {
    pub fn sv39(root_physical: Address) -> ExecResult<Self> {
        if root_physical.0 % PAGE_SIZE != 0 || root_physical.0 >> 56 != 0 {
            return Err(ExecError::InvalidArgument);
        }
        Ok(Self((SV39_MODE << 60) | (root_physical.0 >> 12)))
    }

    pub const fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessAddressSpace {
    pub process: crate::ProcessId,
    pub satp: u64,
    pub plan: Option<AddressSpacePlan>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessAddressSpaceRegistry {
    entries: [Option<ProcessAddressSpace>; MAX_PROCESS_ADDRESS_SPACES],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SharedFrameReferences {
    entries: [Option<(u64, u16)>; MAX_SHARED_FRAMES],
}

impl SharedFrameReferences {
    pub const fn new() -> Self {
        Self {
            entries: [None; MAX_SHARED_FRAMES],
        }
    }

    pub fn retain(&mut self, frame: Address) -> ExecResult<u16> {
        if frame.0 == 0 || frame.0 % PAGE_SIZE != 0 {
            return Err(ExecError::InvalidArgument);
        }
        if let Some(entry) = self
            .entries
            .iter_mut()
            .flatten()
            .find(|entry| entry.0 == frame.0)
        {
            entry.1 = entry.1.checked_add(1).ok_or(ExecError::OutOfMemory)?;
            return Ok(entry.1);
        }
        let slot = self
            .entries
            .iter()
            .position(Option::is_none)
            .ok_or(ExecError::OutOfMemory)?;
        self.entries[slot] = Some((frame.0, 1));
        Ok(1)
    }

    /// Libère une référence et indique si la frame n’a plus de détenteur.
    pub fn release(&mut self, frame: Address) -> ExecResult<bool> {
        let slot = self
            .entries
            .iter()
            .position(|entry| entry.is_some_and(|entry| entry.0 == frame.0))
            .ok_or(ExecError::NotFound)?;
        let (_, count) = self.entries[slot].ok_or(ExecError::NotFound)?;
        if count > 1 {
            self.entries[slot] = Some((frame.0, count - 1));
            Ok(false)
        } else {
            self.entries[slot] = None;
            Ok(true)
        }
    }

    pub fn count(&self, frame: Address) -> ExecResult<u16> {
        self.entries
            .iter()
            .flatten()
            .find(|entry| entry.0 == frame.0)
            .map(|entry| entry.1)
            .ok_or(ExecError::NotFound)
    }

    /// Prépare la résolution d’un fault d’écriture COW. La copie des octets
    /// est laissée au backend ; cette primitive ne gère que les références et
    /// la réservation de la nouvelle frame.
    pub fn prepare_write(
        &mut self,
        allocator: &mut FrameAllocator,
        shared_frame: Address,
    ) -> ExecResult<PhysicalFrameRange> {
        let count = self.count(shared_frame)?;
        if count == 1 {
            return Ok(PhysicalFrameRange {
                base: shared_frame.0,
                count: 1,
            });
        }
        let replacement = allocator.allocate(1)?;
        if let Err(error) = self.release(shared_frame) {
            let _ = allocator.free(replacement);
            return Err(error);
        }
        if let Err(error) = self.retain(Address(replacement.base)) {
            // Restaurer la référence originale si la publication de la frame
            // de remplacement échoue. La réservation et le registre restent
            // ainsi transactionnels même lorsque le registre est saturé.
            let _ = self.retain(shared_frame);
            let _ = allocator.free(replacement);
            return Err(error);
        }
        Ok(replacement)
    }
}

impl ProcessAddressSpaceRegistry {
    pub const fn new() -> Self {
        Self {
            entries: [None; MAX_PROCESS_ADDRESS_SPACES],
        }
    }

    pub fn register(&mut self, process: crate::ProcessId, satp: u64) -> ExecResult<()> {
        if process == 0 || satp >> 60 != SV39_MODE || satp & ((1 << 44) - 1) == 0 {
            return Err(ExecError::InvalidArgument);
        }
        if self
            .entries
            .iter()
            .any(|entry| entry.is_some_and(|entry| entry.process == process))
        {
            return Err(ExecError::InvalidArgument);
        }
        let slot = self
            .entries
            .iter()
            .position(Option::is_none)
            .ok_or(ExecError::OutOfMemory)?;
        self.entries[slot] = Some(ProcessAddressSpace {
            process,
            satp,
            plan: None,
        });
        Ok(())
    }

    pub fn register_plan(
        &mut self,
        process: crate::ProcessId,
        satp: u64,
        plan: AddressSpacePlan,
    ) -> ExecResult<()> {
        plan.validate()?;
        self.register(process, satp)?;
        let entry = self
            .entries
            .iter_mut()
            .flatten()
            .find(|entry| entry.process == process)
            .ok_or(ExecError::NotFound)?;
        entry.plan = Some(plan);
        Ok(())
    }

    pub fn satp(&self, process: crate::ProcessId) -> ExecResult<u64> {
        self.entries
            .iter()
            .flatten()
            .find(|entry| entry.process == process)
            .map(|entry| entry.satp)
            .ok_or(ExecError::NotFound)
    }

    pub fn plan(&self, process: crate::ProcessId) -> ExecResult<AddressSpacePlan> {
        self.entries
            .iter()
            .flatten()
            .find(|entry| entry.process == process)
            .and_then(|entry| entry.plan)
            .ok_or(ExecError::NotFound)
    }

    /// Clone le plan logique d’un espace vers un nouveau processus. Les
    /// frames restent partagées à ce niveau ; le backend pourra ensuite
    /// remplacer les pages privées avant de charger le nouveau `satp`.
    pub fn clone_plan(
        &mut self,
        parent: crate::ProcessId,
        child: crate::ProcessId,
        child_satp: u64,
    ) -> ExecResult<()> {
        let plan = self.plan(parent)?;
        self.register_plan(child, child_satp, plan)
    }

    /// Clone un espace en partageant ses pages utilisateur inscriptibles.
    /// Les mappings concernés deviennent COW dans le parent et l’enfant ; le
    /// registre de références reçoit exactement un détenteur supplémentaire
    /// par page. Les pages kernel et utilisateur déjà en lecture seule restent
    /// simplement recopiées dans le plan logique.
    pub fn clone_plan_cow(
        &mut self,
        parent: crate::ProcessId,
        child: crate::ProcessId,
        child_satp: u64,
        references: &mut SharedFrameReferences,
    ) -> ExecResult<()> {
        let parent_slot = self
            .entries
            .iter()
            .position(|entry| entry.is_some_and(|entry| entry.process == parent))
            .ok_or(ExecError::NotFound)?;
        if self
            .entries
            .iter()
            .any(|entry| entry.is_some_and(|entry| entry.process == child))
        {
            return Err(ExecError::InvalidArgument);
        }
        let parent_plan = self.entries[parent_slot]
            .and_then(|entry| entry.plan)
            .ok_or(ExecError::NotFound)?;
        let mut child_plan = parent_plan;
        let references_checkpoint = *references;
        let mut parent_plan_updated = parent_plan;

        let result = (|| {
            for child_mapping in child_plan.mappings.iter_mut().flatten() {
                if !child_mapping.permissions.user {
                    continue;
                }
                let was_writable = child_mapping.permissions.write;
                let was_cow = child_mapping.permissions == PagePermissions::COW_USER;
                if !was_writable && !was_cow {
                    continue;
                }
                child_mapping.permissions = PagePermissions::COW_USER;
                let parent_mapping = parent_plan_updated
                    .mappings
                    .iter_mut()
                    .flatten()
                    .find(|mapping| {
                        mapping.virtual_range == child_mapping.virtual_range
                            && mapping.physical_base == child_mapping.physical_base
                    })
                    .ok_or(ExecError::NotFound)?;
                parent_mapping.permissions = PagePermissions::COW_USER;

                for offset in 0..child_mapping.virtual_range.page_count {
                    let frame = Address(child_mapping.physical_base + offset * PAGE_SIZE);
                    if was_writable {
                        // Une page jusque-là privée doit recevoir sa référence
                        // parent avant celle de l’enfant.
                        references.retain(frame)?;
                    }
                    references.retain(frame)?;
                }
            }
            self.register_plan(child, child_satp, child_plan)
        })();
        if let Err(error) = result {
            *references = references_checkpoint;
            return Err(error);
        }
        self.entries[parent_slot]
            .as_mut()
            .ok_or(ExecError::NotFound)?
            .plan = Some(parent_plan_updated);
        Ok(())
    }

    /// Résout une écriture COW dans l’espace d’un processus et délègue la
    /// copie des octets au backend fourni par la plateforme.
    pub fn resolve_cow_write_with_copy<C: PageCopier>(
        &mut self,
        process: crate::ProcessId,
        references: &mut SharedFrameReferences,
        allocator: &mut FrameAllocator,
        virtual_address: Address,
        copier: &mut C,
    ) -> ExecResult<CowWriteResolution> {
        let entry = self
            .entries
            .iter_mut()
            .flatten()
            .find(|entry| entry.process == process)
            .ok_or(ExecError::NotFound)?;
        entry
            .plan
            .as_mut()
            .ok_or(ExecError::NotFound)?
            .resolve_cow_write_with_copy(references, allocator, virtual_address, copier)
    }

    pub fn remove(&mut self, process: crate::ProcessId) -> ExecResult<()> {
        let slot = self
            .entries
            .iter()
            .position(|entry| entry.is_some_and(|entry| entry.process == process))
            .ok_or(ExecError::NotFound)?;
        self.entries[slot] = None;
        Ok(())
    }

    /// Retire un espace et libère ses références COW.
    ///
    /// Le booléen retourné par `SharedFrameReferences::release` indique au
    /// backend quelles frames n’ont plus de détenteur. Cette méthode ne rend
    /// pas elle-même la mémoire physique à l’allocateur, car certaines frames
    /// peuvent provenir d’un autre domaine de propriété.
    pub fn remove_with_references(
        &mut self,
        process: crate::ProcessId,
        references: &mut SharedFrameReferences,
    ) -> ExecResult<ReleasedCowFrames> {
        let slot = self
            .entries
            .iter()
            .position(|entry| entry.is_some_and(|entry| entry.process == process))
            .ok_or(ExecError::NotFound)?;
        let plan = self.entries[slot].and_then(|entry| entry.plan);
        let references_checkpoint = *references;
        let result = (|| {
            let mut released = ReleasedCowFrames::new();
            if let Some(plan) = plan {
                for mapping in plan.mappings.iter().flatten() {
                    if !mapping.permissions.user {
                        continue;
                    }
                    for offset in 0..mapping.virtual_range.page_count {
                        let frame = Address(mapping.physical_base + offset * PAGE_SIZE);
                        // Une frame issue d’une résolution COW peut déjà être
                        // privée dans le mapping ; sa présence dans le registre
                        // suffit à identifier la référence à retirer.
                        if references.count(frame).is_ok() {
                            if references.release(frame)? {
                                released.push_unique(frame)?;
                            }
                        }
                    }
                }
            }
            Ok(released)
        })();
        if let Err(error) = result {
            *references = references_checkpoint;
            return Err(error);
        }
        self.entries[slot] = None;
        result
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Sv39PageTable {
    #[doc(hidden)]
    pub root: [u64; SV39_TABLE_ENTRIES],
    #[doc(hidden)]
    pub children: [[u64; SV39_TABLE_ENTRIES]; SV39_MAX_CHILD_TABLES],
    root_physical: u64,
    table_physical_base: u64,
    next_table: usize,
}

pub struct Sv39PageTableView<'a> {
    root_physical: u64,
    table_physical_base: u64,
    next_table: usize,
    root: &'a mut [u64; SV39_TABLE_ENTRIES],
    children: &'a mut [[u64; SV39_TABLE_ENTRIES]; SV39_MAX_CHILD_TABLES],
}

impl<'a> Sv39PageTableView<'a> {
    pub fn new(
        root_physical: Address,
        table_physical_base: Address,
        root: &'a mut [u64; SV39_TABLE_ENTRIES],
        children: &'a mut [[u64; SV39_TABLE_ENTRIES]; SV39_MAX_CHILD_TABLES],
    ) -> ExecResult<Self> {
        SatpValue::sv39(root_physical)?;
        if table_physical_base.0 % PAGE_SIZE != 0 {
            return Err(ExecError::InvalidArgument);
        }
        *root = [0; SV39_TABLE_ENTRIES];
        *children = [[0; SV39_TABLE_ENTRIES]; SV39_MAX_CHILD_TABLES];
        Ok(Self {
            root_physical: root_physical.0,
            table_physical_base: table_physical_base.0,
            next_table: 0,
            root,
            children,
        })
    }

    /// Ouvre une vue sur des tables déjà construites sans effacer leurs
    /// entrées. Cette vue est limitée aux mises à jour de feuilles, notamment
    /// après un fault COW ; la construction initiale continue d’utiliser
    /// [`Self::new`].
    pub fn from_existing(
        root_physical: Address,
        table_physical_base: Address,
        root: &'a mut [u64; SV39_TABLE_ENTRIES],
        children: &'a mut [[u64; SV39_TABLE_ENTRIES]; SV39_MAX_CHILD_TABLES],
    ) -> ExecResult<Self> {
        SatpValue::sv39(root_physical)?;
        if table_physical_base.0 % PAGE_SIZE != 0 {
            return Err(ExecError::InvalidArgument);
        }
        Ok(Self {
            root_physical: root_physical.0,
            table_physical_base: table_physical_base.0,
            next_table: 0,
            root,
            children,
        })
    }

    pub fn map_page(
        &mut self,
        virtual_address: Address,
        physical_address: Address,
        permissions: PagePermissions,
    ) -> ExecResult<()> {
        let virtual_address = validate_sv39_address(virtual_address)?;
        if physical_address.0 % PAGE_SIZE != 0 {
            return Err(ExecError::InvalidArgument);
        }
        let vpn0 = ((virtual_address.0 >> 12) & 0x1ff) as usize;
        let vpn1 = ((virtual_address.0 >> 21) & 0x1ff) as usize;
        let vpn2 = ((virtual_address.0 >> 30) & 0x1ff) as usize;
        let l1 = self.ensure_child(true, vpn2, 0)?;
        let l0 = self.ensure_child(false, vpn1, l1)?;
        if self.children[l0][vpn0] != 0 {
            return Err(ExecError::InvalidArgument);
        }
        self.children[l0][vpn0] = (physical_address.0 >> 12) << 10 | permissions.pte_flags();
        Ok(())
    }

    /// Remplace le PTE d’une page déjà présente sans reconstruire la table.
    /// Cette opération est utilisée après une résolution COW réussie.
    pub fn replace_page(
        &mut self,
        virtual_address: Address,
        physical_address: Address,
        permissions: PagePermissions,
    ) -> ExecResult<()> {
        let virtual_address = validate_sv39_address(virtual_address)?;
        if physical_address.0 % PAGE_SIZE != 0 {
            return Err(ExecError::InvalidArgument);
        }
        let pte = PageTablePlan::encode_pte(PageMapping {
            virtual_page: virtual_address.0 / PAGE_SIZE,
            physical_page: physical_address.0,
            permissions,
        })?;
        let vpn0 = ((virtual_address.0 >> 12) & 0x1ff) as usize;
        let vpn1 = ((virtual_address.0 >> 21) & 0x1ff) as usize;
        let vpn2 = ((virtual_address.0 >> 30) & 0x1ff) as usize;
        let l1 = self.child_from_entry(self.root[vpn2])?;
        let l0 = self.child_from_entry(self.children[l1][vpn1])?;
        if self.children[l0][vpn0] == 0 {
            return Err(ExecError::NotFound);
        }
        self.children[l0][vpn0] = pte;
        Ok(())
    }

    fn child_from_entry(&self, entry: u64) -> ExecResult<usize> {
        if entry == 0 || entry & (PTE_READ | PTE_WRITE | PTE_EXECUTE) != 0 {
            return Err(ExecError::NotFound);
        }
        let physical = (entry >> 10) * PAGE_SIZE;
        if physical < self.table_physical_base {
            return Err(ExecError::InvalidArgument);
        }
        let child = ((physical - self.table_physical_base) / PAGE_SIZE) as usize;
        if child >= SV39_MAX_CHILD_TABLES {
            return Err(ExecError::InvalidArgument);
        }
        Ok(child)
    }

    fn ensure_child(&mut self, root: bool, index: usize, parent: usize) -> ExecResult<usize> {
        let entry = if root {
            self.root[index]
        } else {
            self.children[parent][index]
        };
        if entry != 0 {
            if entry & (PTE_READ | PTE_WRITE | PTE_EXECUTE) != 0 {
                return Err(ExecError::InvalidArgument);
            }
            let physical = (entry >> 10) * PAGE_SIZE;
            if physical < self.table_physical_base {
                return Err(ExecError::InvalidArgument);
            }
            let child = ((physical - self.table_physical_base) / PAGE_SIZE) as usize;
            if child >= self.next_table {
                return Err(ExecError::InvalidArgument);
            }
            return Ok(child);
        }
        if self.next_table == SV39_MAX_CHILD_TABLES {
            return Err(ExecError::OutOfMemory);
        }
        let child = self.next_table;
        self.next_table += 1;
        let physical = self.table_physical_base + child as u64 * PAGE_SIZE;
        if root {
            self.root[index] = (physical >> 12) << 10 | PTE_VALID;
        } else {
            self.children[parent][index] = (physical >> 12) << 10 | PTE_VALID;
        }
        Ok(child)
    }

    pub const fn satp(&self) -> u64 {
        (SV39_MODE << 60) | (self.root_physical >> 12)
    }
}

impl Sv39PageTable {
    pub fn new(root_physical: Address, table_physical_base: Address) -> ExecResult<Self> {
        SatpValue::sv39(root_physical)?;
        if table_physical_base.0 % PAGE_SIZE != 0
            || table_physical_base.0 >> 56 != 0
            || table_physical_base
                .0
                .checked_add((SV39_MAX_CHILD_TABLES as u64) * PAGE_SIZE)
                .is_none()
        {
            return Err(ExecError::InvalidArgument);
        }
        Ok(Self {
            root: [0; SV39_TABLE_ENTRIES],
            children: [[0; SV39_TABLE_ENTRIES]; SV39_MAX_CHILD_TABLES],
            root_physical: root_physical.0,
            table_physical_base: table_physical_base.0,
            next_table: 0,
        })
    }

    pub fn map_page(
        &mut self,
        virtual_address: Address,
        physical_address: Address,
        permissions: PagePermissions,
    ) -> ExecResult<()> {
        let virtual_address = validate_sv39_address(virtual_address)?;
        if physical_address.0 % PAGE_SIZE != 0 {
            return Err(ExecError::InvalidArgument);
        }
        let flags = permissions.pte_flags();
        let vpn0 = ((virtual_address.0 >> 12) & 0x1ff) as usize;
        let vpn1 = ((virtual_address.0 >> 21) & 0x1ff) as usize;
        let vpn2 = ((virtual_address.0 >> 30) & 0x1ff) as usize;
        let l1 = if self.root[vpn2] == 0 {
            let child = self.allocate_child()?;
            self.root[vpn2] = self.child_pte(child);
            child
        } else {
            if self.root[vpn2] & (PTE_READ | PTE_WRITE | PTE_EXECUTE) != 0 {
                return Err(ExecError::InvalidArgument);
            }
            self.child_index(self.root[vpn2])?
        };
        let l0 = if self.children[l1][vpn1] == 0 {
            let child = self.allocate_child()?;
            self.children[l1][vpn1] = self.child_pte(child);
            child
        } else {
            if self.children[l1][vpn1] & (PTE_READ | PTE_WRITE | PTE_EXECUTE) != 0 {
                return Err(ExecError::InvalidArgument);
            }
            self.child_index(self.children[l1][vpn1])?
        };
        if self.children[l0][vpn0] != 0 {
            return Err(ExecError::InvalidArgument);
        }
        self.children[l0][vpn0] = (physical_address.0 >> 12) << 10 | flags;
        Ok(())
    }

    /// Remplace le PTE d’une page déjà présente sans reconstruire la table.
    pub fn replace_page(
        &mut self,
        virtual_address: Address,
        physical_address: Address,
        permissions: PagePermissions,
    ) -> ExecResult<()> {
        let virtual_address = validate_sv39_address(virtual_address)?;
        if physical_address.0 % PAGE_SIZE != 0 {
            return Err(ExecError::InvalidArgument);
        }
        let pte = PageTablePlan::encode_pte(PageMapping {
            virtual_page: virtual_address.0 / PAGE_SIZE,
            physical_page: physical_address.0,
            permissions,
        })?;
        let vpn0 = ((virtual_address.0 >> 12) & 0x1ff) as usize;
        let vpn1 = ((virtual_address.0 >> 21) & 0x1ff) as usize;
        let vpn2 = ((virtual_address.0 >> 30) & 0x1ff) as usize;
        let l1 = self.child_index(self.root[vpn2])?;
        let l0 = self.child_index(self.children[l1][vpn1])?;
        if self.children[l0][vpn0] == 0 {
            return Err(ExecError::NotFound);
        }
        self.children[l0][vpn0] = pte;
        Ok(())
    }

    fn allocate_child(&mut self) -> ExecResult<usize> {
        if self.next_table == SV39_MAX_CHILD_TABLES {
            return Err(ExecError::OutOfMemory);
        }
        let child = self.next_table;
        self.next_table += 1;
        Ok(child)
    }

    fn child_pte(&self, child: usize) -> u64 {
        let physical = self.table_physical_base + child as u64 * PAGE_SIZE;
        (physical >> 12) << 10 | PTE_VALID
    }

    fn child_index(&self, entry: u64) -> ExecResult<usize> {
        let physical = (entry >> 10) * PAGE_SIZE;
        if physical < self.table_physical_base {
            return Err(ExecError::InvalidArgument);
        }
        let index = ((physical - self.table_physical_base) / PAGE_SIZE) as usize;
        if index >= self.next_table {
            return Err(ExecError::InvalidArgument);
        }
        Ok(index)
    }

    pub fn satp(&self) -> SatpValue {
        SatpValue::sv39(Address(self.root_physical)).expect("validated Sv39 root")
    }

    pub fn root_entry(&self, index: usize) -> Option<u64> {
        self.root.get(index).copied().filter(|entry| *entry != 0)
    }
}

impl MappingPlan {
    pub fn new(
        virtual_range: UserPageRange,
        physical_base: u64,
        permissions: PagePermissions,
    ) -> ExecResult<Self> {
        if physical_base % PAGE_SIZE != 0
            || !permissions.user
            || (!permissions.read && !permissions.write && !permissions.execute)
        {
            return Err(ExecError::InvalidArgument);
        }
        let bytes = virtual_range
            .page_count
            .checked_mul(PAGE_SIZE)
            .ok_or(ExecError::OutOfMemory)?;
        physical_base
            .checked_add(bytes)
            .ok_or(ExecError::OutOfMemory)?;
        Ok(Self {
            virtual_range,
            physical_base,
            permissions,
        })
    }

    pub fn kernel(
        virtual_range: UserPageRange,
        physical_base: u64,
        permissions: PagePermissions,
    ) -> ExecResult<Self> {
        if physical_base % PAGE_SIZE != 0
            || permissions.user
            || (!permissions.read && !permissions.write && !permissions.execute)
        {
            return Err(ExecError::InvalidArgument);
        }
        let bytes = virtual_range
            .page_count
            .checked_mul(PAGE_SIZE)
            .ok_or(ExecError::OutOfMemory)?;
        physical_base
            .checked_add(bytes)
            .ok_or(ExecError::OutOfMemory)?;
        Ok(Self {
            virtual_range,
            physical_base,
            permissions,
        })
    }
}

impl UserPageRange {
    pub fn new(address: Address, length: u64) -> ExecResult<Self> {
        if length == 0 {
            return Err(ExecError::InvalidArgument);
        }
        let start = validate_user_address(address)?.0;
        let end = start
            .checked_add(length - 1)
            .ok_or(ExecError::InvalidArgument)?;
        if end > SV39_USER_MAX || !is_sv39_canonical(end) {
            return Err(ExecError::InvalidArgument);
        }
        let first_page = start / PAGE_SIZE;
        let last_page = end / PAGE_SIZE;
        Ok(Self {
            first_page,
            page_count: last_page - first_page + 1,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakePageCopier {
        fail: bool,
        calls: usize,
        source: Option<Address>,
        destination: Option<Address>,
        source_page: [u8; PAGE_SIZE as usize],
        destination_page: [u8; PAGE_SIZE as usize],
    }

    impl FakePageCopier {
        fn new(fail: bool) -> Self {
            Self {
                fail,
                calls: 0,
                source: None,
                destination: None,
                source_page: [0; PAGE_SIZE as usize],
                destination_page: [0; PAGE_SIZE as usize],
            }
        }
    }

    impl PageCopier for FakePageCopier {
        fn copy_page(&mut self, source: Address, destination: Address) -> ExecResult<()> {
            self.calls += 1;
            self.source = Some(source);
            self.destination = Some(destination);
            if self.fail {
                return Err(ExecError::Fault);
            }
            self.destination_page.copy_from_slice(&self.source_page);
            Ok(())
        }
    }

    #[test]
    fn sv39_accepts_user_half_and_kernel_canonical_half() {
        assert!(is_sv39_canonical(0x3f_ffff_ffff));
        assert!(is_sv39_canonical(0xffff_ffc0_0000_0000));
        assert!(!is_sv39_canonical(0x4000_0000_0000));
        assert!(!is_sv39_canonical(0x8000_0000_0000));
    }

    #[test]
    fn user_page_range_rounds_end_to_a_page() {
        assert_eq!(
            UserPageRange::new(Address(0x1fff), 2),
            Ok(UserPageRange {
                first_page: 1,
                page_count: 2
            })
        );
        assert_eq!(
            UserPageRange::new(Address(SV39_USER_MAX), 2),
            Err(ExecError::InvalidArgument)
        );
    }

    #[test]
    fn mapping_plan_requires_aligned_physical_pages_and_user_permissions() {
        let range = UserPageRange::new(Address(0x2000), 0x1000).expect("range");
        let plan = MappingPlan::new(range, 0x8000, PagePermissions::USER_STACK).expect("mapping");
        assert_eq!(plan.physical_base, 0x8000);
        assert_eq!(
            MappingPlan::new(range, 0x8001, PagePermissions::USER_STACK),
            Err(ExecError::InvalidArgument)
        );
        assert_eq!(
            MappingPlan::new(
                range,
                0x8000,
                PagePermissions {
                    user: false,
                    ..PagePermissions::USER_STACK
                }
            ),
            Err(ExecError::InvalidArgument)
        );
    }

    #[test]
    fn address_space_plan_adds_one_private_user_page() {
        let mut plan = AddressSpacePlan::new();
        plan.add_private_page(
            Address(0x4000),
            Address(0x9000),
            PagePermissions::USER_STACK,
        )
        .expect("private page");
        assert_eq!(
            plan.add_private_page(
                Address(0x4000),
                Address(0xa000),
                PagePermissions::USER_STACK
            ),
            Err(ExecError::InvalidArgument)
        );
        let table = plan.compile().expect("plan");
        assert_eq!(
            table.translate(Address(0x4abc)),
            Ok((Address(0x9abc), PagePermissions::USER_STACK))
        );
    }

    #[test]
    fn address_space_plan_translates_inside_a_large_mapping() {
        let mut plan = AddressSpacePlan::new();
        let mapping = MappingPlan::kernel(
            UserPageRange {
                first_page: 0x8020_0000 / PAGE_SIZE,
                page_count: 1024,
            },
            0x8020_0000,
            PagePermissions {
                read: true,
                write: true,
                execute: true,
                user: false,
            },
        )
        .expect("large mapping");
        plan.add(mapping).expect("mapping");

        assert_eq!(
            plan.translate(Address(0x8020_4abc)),
            Ok((Address(0x8020_4abc), mapping.permissions))
        );
        assert_eq!(
            plan.translate(Address(0x8060_0000)),
            Err(ExecError::NotFound)
        );
    }

    #[test]
    fn address_space_plan_allocates_and_rolls_back_private_page() {
        let mut allocator = FrameAllocator::new(0x20_000, 2).expect("allocator");
        let mut plan = AddressSpacePlan::new();
        let frame = plan
            .allocate_private_page(&mut allocator, Address(0x5000), PagePermissions::USER_STACK)
            .expect("private frame");
        assert_eq!(frame.base, 0x20_000);
        let frame = plan.allocate_private_page(
            &mut allocator,
            Address(0x5000),
            PagePermissions::USER_STACK,
        );
        assert_eq!(frame, Err(ExecError::InvalidArgument));
        assert_eq!(
            allocator.allocate(1).expect("reclaimed frame").base,
            0x21_000
        );
    }

    #[test]
    fn frame_allocator_reclaims_owned_cow_frames() {
        let mut allocator = FrameAllocator::new(0x30_000, 2).expect("allocator");
        let allocated = allocator.allocate(1).expect("frame");
        let mut released = ReleasedCowFrames::new();
        released
            .push_unique(Address(allocated.base))
            .expect("report");

        assert_eq!(allocator.reclaim_cow_frames(&released), Ok(1));
        assert_eq!(
            allocator.allocate(1).expect("reclaimed").base,
            allocated.base
        );
    }

    #[test]
    fn frame_allocator_ignores_cow_frames_owned_by_another_backend() {
        let mut allocator = FrameAllocator::new(0x30_000, 1).expect("allocator");
        let mut released = ReleasedCowFrames::new();
        released
            .push_unique(Address(0x20_000))
            .expect("foreign report");

        assert_eq!(allocator.reclaim_cow_frames(&released), Ok(0));
        assert_eq!(allocator.allocate(1).expect("local frame").base, 0x30_000);
    }

    #[test]
    fn frame_allocator_rejects_reclaim_of_an_already_free_owned_frame() {
        let mut allocator = FrameAllocator::new(0x30_000, 1).expect("allocator");
        let mut released = ReleasedCowFrames::new();
        released.push_unique(Address(0x30_000)).expect("report");

        assert_eq!(
            allocator.reclaim_cow_frames(&released),
            Err(ExecError::InvalidArgument)
        );
        assert_eq!(allocator.allocate(1).expect("still free").base, 0x30_000);
    }

    #[test]
    fn page_table_plan_maps_and_translates_each_page() {
        let range = UserPageRange::new(Address(0x4000), 0x2000).expect("range");
        let mapping =
            MappingPlan::new(range, 0x9000, PagePermissions::USER_STACK).expect("mapping");
        let mut table = PageTablePlan::new();
        table.map(mapping).expect("map");

        assert_eq!(
            table.translate(Address(0x4abc)),
            Ok((Address(0x9abc), PagePermissions::USER_STACK))
        );
        assert_eq!(table.map(mapping), Err(ExecError::InvalidArgument));
        assert_eq!(
            PageTablePlan::encode_pte(PageMapping {
                virtual_page: 4,
                physical_page: 0x9000,
                permissions: PagePermissions::USER_STACK,
            }),
            Ok((0x9000 >> 12) << 10 | PagePermissions::USER_STACK.pte_flags())
        );
    }

    #[test]
    fn satp_value_requires_aligned_sv39_root() {
        assert_eq!(
            SatpValue::sv39(Address(0x8000)).expect("satp").raw() >> 60,
            SV39_MODE
        );
        assert_eq!(
            SatpValue::sv39(Address(0x8001)),
            Err(ExecError::InvalidArgument)
        );
    }

    #[test]
    fn process_address_spaces_bind_and_remove_satp() {
        let mut registry = ProcessAddressSpaceRegistry::new();
        let satp = SatpValue::sv39(Address(0x80_000)).expect("satp").raw();
        registry.register(7, satp).expect("register");
        assert_eq!(registry.satp(7), Ok(satp));
        assert_eq!(registry.register(7, satp), Err(ExecError::InvalidArgument));
        registry.remove(7).expect("remove");
        assert_eq!(registry.satp(7), Err(ExecError::NotFound));
    }

    #[test]
    fn process_address_spaces_reject_invalid_satp() {
        let mut registry = ProcessAddressSpaceRegistry::new();
        assert_eq!(registry.register(7, 0), Err(ExecError::InvalidArgument));
        assert_eq!(
            registry.register(0, 8 << 60 | 1),
            Err(ExecError::InvalidArgument)
        );
    }

    #[test]
    fn process_address_spaces_clone_a_validated_plan() {
        let mut registry = ProcessAddressSpaceRegistry::new();
        let mut plan = AddressSpacePlan::new();
        plan.add_private_page(
            Address(0x4000),
            Address(0x9000),
            PagePermissions::USER_STACK,
        )
        .expect("page");
        registry
            .register_plan(1, (SV39_MODE << 60) | 1, plan)
            .expect("parent");
        registry
            .clone_plan(1, 2, (SV39_MODE << 60) | 2)
            .expect("child");
        assert_eq!(registry.plan(2), Ok(plan));
    }

    #[test]
    fn process_address_spaces_clone_writable_pages_as_cow() {
        let mut registry = ProcessAddressSpaceRegistry::new();
        let mut plan = AddressSpacePlan::new();
        plan.add_private_page(
            Address(0x4000),
            Address(0x9000),
            PagePermissions::USER_STACK,
        )
        .expect("page");
        registry
            .register_plan(1, (SV39_MODE << 60) | 1, plan)
            .expect("parent");
        let mut references = SharedFrameReferences::new();

        registry
            .clone_plan_cow(1, 2, (SV39_MODE << 60) | 2, &mut references)
            .expect("cow child");

        assert_eq!(references.count(Address(0x9000)), Ok(2));
        assert_eq!(
            registry
                .plan(1)
                .expect("parent plan")
                .compile()
                .expect("parent table")
                .translate(Address(0x4000)),
            Ok((Address(0x9000), PagePermissions::COW_USER))
        );
        assert_eq!(
            registry
                .plan(2)
                .expect("child plan")
                .compile()
                .expect("child table")
                .translate(Address(0x4000)),
            Ok((Address(0x9000), PagePermissions::COW_USER))
        );
    }

    #[test]
    fn process_address_spaces_route_cow_fault_to_the_selected_process() {
        let shared = Address(0x20_000);
        let mut registry = ProcessAddressSpaceRegistry::new();
        let mut plan = AddressSpacePlan::new();
        plan.add_private_page(Address(0x4000), shared, PagePermissions::USER_STACK)
            .expect("page");
        registry
            .register_plan(1, (SV39_MODE << 60) | 1, plan)
            .expect("parent");
        let mut references = SharedFrameReferences::new();
        registry
            .clone_plan_cow(1, 2, (SV39_MODE << 60) | 2, &mut references)
            .expect("child");
        let mut allocator = FrameAllocator::new(0x30_000, 1).expect("allocator");
        let mut copier = FakePageCopier::new(false);

        let resolution = registry
            .resolve_cow_write_with_copy(
                2,
                &mut references,
                &mut allocator,
                Address(0x4abc),
                &mut copier,
            )
            .expect("child fault");

        assert_eq!(resolution.virtual_address, Address(0x4000));
        assert_eq!(resolution.new_physical_address, Address(0x30_000));
        assert_eq!(
            registry
                .plan(1)
                .expect("parent")
                .compile()
                .expect("parent table")
                .translate(Address(0x4000)),
            Ok((shared, PagePermissions::COW_USER))
        );
        assert_eq!(
            registry
                .plan(2)
                .expect("child")
                .compile()
                .expect("child table")
                .translate(Address(0x4000)),
            Ok((Address(0x30_000), PagePermissions::USER_STACK))
        );
    }

    #[test]
    fn process_address_spaces_release_cow_references_on_reap() {
        let shared = Address(0x20_000);
        let mut registry = ProcessAddressSpaceRegistry::new();
        let mut plan = AddressSpacePlan::new();
        plan.add_private_page(Address(0x4000), shared, PagePermissions::USER_STACK)
            .expect("page");
        registry
            .register_plan(1, (SV39_MODE << 60) | 1, plan)
            .expect("parent");
        let mut references = SharedFrameReferences::new();
        registry
            .clone_plan_cow(1, 2, (SV39_MODE << 60) | 2, &mut references)
            .expect("child");

        let released_child = registry
            .remove_with_references(2, &mut references)
            .expect("child reap");
        assert!(released_child.is_empty());
        assert_eq!(references.count(shared), Ok(1));
        assert_eq!(registry.plan(2), Err(ExecError::NotFound));

        let released_parent = registry
            .remove_with_references(1, &mut references)
            .expect("parent reap");
        assert_eq!(released_parent.len(), 1);
        assert_eq!(released_parent.get(0), Some(shared));
        assert_eq!(references.count(shared), Err(ExecError::NotFound));
    }

    #[test]
    fn shared_frame_references_release_on_last_owner() {
        let mut refs = SharedFrameReferences::new();
        let frame = Address(0x20_000);
        assert_eq!(refs.retain(frame), Ok(1));
        assert_eq!(refs.retain(frame), Ok(2));
        assert_eq!(refs.count(frame), Ok(2));
        assert_eq!(refs.release(frame), Ok(false));
        assert_eq!(refs.release(frame), Ok(true));
        assert_eq!(refs.count(frame), Err(ExecError::NotFound));
    }

    #[test]
    fn cow_write_prepares_replacement_only_when_shared() {
        let mut refs = SharedFrameReferences::new();
        let mut allocator = FrameAllocator::new(0x30_000, 2).expect("allocator");
        let shared = Address(0x20_000);
        refs.retain(shared).expect("owner");
        refs.retain(shared).expect("second owner");
        let replacement = refs
            .prepare_write(&mut allocator, shared)
            .expect("replacement");
        assert_eq!(replacement.base, 0x30_000);
        assert_eq!(refs.count(shared), Ok(1));
        assert_eq!(refs.count(Address(replacement.base)), Ok(1));
    }

    #[test]
    fn address_space_plan_replaces_cow_page_with_private_mapping() {
        let mut plan = AddressSpacePlan::new();
        plan.add_cow_page(CowPage::new(Address(0x7000), Address(0x20_000)).expect("cow"))
            .expect("cow mapping");
        plan.replace_private_page(
            Address(0x7000),
            Address(0x30_000),
            PagePermissions::USER_STACK,
        )
        .expect("private mapping");
        assert_eq!(
            plan.compile().expect("plan").translate(Address(0x7001)),
            Ok((Address(0x30_001), PagePermissions::USER_STACK))
        );
    }

    #[test]
    fn address_space_plan_resolves_shared_cow_write() {
        let shared = Address(0x20_000);
        let mut references = SharedFrameReferences::new();
        references.retain(shared).expect("owner");
        references.retain(shared).expect("second owner");
        let mut allocator = FrameAllocator::new(0x30_000, 1).expect("allocator");
        let mut plan = AddressSpacePlan::new();
        plan.add_cow_page(CowPage::new(Address(0x8000), shared).expect("cow"))
            .expect("mapping");
        let resolution = plan
            .resolve_cow_write(&mut references, &mut allocator, Address(0x8123))
            .expect("resolve");
        assert!(resolution.copied);
        assert_eq!(resolution.virtual_address, Address(0x8000));
        assert_eq!(resolution.new_physical_address, Address(0x30_000));
        assert_eq!(references.count(shared), Ok(1));
    }

    #[test]
    fn address_space_plan_cow_copy_backend_copies_the_page() {
        let shared = Address(0x20_000);
        let mut references = SharedFrameReferences::new();
        references.retain(shared).expect("owner");
        references.retain(shared).expect("second owner");
        let mut allocator = FrameAllocator::new(0x30_000, 1).expect("allocator");
        let mut plan = AddressSpacePlan::new();
        plan.add_cow_page(CowPage::new(Address(0x8000), shared).expect("cow"))
            .expect("mapping");
        let mut copier = FakePageCopier::new(false);
        copier.source_page[0] = 0xa5;
        copier.source_page[PAGE_SIZE as usize - 1] = 0x5a;

        let resolution = plan
            .resolve_cow_write_with_copy(
                &mut references,
                &mut allocator,
                Address(0x8000),
                &mut copier,
            )
            .expect("copy");

        assert!(resolution.copied);
        assert_eq!(copier.calls, 1);
        assert_eq!(copier.source, Some(shared));
        assert_eq!(copier.destination, Some(Address(0x30_000)));
        assert_eq!(copier.destination_page, copier.source_page);
        assert_eq!(references.count(shared), Ok(1));
        assert_eq!(references.count(Address(0x30_000)), Ok(1));
        assert_eq!(
            plan.compile().expect("plan").translate(Address(0x8000)),
            Ok((Address(0x30_000), PagePermissions::USER_STACK))
        );
    }

    #[test]
    fn address_space_plan_rolls_back_cow_copy_failure() {
        let shared = Address(0x20_000);
        let mut references = SharedFrameReferences::new();
        references.retain(shared).expect("owner");
        references.retain(shared).expect("second owner");
        let mut allocator = FrameAllocator::new(0x30_000, 1).expect("allocator");
        let mut plan = AddressSpacePlan::new();
        plan.add_cow_page(CowPage::new(Address(0x8000), shared).expect("cow"))
            .expect("mapping");
        let plan_checkpoint = plan;
        let mut copier = FakePageCopier::new(true);

        assert_eq!(
            plan.resolve_cow_write_with_copy(
                &mut references,
                &mut allocator,
                Address(0x8000),
                &mut copier,
            ),
            Err(ExecError::Fault)
        );
        assert_eq!(plan, plan_checkpoint);
        assert_eq!(references.count(shared), Ok(2));
        assert_eq!(
            references.count(Address(0x30_000)),
            Err(ExecError::NotFound)
        );
        assert_eq!(
            allocator.allocate(1).expect("frame restored").base,
            0x30_000
        );
    }

    #[test]
    fn address_space_plan_does_not_copy_a_unique_cow_page() {
        let shared = Address(0x20_000);
        let mut references = SharedFrameReferences::new();
        references.retain(shared).expect("owner");
        let mut allocator = FrameAllocator::new(0x30_000, 1).expect("allocator");
        let mut plan = AddressSpacePlan::new();
        plan.add_cow_page(CowPage::new(Address(0x8000), shared).expect("cow"))
            .expect("mapping");
        let mut copier = FakePageCopier::new(true);

        let resolution = plan
            .resolve_cow_write_with_copy(
                &mut references,
                &mut allocator,
                Address(0x8000),
                &mut copier,
            )
            .expect("private transition");

        assert!(!resolution.copied);
        assert_eq!(resolution.new_physical_address, shared);
        assert_eq!(copier.calls, 0);
        assert_eq!(references.count(shared), Ok(1));
        assert_eq!(
            allocator.allocate(1).expect("no replacement").base,
            0x30_000
        );
    }

    #[test]
    fn cow_page_is_mapped_read_only_in_user_space() {
        let page = CowPage::new(Address(0x6000), Address(0x30_000)).expect("cow page");
        let mut plan = AddressSpacePlan::new();
        plan.add_cow_page(page).expect("mapping");
        assert_eq!(
            plan.compile().expect("plan").translate(Address(0x6001)),
            Ok((Address(0x30_001), PagePermissions::COW_USER))
        );
        assert_eq!(
            CowPage::new(Address(0x6001), Address(0x30_000)),
            Err(ExecError::InvalidArgument)
        );
    }

    #[test]
    fn sv39_page_table_builds_three_level_user_mapping() {
        let mut table = Sv39PageTable::new(Address(0x80_000), Address(0x90_000)).expect("table");
        table
            .map_page(
                Address(0x4000),
                Address(0x20_000),
                PagePermissions::USER_STACK,
            )
            .expect("map");
        assert!(table.root_entry(0).is_some());
        assert_eq!(table.satp().raw() >> 60, SV39_MODE);
        assert_eq!(
            table.map_page(
                Address(0x4000),
                Address(0x24_000),
                PagePermissions::USER_STACK
            ),
            Err(ExecError::InvalidArgument)
        );
    }

    #[test]
    fn sv39_page_table_replaces_existing_cow_leaf() {
        let mut table = Sv39PageTable::new(Address(0x80_000), Address(0x90_000)).expect("table");
        table
            .map_page(
                Address(0x4000),
                Address(0x20_000),
                PagePermissions::COW_USER,
            )
            .expect("cow map");
        table
            .replace_page(
                Address(0x4000),
                Address(0x30_000),
                PagePermissions::USER_STACK,
            )
            .expect("private leaf");

        let l1 = table.child_index(table.root[0]).expect("l1");
        let l0 = table.child_index(table.children[l1][0]).expect("l0");
        assert_eq!(
            table.children[l0][4],
            (0x30_000 >> 12) << 10 | PagePermissions::USER_STACK.pte_flags()
        );
    }

    #[test]
    fn sv39_existing_view_replaces_leaf_without_rebuilding_tables() {
        let mut root = [0; SV39_TABLE_ENTRIES];
        let mut children = [[0; SV39_TABLE_ENTRIES]; SV39_MAX_CHILD_TABLES];
        let mut plan = AddressSpacePlan::new();
        plan.add_cow_page(CowPage::new(Address(0x4000), Address(0x20_000)).expect("cow"))
            .expect("mapping");
        let _initial = plan
            .compile_view(
                Address(0x80_000),
                Address(0x90_000),
                &mut root,
                &mut children,
            )
            .expect("initial table");
        drop(_initial);

        let mut existing = Sv39PageTableView::from_existing(
            Address(0x80_000),
            Address(0x90_000),
            &mut root,
            &mut children,
        )
        .expect("existing table");
        existing
            .replace_page(
                Address(0x4abc),
                Address(0x30_000),
                PagePermissions::USER_STACK,
            )
            .expect("replace leaf");
        drop(existing);

        let l1 = ((root[0] >> 10) * PAGE_SIZE - 0x90_000) / PAGE_SIZE;
        let l0 = ((children[l1 as usize][0] >> 10) * PAGE_SIZE - 0x90_000) / PAGE_SIZE;
        assert_eq!(
            children[l0 as usize][4],
            (0x30_000 >> 12) << 10 | PagePermissions::USER_STACK.pte_flags()
        );
    }

    #[test]
    fn sv39_page_table_accepts_canonical_kernel_mapping() {
        let mut table = Sv39PageTable::new(Address(0x80_000), Address(0x90_000)).expect("table");
        let permissions = PagePermissions {
            read: true,
            write: true,
            execute: true,
            user: false,
        };
        table
            .map_page(Address(0x8020_0000), Address(0x8020_0000), permissions)
            .expect("kernel map");
        assert!(table.root_entry(2).is_some());
        assert_eq!(
            validate_sv39_address(Address(0x4000_0000_0000)),
            Err(ExecError::InvalidArgument)
        );
    }

    #[test]
    fn address_space_plan_composes_kernel_and_user_mappings() {
        let mut plan = AddressSpacePlan::new();
        plan.add(
            MappingPlan::kernel(
                UserPageRange {
                    first_page: 0x8020_0000 / PAGE_SIZE,
                    page_count: 1,
                },
                0x8020_0000,
                PagePermissions {
                    read: true,
                    write: true,
                    execute: true,
                    user: false,
                },
            )
            .expect("kernel mapping"),
        )
        .expect("kernel plan");
        plan.add(
            MappingPlan::new(
                UserPageRange {
                    first_page: 0x4000 / PAGE_SIZE,
                    page_count: 1,
                },
                0x20_000,
                PagePermissions::USER_STACK,
            )
            .expect("user mapping"),
        )
        .expect("user plan");
        assert!(
            plan.compile()
                .expect("compiled")
                .translate(Address(0x8020_0000))
                .is_ok()
        );
    }

    #[test]
    fn address_space_plan_compiles_into_caller_owned_sv39_storage() {
        let mut plan = AddressSpacePlan::new();
        plan.add(
            MappingPlan::new(
                UserPageRange {
                    first_page: 0x4000 / PAGE_SIZE,
                    page_count: 1,
                },
                0x20_000,
                PagePermissions::USER_STACK,
            )
            .expect("mapping"),
        )
        .expect("plan");
        let mut root = [0u64; SV39_TABLE_ENTRIES];
        let mut children = [[0u64; SV39_TABLE_ENTRIES]; SV39_MAX_CHILD_TABLES];
        let table = plan
            .compile_view(
                Address(0x80_000),
                Address(0x90_000),
                &mut root,
                &mut children,
            )
            .expect("in-place tables");
        assert_eq!(table.satp() >> 60, SV39_MODE);
        assert_ne!(root[0], 0);
    }

    #[test]
    fn frame_allocator_allocates_contiguous_frames_and_reuses_freed_space() {
        let mut allocator = FrameAllocator::new(0x8000, 8).expect("allocator");
        let first = allocator.allocate(2).expect("first");
        let second = allocator.allocate(1).expect("second");
        assert_eq!(first.base, 0x8000);
        assert_eq!(second.base, 0xa000);
        allocator.free(first).expect("free");
        assert_eq!(allocator.allocate(2).expect("reuse").base, 0x8000);
        assert_eq!(allocator.free(second), Ok(()));
    }

    #[test]
    fn frame_allocator_uses_multiple_usable_regions_without_crossing_holes() {
        let mut allocator = FrameAllocator::from_ranges(&[(0x8000, 0x2000), (0x20_000, 0x2000)])
            .expect("multi-region allocator");
        assert_eq!(allocator.allocate(2).expect("first").base, 0x8000);
        assert_eq!(allocator.allocate(1).expect("second").base, 0x20_000);
    }

    #[test]
    fn memory_map_rejects_overlap_and_counts_only_usable_frames() {
        let mut map = MemoryMap::new();
        map.add(MemoryRegion::new(0x8000, 0x2000, MemoryRegionKind::Usable).expect("usable"))
            .expect("add");
        map.add(MemoryRegion::new(0xa000, 0x1000, MemoryRegionKind::Reserved).expect("reserved"))
            .expect("add");
        assert_eq!(map.usable_frames(), 2);
        assert_eq!(
            map.add(MemoryRegion::new(0x9000, 0x2000, MemoryRegionKind::Usable).expect("overlap")),
            Err(ExecError::InvalidArgument)
        );
    }

    #[test]
    fn memory_map_can_be_built_from_fdt_cleaned_ranges() {
        let map =
            MemoryMap::from_usable_ranges(&[(0x8000, 0x2000), (0xc000, 0x1000)]).expect("map");
        assert_eq!(map.usable_frames(), 3);
    }

    #[test]
    fn memory_map_excludes_kernel_reserved_interval() {
        let map = MemoryMap::from_usable_ranges_excluding(&[(0x8000, 0x8000)], &[(0xa000, 0x2000)])
            .expect("map");
        assert_eq!(map.usable_ranges().1, 2);
        assert_eq!(map.usable_frames(), 6);
    }
}
