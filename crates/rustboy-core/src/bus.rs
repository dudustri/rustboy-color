//! Sends every address to the right chip. The full map is in `docs/architecture.md` section 2.

use crate::apu::Apu;
use crate::cartridge::Cartridge;
use crate::joypad::Joypad;
use crate::ppu::Ppu;
use crate::serial::Serial;
use crate::timer::Timer;

pub const IF_VBLANK: u8 = 1 << 0;
pub const IF_STAT: u8 = 1 << 1;
pub const IF_TIMER: u8 = 1 << 2;
pub const IF_SERIAL: u8 = 1 << 3;
pub const IF_JOYPAD: u8 = 1 << 4;

const WRAM_BANK_SIZE: usize = 0x1000;
const WRAM_BANKS: usize = 8;
const HRAM_SIZE: usize = 0x7F;

pub struct Bus {
    pub cartridge: Option<Cartridge>, // the game, or None when nothing is plugged in
    pub ppu: Ppu,                     // the screen
    pub apu: Apu,                     // the sound chip
    pub timer: Timer,                 // the counters a game uses to measure time
    pub joypad: Joypad,               // the buttons
    pub serial: Serial,               // the link cable port
    pub double_speed: bool,           // true while the CGB runs its CPU twice as fast
    wram: Vec<u8>,                    // 8 banks of work RAM, seen at C000-DFFF
    hram: [u8; HRAM_SIZE],            // 127 fast bytes at FF80-FFFE, usable during DMA
    t_cycles: u64,                    // ticks since power on

    // registers that belong to no single chip
    pub interrupt_flag: u8, // FF0F which interrupts are waiting to be handled
    pub interrupt_enable: u8, // FFFF which interrupts the game allows
    svbk: u8,               // FF70 which work RAM bank sits at D000-DFFF
    key1: u8,               // FF4D ask for double speed, and report whether it is on
}

impl Bus {
    pub fn new(cartridge: Option<Cartridge>) -> Self {
        Self {
            cartridge,
            ppu: Ppu::new(),
            apu: Apu::new(),
            timer: Timer::new(),
            joypad: Joypad::new(),
            serial: Serial::new(),
            interrupt_flag: 0,
            interrupt_enable: 0,
            double_speed: false,
            wram: vec![0; WRAM_BANK_SIZE * WRAM_BANKS],
            hram: [0; HRAM_SIZE],
            svbk: 1,
            key1: 0,
            t_cycles: 0,
        }
    }

    /// A fake machine you can write anywhere in, so opcode tests can place code freely.
    pub fn testing() -> Self {
        Self::new(Some(Cartridge::test_ram()))
    }

    /// Ticks counted since the console was switched on.
    pub fn cycles(&self) -> u64 {
        self.t_cycles
    }

    /// Moves every chip forward. The CPU calls this once per M-cycle.
    pub fn tick(&mut self, t_cycles: u32) {
        self.t_cycles += t_cycles as u64;

        // In double speed only the CPU runs faster; screen and sound keep the normal rate.
        let base = if self.double_speed {
            t_cycles / 2
        } else {
            t_cycles
        };
        self.interrupt_flag |= self.ppu.tick(base);
        self.apu.tick(base);

        self.interrupt_flag |= self.timer.tick(t_cycles);
        self.interrupt_flag |= self.serial.tick(t_cycles);
        self.interrupt_flag |= self.joypad.take_interrupt();
    }

    pub fn request_interrupt(&mut self, mask: u8) {
        self.interrupt_flag |= mask;
    }

    fn wram_offset(&self) -> usize {
        let bank = (self.svbk & 0x07).max(1) as usize;
        bank * WRAM_BANK_SIZE
    }

