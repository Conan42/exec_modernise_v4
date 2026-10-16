#![no_std]

//! Contrats binaires minimaux partagés par l'outil hôte et le futur firmware.
//!
//! Le diagnostic UART de boot reste du texte brut. Ces trames servent ensuite
//! aux commandes explicites, une fois le firmware V4 chargé par le bootloader.

pub const FRAME_MAGIC: u16 = 0x5634;
pub const PROTOCOL_VERSION: u8 = 1;
pub const MAX_PAYLOAD: usize = 256;

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Command {
    Ping = 1,
    ReadIdentity = 2,
    Reboot = 3,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameHeader {
    pub magic: u16,
    pub version: u8,
    pub command: u8,
    pub payload_len: u16,
    pub sequence: u16,
}

impl FrameHeader {
    pub const fn new(command: Command, payload_len: u16, sequence: u16) -> Self {
        Self {
            magic: FRAME_MAGIC,
            version: PROTOCOL_VERSION,
            command: command as u8,
            payload_len,
            sequence,
        }
    }
}
