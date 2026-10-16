//! Petit outil hôte de construction d'une image MFFS V1.
//!
//! Il ne fait pas partie du chemin U-Mode : il valide un ELF d'application,
//! puis écrit un catalogue MFFS minimal à une entrée. Le lecteur embarqué
//! reste entièrement dans `filesystem-v4` et ne dépend pas de cet outil.

extern crate std;

use filesystem_v4::{
    FILE_TYPE_APP, MFFS_ENTRY_SIZE, MFFS_SIGNATURE, MFFS_VERSION, MffsImage, parse_elf64,
};
use std::env;
use std::fs;
use std::process;

const HEADER_SIZE: usize = 32;
const BLOCK_SIZE: usize = 512;

fn fnv1a(bytes: &[u8]) -> u32 {
    let mut hash = 0x811c9dc5u32;
    for byte in bytes {
        hash = (hash ^ u32::from(*byte)).wrapping_mul(0x0100_0193);
    }
    hash
}

fn put16(data: &mut [u8], offset: usize, value: u16) {
    data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put32(data: &mut [u8], offset: usize, value: u32) {
    data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn fail(message: &str) -> ! {
    eprintln!("mffs-pack: {message}");
    process::exit(2)
}

fn main() {
    let arguments: std::vec::Vec<String> = env::args().collect();
    if arguments.len() != 3 {
        fail("usage: mffs-pack <ELF> <MFFS>");
    }

    let input = fs::read(&arguments[1]).unwrap_or_else(|_| fail("cannot read ELF"));
    parse_elf64(&input).unwrap_or_else(|_| fail("input is not a valid V3 RISC-V APP ELF"));

    let name = b"UILab.app\0";
    let catalog_offset = HEADER_SIZE;
    let payload_offset = catalog_offset + MFFS_ENTRY_SIZE;
    let image_size = payload_offset
        .checked_add(input.len())
        .unwrap_or_else(|| fail("image is too large"));
    let image_size_u32 = u32::try_from(image_size).unwrap_or_else(|_| fail("image is too large"));
    let disk_size = image_size
        .checked_add(BLOCK_SIZE - 1)
        .map(|size| size / BLOCK_SIZE * BLOCK_SIZE)
        .unwrap_or_else(|| fail("image is too large"));

    // Le champ MFFS image_size reste la taille logique exacte. Le fichier
    // physique est, lui, complété à un secteur afin que le backend bloc ne
    // puisse jamais exposer une image dont le dernier secteur est tronqué.
    let mut image = std::vec![0u8; disk_size];
    image[..4].copy_from_slice(MFFS_SIGNATURE);
    put16(&mut image, 4, MFFS_VERSION);
    put16(&mut image, 6, 512);
    put16(&mut image, 8, 1);
    put16(&mut image, 10, MFFS_ENTRY_SIZE as u16);
    put32(&mut image, 12, catalog_offset as u32);
    put32(&mut image, 20, image_size_u32);

    let entry = catalog_offset;
    image[entry..entry + name.len()].copy_from_slice(name);
    image[entry + 32] = FILE_TYPE_APP;
    put16(&mut image, entry + 34, 0o555);
    put32(&mut image, entry + 44, payload_offset as u32);
    put32(
        &mut image,
        entry + 48,
        u32::try_from(input.len()).unwrap_or_else(|_| fail("ELF is too large")),
    );
    put16(&mut image, entry + 52, 1);
    put32(&mut image, entry + 56, fnv1a(&input));
    image[payload_offset..payload_offset + input.len()].copy_from_slice(&input);
    MffsImage::parse(&image).unwrap_or_else(|_| fail("generated MFFS failed validation"));

    fs::write(&arguments[2], image).unwrap_or_else(|_| fail("cannot write MFFS"));
}
