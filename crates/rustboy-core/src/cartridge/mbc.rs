//! Cartridges hold more than fits in the address space; these chips swap the pieces in and out.

use super::CartridgeError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mbc {
    /// No chip at all, just a flat 32 KiB. Only `Cartridge::test_ram` sets `writable`.
    None { writable: bool },
    Mbc1 {
        rom_bank: u8,
        ram_bank: u8,
        ram_enabled: bool,
        advanced_mode: bool,
    },
    Mbc3 {
        rom_bank: u8,
        ram_bank: u8,
        ram_enabled: bool,
    },
    Mbc5 {
        rom_bank: u16,
        ram_bank: u8,
        ram_enabled: bool,
    },
}

impl Mbc {
    pub fn from_cartridge_type(code: u8) -> Result<Self, CartridgeError> {
        match code {
            0x00 | 0x08 | 0x09 => Ok(Self::None { writable: false }),
            0x01..=0x03 => Ok(Self::Mbc1 {
                rom_bank: 1,
                ram_bank: 0,
                ram_enabled: false,
                advanced_mode: false,
            }),
            0x0F..=0x13 => Ok(Self::Mbc3 {
                rom_bank: 1,
                ram_bank: 0,
                ram_enabled: false,
            }),
            0x19..=0x1E => Ok(Self::Mbc5 {
                rom_bank: 1,
                ram_bank: 0,
                ram_enabled: false,
            }),
            other => Err(CartridgeError::UnsupportedMapper(other)),
        }
    }

    /// Turns an address the CPU asked for into a position in the ROM file.
    pub fn rom_offset(&self, addr: u16) -> usize {
        match self {
            Self::None { .. } => addr as usize,
            // TODO(PR-20): wrap the bank number around the cartridge's real size.
            _ => todo!("PR-20: ROM banking"),
        }
    }

    /// The same for save RAM, or `None` while the game has it switched off.
    pub fn ram_offset(&self, addr: u16) -> Option<usize> {
        match self {
            Self::None { .. } => Some((addr - 0xA000) as usize),
            _ => todo!("PR-20: RAM banking"),
        }
    }

    /// Writing to ROM stores nothing: it is how a game gives this chip orders.
    pub fn write_control(&mut self, _addr: u16, _value: u8) {
        match self {
            Self::None { .. } => {}
            _ => todo!("PR-20: mapper registers"),
        }
    }
}
