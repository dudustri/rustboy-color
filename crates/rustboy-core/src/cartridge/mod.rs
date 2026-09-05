//! The game cartridge: its ROM, its save RAM, and the chip that swaps banks between them.

mod header;
mod mbc;

pub use header::{CgbFlag, Header};
pub use mbc::Mbc;

use core::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CartridgeError {
    /// The file is too short to even hold a cartridge header.
    TooSmall(usize),
    UnsupportedMapper(u8),
}

impl fmt::Display for CartridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooSmall(len) => write!(f, "ROM is {len} bytes, too small to hold a header"),
            Self::UnsupportedMapper(code) => write!(f, "unsupported cartridge type {code:#04X}"),
        }
    }
}

impl std::error::Error for CartridgeError {}

pub struct Cartridge {
    pub header: Header, // what the game says about itself
    rom: Vec<u8>,       // the whole game file
    ram: Vec<u8>,       // save data, kept alive by a battery in the cart
    mbc: Mbc,           // the chip that swaps banks in and out
}

impl fmt::Debug for Cartridge {
    /// Written by hand on purpose: deriving it would dump the entire ROM into panic messages.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Cartridge")
            .field("title", &self.header.title)
            .field("mbc", &self.mbc)
            .field("rom_len", &self.rom.len())
            .field("ram_len", &self.ram.len())
            .finish()
    }
}

impl Cartridge {
    pub fn new(rom: Vec<u8>) -> Result<Self, CartridgeError> {
        let header = Header::parse(&rom)?;
        let mbc = Mbc::from_cartridge_type(header.cartridge_type)?;
        let ram = vec![0; header.ram_size];
        Ok(Self {
            header,
            rom,
            ram,
            mbc,
        })
    }

    /// A fake 32 KiB cartridge we can write to, so tests can place opcodes anywhere.
    pub fn test_ram() -> Self {
        Self {
            header: Header {
                title: "TEST".to_string(),
                cgb: CgbFlag::Only,
                cartridge_type: 0x00,
                rom_size: 0x8000,
                ram_size: 0x2000,
            },
            rom: vec![0xFF; 0x8000],
            ram: vec![0; 0x2000],
            mbc: Mbc::None { writable: true },
        }
    }

    pub fn read_rom(&self, addr: u16) -> u8 {
        self.rom
            .get(self.mbc.rom_offset(addr))
            .copied()
            .unwrap_or(0xFF)
    }

    pub fn write_rom(&mut self, addr: u16, value: u8) {
        if let Mbc::None { writable: true } = self.mbc {
            let offset = addr as usize;
            if offset < self.rom.len() {
                self.rom[offset] = value;
            }
            return;
        }
        self.mbc.write_control(addr, value);
    }

    pub fn read_ram(&self, addr: u16) -> u8 {
        match self.mbc.ram_offset(addr) {
            Some(offset) => self.ram.get(offset).copied().unwrap_or(0xFF),
            None => 0xFF,
        }
    }

    pub fn write_ram(&mut self, addr: u16, value: u8) {
        if let Some(offset) = self.mbc.ram_offset(addr)
            && offset < self.ram.len()
        {
            self.ram[offset] = value;
        }
    }

    /// The save data, for the host to keep on disk.
    pub fn ram(&self) -> &[u8] {
        &self.ram
    }

    pub fn load_ram(&mut self, data: &[u8]) {
        let len = data.len().min(self.ram.len());
        self.ram[..len].copy_from_slice(&data[..len]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_rom_shorter_than_its_header() {
        assert_eq!(
            Cartridge::new(vec![0; 16]).unwrap_err(),
            CartridgeError::TooSmall(16)
        );
    }

    #[test]
    fn rejects_an_unknown_mapper() {
        let mut rom = vec![0; 0x8000];
        rom[0x0147] = 0xFE;
        assert_eq!(
            Cartridge::new(rom).unwrap_err(),
            CartridgeError::UnsupportedMapper(0xFE)
        );
    }

    #[test]
    fn reads_the_title() {
        let mut rom = vec![0; 0x8000];
        rom[0x0134..0x0139].copy_from_slice(b"ZELDA");
        let cart = Cartridge::new(rom).unwrap();
        assert_eq!(cart.header.title, "ZELDA");
    }
}
