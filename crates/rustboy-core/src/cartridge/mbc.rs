//! cartridges hold more than fits in address space; these chips swap pieces in and out.

use super::CartridgeError;

const ROM_BANK: usize = 0x4000; // one ROM bank, size of 4000-7FFF window
const RAM_BANK: usize = 0x2000; // one RAM bank, size of A000-BFFF window

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mbc {
    /// no chip at all, just a flat 32 KiB. Only `Cartridge::test_ram` sets `writable`.
    None { writable: bool },
    Mbc1 {
        rom_bank: u8,        // low 5 bits of ROM bank number, never 0
        ram_bank: u8,        // 2 bits: RAM bank, or top of ROM bank number
        ram_enabled: bool,   // save RAM only answers after a game writes 0xA
        advanced_mode: bool, // lets those 2 bits also move bank 0 and RAM bank
    },
    Mbc3 {
        rom_bank: u8,      // 7 bits, so 127 banks, never 0
        ram_bank: u8,      // 0 to 3 pick save RAM, 8 to 12 pick the clock
        ram_enabled: bool, // save RAM and clock share one switch
    },
    Mbc5 {
        rom_bank: u16,     // 9 bits, so 512 banks, and 0 is allowed here
        ram_bank: u8,      // 0 to 15
        ram_enabled: bool, // same switch as the chips before it
    },
}

/// where a bank starts in the file, wrapping round for a bank past the end.
fn in_bank(bank: usize, addr: u16, rom_len: usize) -> usize {
    let banks = (rom_len / ROM_BANK).max(1);
    (bank % banks) * ROM_BANK + (addr as usize & 0x3FFF)
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

    /// turns an address CPU asked for into a position in ROM file.
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
                in_bank(bank, addr, rom_len)
            }
            Self::Mbc3 { rom_bank, .. } => {
                let bank = if addr < 0x4000 { 0 } else { rom_bank as usize };
                in_bank(bank, addr, rom_len)
            }
            Self::Mbc5 { rom_bank, .. } => {
                let bank = if addr < 0x4000 { 0 } else { rom_bank as usize };
                in_bank(bank, addr, rom_len)
            }
        }
    }

    /// same for save RAM, or `None` while game has it switched off.
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
            Self::Mbc3 {
                ram_bank,
                ram_enabled,
                ..
            } => {
                // TODO(PR-20 stage 2): banks 8 to 12 are the clock, not save RAM
                if !ram_enabled || ram_bank > 3 {
                    return None;
                }
                ram_bank as usize * RAM_BANK + (addr as usize - 0xA000)
            }
            Self::Mbc5 {
                ram_bank,
                ram_enabled,
                ..
            } => {
                if !ram_enabled {
                    return None;
                }
                ram_bank as usize * RAM_BANK + (addr as usize - 0xA000)
            }
        };
        Some(if ram_len > 0 {
            offset % ram_len
        } else {
            offset
        })
    }

    /// writing to ROM stores nothing: it is how a game gives this chip orders.
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
                // asking for bank 0 here gives bank 1, since bank 0 already has its own window.
                0x2000..=0x3FFF => *rom_bank = (value & 0x1F).max(1),
                0x4000..=0x5FFF => *ram_bank = value & 0x03,
                _ => *advanced_mode = value & 0x01 != 0,
            },
            Self::Mbc3 {
                rom_bank,
                ram_bank,
                ram_enabled,
            } => match addr {
                0x0000..=0x1FFF => *ram_enabled = value & 0x0F == 0x0A,
                // asking for bank 0 here gives bank 1, since bank 0 already has its own window.
                0x2000..=0x3FFF => *rom_bank = (value & 0x7F).max(1),
                0x4000..=0x5FFF => *ram_bank = value & 0x0F,
                _ => {} // TODO(PR-20 stage 2): this is where a game freezes the clock to read it
            },
            Self::Mbc5 {
                rom_bank,
                ram_bank,
                ram_enabled,
            } => match addr {
                0x0000..=0x1FFF => *ram_enabled = value & 0x0F == 0x0A,
                // this chip takes its bank number in two halves, and bank 0 is allowed
                0x2000..=0x2FFF => *rom_bank = (*rom_bank & 0x100) | value as u16,
                0x3000..=0x3FFF => *rom_bank = (*rom_bank & 0x0FF) | ((value as u16 & 1) << 8),
                0x4000..=0x5FFF => *ram_bank = value & 0x0F,
                _ => {}
            },
        }
    }
}
