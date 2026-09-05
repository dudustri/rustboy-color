//! The block at 0100-014F where every cartridge describes itself.

use super::CartridgeError;

const TITLE: core::ops::Range<usize> = 0x0134..0x0144;
const CGB_FLAG: usize = 0x0143;
const CARTRIDGE_TYPE: usize = 0x0147;
const ROM_SIZE: usize = 0x0148;
const RAM_SIZE: usize = 0x0149;
const HEADER_END: usize = 0x0150;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CgbFlag {
    /// An original Game Boy game; a Color runs it in compatibility mode.
    None,
    /// Uses Color features, but still works on an original Game Boy.
    Enhanced,
    /// Color only; it refuses to run on an original Game Boy.
    Only,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub title: String,
    pub cgb: CgbFlag,
    pub cartridge_type: u8,
    pub rom_size: usize,
    pub ram_size: usize,
}

impl Header {
    pub fn parse(rom: &[u8]) -> Result<Self, CartridgeError> {
        if rom.len() < HEADER_END {
            return Err(CartridgeError::TooSmall(rom.len()));
        }

        // The Color flag sits on the title's last byte, so Color titles are one character shorter.
        let cgb = match rom[CGB_FLAG] {
            0x80 => CgbFlag::Enhanced,
            0xC0 => CgbFlag::Only,
            _ => CgbFlag::None,
        };
        let title_end = if cgb == CgbFlag::None {
            TITLE.end
        } else {
            CGB_FLAG
        };
        let title = rom[TITLE.start..title_end]
            .iter()
            .take_while(|&&byte| byte != 0)
            .map(|&byte| byte as char)
            .filter(|c| c.is_ascii_graphic() || *c == ' ')
            .collect::<String>()
            .trim_end()
            .to_string();

        Ok(Self {
            title,
            cgb,
            cartridge_type: rom[CARTRIDGE_TYPE],
            rom_size: 0x8000 << rom[ROM_SIZE].min(8),
            ram_size: match rom[RAM_SIZE] {
                0x02 => 0x2000,
                0x03 => 0x8000,
                0x04 => 0x20000,
                0x05 => 0x10000,
                _ => 0,
            },
        })
    }
}