    pub fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x7FFF => self.cartridge.as_ref().map_or(0xFF, |c| c.read_rom(addr)),
            0x8000..=0x9FFF => self.ppu.read_vram(addr),
            0xA000..=0xBFFF => self.cartridge.as_ref().map_or(0xFF, |c| c.read_ram(addr)),
            0xC000..=0xCFFF => self.wram[(addr - 0xC000) as usize],
            0xD000..=0xDFFF => self.wram[self.wram_offset() + (addr - 0xD000) as usize],
            0xE000..=0xFDFF => self.read(addr - 0x2000),
            0xFE00..=0xFE9F => self.ppu.read_oam(addr),
            0xFEA0..=0xFEFF => 0xFF,
            0xFF00..=0xFF7F => self.read_io(addr),
            0xFF80..=0xFFFE => self.hram[(addr - 0xFF80) as usize],
            0xFFFF => self.interrupt_enable,
        }
    }

    pub fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x7FFF => {
                if let Some(cart) = self.cartridge.as_mut() {
                    cart.write_rom(addr, value);
                }
            }
            0x8000..=0x9FFF => self.ppu.write_vram(addr, value),
            0xA000..=0xBFFF => {
                if let Some(cart) = self.cartridge.as_mut() {
                    cart.write_ram(addr, value);
                }
            }
            0xC000..=0xCFFF => self.wram[(addr - 0xC000) as usize] = value,
            0xD000..=0xDFFF => {
                let offset = self.wram_offset();
                self.wram[offset + (addr - 0xD000) as usize] = value;
            }
            0xE000..=0xFDFF => self.write(addr - 0x2000, value),
            0xFE00..=0xFE9F => self.ppu.write_oam(addr, value),
            0xFEA0..=0xFEFF => {}
            0xFF00..=0xFF7F => self.write_io(addr, value),
            0xFF80..=0xFFFE => self.hram[(addr - 0xFF80) as usize] = value,
            0xFFFF => self.interrupt_enable = value,
        }
    }

    fn read_io(&mut self, addr: u16) -> u8 {
        match addr {
            0xFF00 => self.joypad.read(),
            0xFF01..=0xFF02 => self.serial.read(addr),
            0xFF04..=0xFF07 => self.timer.read(addr),
            0xFF0F => self.interrupt_flag | 0xE0,
            0xFF10..=0xFF3F => self.apu.read(addr),
            0xFF40..=0xFF4B => self.ppu.read_register(addr),
            0xFF4D => self.key1 | if self.double_speed { 0x80 } else { 0x00 },
            0xFF4F => self.ppu.read_register(addr),
            0xFF51..=0xFF55 => 0xFF, // TODO(PR-18): HDMA
            0xFF68..=0xFF6B => self.ppu.read_register(addr),
            0xFF70 => self.svbk | 0xF8,
            _ => 0xFF,
        }
    }

    fn write_io(&mut self, addr: u16, value: u8) {
        match addr {
            0xFF00 => self.joypad.write(value),
            0xFF01..=0xFF02 => self.serial.write(addr, value),
            0xFF04..=0xFF07 => self.timer.write(addr, value),
            0xFF0F => self.interrupt_flag = value & 0x1F,
            0xFF10..=0xFF3F => self.apu.write(addr, value),
            0xFF46 => {} // TODO(PR-18): OAM DMA
            0xFF40..=0xFF4B => self.ppu.write_register(addr, value),
            0xFF4D => self.key1 = value & 0x01,
            0xFF4F => self.ppu.write_register(addr, value),
            0xFF51..=0xFF55 => {} // TODO(PR-18): HDMA
            0xFF68..=0xFF6B => self.ppu.write_register(addr, value),
            0xFF70 => self.svbk = value & 0x07,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echo_ram_mirrors_work_ram() {
        let mut bus = Bus::testing();
        bus.write(0xC123, 0x42);
        assert_eq!(bus.read(0xE123), 0x42);
    }

    #[test]
    fn unusable_region_reads_back_ff() {
        let mut bus = Bus::testing();
        bus.write(0xFEA0, 0x42);
        assert_eq!(bus.read(0xFEA0), 0xFF);
    }

    #[test]
    fn hram_and_ie_are_distinct() {
        let mut bus = Bus::testing();
        bus.write(0xFFFE, 0x11);
        bus.write(0xFFFF, 0x22);
        assert_eq!(bus.read(0xFFFE), 0x11);
        assert_eq!(bus.read(0xFFFF), 0x22);
    }
}
