#![no_std]
#![no_main]

//! Image minimale destinée au premier essai TFTP.
//!
//! Le point d'entrée matériel et le script de liaison seront ajoutés avec le
//! profil de boot validé sur la carte. Tant que l'adresse DRAM et la séquence
//! de transfert du bootloader ne sont pas figées, cette crate reste exclue
//! des builds hôte par défaut.

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    loop {
        core::hint::spin_loop();
    }
}
