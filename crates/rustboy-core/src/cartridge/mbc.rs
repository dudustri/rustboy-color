//! Cartridges hold more than fits in the address space; these chips swap the pieces in and out.

use super::CartridgeError;

const ROM_BANK: usize = 0x4000; // one ROM bank, the size of the 4000-7FFF window
const RAM_BANK: usize = 0x2000; // one RAM bank, the size of the A000-BFFF window

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mbc {
    /// No chip at all, just a flat 32 KiB. Only `Cartridge::test_ram` sets `writable`.
    None { writable: bool },
    Mbc1 {
        rom_bank: u8,        // low 5 bits of the ROM bank number, never 0
        ram_bank: u8,        // 2 bits: the RAM bank, or the top of the ROM bank number
        ram_enabled: bool,   // save RAM only answers after a game writes 0xA
        advanced_mode: bool, // lets those 2 bits also move bank 0 and the RAM bank
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
    pub fn rom_offset(&self, addr: u16, rom_len: usize) -> usize {
        match *self {
            Self::None { .. } => addr as usize,
            Self::Mbc1 {
                rom_bank,
                ram_bank,
                advanced_mode,
                ..
            } => {
                let upper = (ram_bank as usize) << 5;
                let bank = match addr {
                    0x0000..=0x3FFF if advanced_mode => upper,
                    0x0000..=0x3FFF => 0,
                    _ => upper | rom_bank as usize,
                };
                // A bank past the end of the ROM wraps back round to the start.
                let banks = (rom_len / ROM_BANK).max(1);
                (bank % banks) * ROM_BANK + (addr as usize & 0x3FFF)
            }
            // TODO(PR-20): MBC3 and MBC5 banking.
            _ => todo!("PR-20: ROM banking"),
        }
    }

    /// The same for save RAM, or `None` while the game has it switched off.
    pub fn ram_offset(&self, addr: u16, ram_len: usize) -> Option<usize> {
        let offset = match *self {
            Self::None { .. } => addr as usize - 0xA000,
            Self::Mbc1 {
                ram_bank,
                ram_enabled,
                advanced_mode,
                ..
            } => {
                if !ram_enabled {
                    return None;
                }
                let bank = if advanced_mode { ram_bank as usize } else { 0 };
                bank * RAM_BANK + (addr as usize - 0xA000)
            }
            _ => todo!("PR-20: RAM banking"),
        };
        Some(if ram_len > 0 {
            offset % ram_len
        } else {
            offset
        })
    }

    /// Writing to ROM stores nothing: it is how a game gives this chip orders.
    pub fn write_control(&mut self, addr: u16, value: u8) {
        match self {
            Self::None { .. } => {}
            Self::Mbc1 {
                rom_bank,
                ram_bank,
                ram_enabled,
                advanced_mode,
            } => match addr {
                0x0000..=0x1FFF => *ram_enabled = value & 0x0F == 0x0A,
                // Asking for bank 0 here gives bank 1, since bank 0 already has its own window.
                0x2000..=0x3FFF => *rom_bank = (value & 0x1F).max(1),
                0x4000..=0x5FFF => *ram_bank = value & 0x03,
                _ => *advanced_mode = value & 0x01 != 0,
            },
            _ => todo!("PR-20: mapper registers"),
        }
    }
}
