//! Memory bank controllers. See `docs/architecture.md` 2.2.

use super::CartridgeError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mbc {
    /// No mapper: 32 KiB flat. `writable` is only set by `Cartridge::test_ram`.
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

    /// Map a CPU address in 0000-7FFF onto an offset into the ROM image.
    pub fn rom_offset(&self, addr: u16) -> usize {
        match self {
            Self::None { .. } => addr as usize,
            // TODO(PR-20): bank number masking against the real ROM size.
            _ => todo!("PR-20: ROM banking"),
        }
    }

    /// Map A000-BFFF onto external RAM, or `None` when RAM is disabled.
    pub fn ram_offset(&self, addr: u16) -> Option<usize> {
        match self {
            Self::None { .. } => Some((addr - 0xA000) as usize),
            _ => todo!("PR-20: RAM banking"),
        }
    }

    /// Writes into ROM space are mapper commands, not memory writes.
    pub fn write_control(&mut self, _addr: u16, _value: u8) {
        match self {
            Self::None { .. } => {}
            _ => todo!("PR-20: mapper registers"),
        }
    }
}
