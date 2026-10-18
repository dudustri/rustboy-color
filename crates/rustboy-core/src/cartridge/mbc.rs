//! cartridges hold more than fits in address space; these chips swap pieces in and out.

use super::CartridgeError;
use crate::T_CYCLES_PER_SECOND;

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
        clock: Clock,      // only MBC3 carries one
    },
    Mbc5 {
        rom_bank: u16,     // 9 bits, so 512 banks, and 0 is allowed here
        ram_bank: u8,      // 0 to 15
        ram_enabled: bool, // same switch as the chips before it
    },
}

/// a moment in time, as this chip counts it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Time {
    seconds: u8,
    minutes: u8,
    hours: u8,
    days: u16,        // 9 bits, so it runs out after 511 days
    halted: bool,     // a game can stop it to set it
    overflowed: bool, // stays on once days have run out, until game clears it
}

impl Time {
    // one more second, carrying up through minutes, hours and days
    fn advance(&mut self) {
        if self.halted {
            return;
        }
        self.seconds += 1;
        if self.seconds < 60 {
            return;
        }
        self.seconds = 0;
        self.minutes += 1;
        if self.minutes < 60 {
            return;
        }
        self.minutes = 0;
        self.hours += 1;
        if self.hours < 24 {
            return;
        }
        self.hours = 0;
        self.days += 1;
        if self.days > 0x1FF {
            self.days = 0;
            self.overflowed = true;
        }
    }
}

/// clock inside an MBC3 cartridge, which a game reads through save RAM window.
///
/// counts emulated time, so a test can wind it forward and a browser needs no real clock.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Clock {
    now: Time,         // keeps counting
    shown: Time,       // frozen copy a game actually reads
    ticks: u32,        // emulated ticks since last whole second
    latch_armed: bool, // a zero then a one takes a fresh copy
}

impl Clock {
    fn tick(&mut self, t_cycles: u32) {
        self.ticks += t_cycles;
        while self.ticks >= T_CYCLES_PER_SECOND {
            self.ticks -= T_CYCLES_PER_SECOND;
            self.now.advance();
        }
    }

    // reading straight off a running clock could catch it mid change, so a game freezes a copy
    fn latch(&mut self, value: u8) {
        if self.latch_armed && value == 1 {
            self.shown = self.now;
        }
        self.latch_armed = value == 0;
    }

    fn read(&self, register: u8) -> u8 {
        match register {
            0x08 => self.shown.seconds,
            0x09 => self.shown.minutes,
            0x0A => self.shown.hours,
            0x0B => self.shown.days as u8,
            _ => {
                (self.shown.days >> 8) as u8
                    | if self.shown.halted { 0x40 } else { 0 }
                    | if self.shown.overflowed { 0x80 } else { 0 }
            }
        }
    }

    fn write(&mut self, register: u8, value: u8) {
        match register {
            0x08 => {
                self.now.seconds = value % 60;
                self.ticks = 0; // setting seconds starts next one from scratch
            }
            0x09 => self.now.minutes = value % 60,
            0x0A => self.now.hours = value % 24,
            0x0B => self.now.days = (self.now.days & 0x100) | value as u16,
            _ => {
                self.now.days = (self.now.days & 0xFF) | ((value as u16 & 1) << 8);
                self.now.halted = value & 0x40 != 0;
                self.now.overflowed = value & 0x80 != 0;
            }
        }
        self.shown = self.now;
    }
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
                clock: Clock::default(),
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

    /// keeps a cartridge clock running. Chips without one ignore it.
    pub fn tick(&mut self, t_cycles: u32) {
        if let Self::Mbc3 { clock, .. } = self {
            clock.tick(t_cycles);
        }
    }

    /// which clock register save RAM window is showing, if it is showing one at all.
    pub fn clock_register(&self) -> Option<u8> {
        match *self {
            Self::Mbc3 {
                ram_bank,
                ram_enabled: true,
                ..
            } if (0x08..=0x0C).contains(&ram_bank) => Some(ram_bank),
            _ => None,
        }
    }

    pub fn read_clock(&self, register: u8) -> u8 {
        match self {
            Self::Mbc3 { clock, .. } => clock.read(register),
            _ => 0xFF,
        }
    }

    pub fn write_clock(&mut self, register: u8, value: u8) {
        if let Self::Mbc3 { clock, .. } = self {
            clock.write(register, value);
        }
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
                clock,
            } => match addr {
                0x0000..=0x1FFF => *ram_enabled = value & 0x0F == 0x0A,
                // asking for bank 0 here gives bank 1, since bank 0 already has its own window.
                0x2000..=0x3FFF => *rom_bank = (value & 0x7F).max(1),
                0x4000..=0x5FFF => *ram_bank = value & 0x0F,
                _ => clock.latch(value),
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
