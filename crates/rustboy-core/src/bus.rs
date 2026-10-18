//! sends every address to right chip. full map is in `docs/architecture.md` section 2.

use crate::apu::Apu;
use crate::cartridge::{Cartridge, CgbFlag};
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
const OAM_BYTES: u8 = 160; // 40 sprites, four bytes each

pub struct Bus {
    pub cartridge: Option<Cartridge>, // game, or None when nothing is plugged in
    pub ppu: Ppu,                     // screen
    pub apu: Apu,                     // sound chip
    pub timer: Timer,                 // counters a game uses to measure time
    pub joypad: Joypad,               // buttons
    pub serial: Serial,               // link cable port
    pub double_speed: bool,           // true while CGB runs its CPU twice as fast
    wram: Vec<u8>,                    // 8 banks of work RAM, seen at C000-DFFF
    hram: [u8; HRAM_SIZE],            // 127 bytes of high RAM at FF80-FFFE
    t_cycles: u64,                    // ticks since power on

    // registers CPU chip keeps for itself: interrupts, RAM banking, speed
    pub interrupt_flag: u8, // FF0F which interrupts are waiting to be handled
    pub interrupt_enable: u8, // FFFF which interrupts game allows
    svbk: u8,               // FF70 which work RAM bank sits at D000-DFFF
    dma: u8,                // FF46 last block asked for, kept only so reads give it back
    dma_from: u16,          // where next byte of that block comes from
    dma_left: u8,           // bytes still to copy, zero when nothing is going on
    dma_wait: u32,          // ticks before next byte moves
    key1: u8,               // FF4D ask for double speed, and report whether it is on
}

impl Bus {
    pub fn new(cartridge: Option<Cartridge>) -> Self {
        let mut ppu = Ppu::new();
        // a colour game says so in its header, and only then are palettes real colours
        ppu.cgb = cartridge
            .as_ref()
            .is_some_and(|c| c.header.cgb != CgbFlag::None);
        Self {
            cartridge,
            ppu,
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
            dma: 0xFF,
            dma_from: 0,
            dma_left: 0,
            dma_wait: 0,
            t_cycles: 0,
        }
    }

    /// a fake machine you can write anywhere in, so opcode tests can place code freely.
    pub fn testing() -> Self {
        Self::new(Some(Cartridge::test_ram()))
    }

    /// ticks counted since console was switched on.
    pub fn cycles(&self) -> u64 {
        self.t_cycles
    }

    /// moves every chip forward. CPU calls this once per M-cycle.
    pub fn tick(&mut self, t_cycles: u32) {
        self.t_cycles += t_cycles as u64;

        // in double speed only CPU runs faster; screen and sound keep normal rate.
        let base = if self.double_speed {
            t_cycles / 2
        } else {
            t_cycles
        };
        self.step_dma(base);
        self.interrupt_flag |= self.ppu.tick(base);
        self.apu.tick(base);

        self.interrupt_flag |= self.timer.tick(t_cycles);
        self.interrupt_flag |= self.serial.tick(t_cycles);
        self.interrupt_flag |= self.joypad.take_interrupt();
    }

    /// whether a game has asked, through KEY1, for next STOP to change speed.
    pub fn speed_switch_armed(&self) -> bool {
        self.key1 & 0x01 != 0
    }

    /// flip between normal and double speed, and clear request.
    pub fn switch_speed(&mut self) {
        self.double_speed = !self.double_speed;
        self.key1 = 0;
    }

    pub fn request_interrupt(&mut self, mask: u8) {
        self.interrupt_flag |= mask;
    }

    fn wram_offset(&self) -> usize {
        let bank = (self.svbk & 0x07).max(1) as usize;
        bank * WRAM_BANK_SIZE
    }

    /// 160 bytes into sprite table, one every machine cycle.
    fn step_dma(&mut self, t_cycles: u32) {
        let mut ticks = t_cycles;
        while self.dma_left > 0 && ticks > 0 {
            let step = ticks.min(self.dma_wait);
            self.dma_wait -= step;
            ticks -= step;
            if self.dma_wait == 0 {
                let byte = self.read_anywhere(self.dma_from);
                self.ppu.force_oam(OAM_BYTES - self.dma_left, byte);
                self.dma_from += 1;
                self.dma_left -= 1;
                self.dma_wait = 4;
            }
        }
    }

    pub fn read(&mut self, addr: u16) -> u8 {
        // a copy owns bus while it runs, so only high RAM answers CPU
        if self.dma_left > 0 && !(0xFF80..=0xFFFE).contains(&addr) {
            return 0xFF;
        }
        self.read_anywhere(addr)
    }

    // copy itself is what holds bus, so it reads without asking
    fn read_anywhere(&mut self, addr: u16) -> u8 {
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
            0xFF46 => self.dma,
            0xFF40..=0xFF4B => self.ppu.read_register(addr),
            0xFF4D => 0x7E | self.key1 | if self.double_speed { 0x80 } else { 0x00 },
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
            0xFF46 => {
                self.dma = value;
                self.dma_from = (value as u16) << 8;
                self.dma_left = OAM_BYTES;
                self.dma_wait = 4;
            }
            0xFF40..=0xFF4B => self.interrupt_flag |= self.ppu.write_register(addr, value),
            0xFF4D => self.key1 = value & 0x01,
            0xFF4F => self.interrupt_flag |= self.ppu.write_register(addr, value),
            0xFF51..=0xFF55 => {} // TODO(PR-18): HDMA
            0xFF68..=0xFF6B => self.interrupt_flag |= self.ppu.write_register(addr, value),
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

    // fill a page of work RAM, then ask for it to be copied into sprite table
    fn start_dma(bus: &mut Bus) {
        bus.write(0xFF40, 0x00); // screen off, so sprite table is readable in test
        for i in 0..OAM_BYTES as u16 {
            bus.write(0xC100 + i, 0x40 + i as u8); // never zero, so untouched bytes stand out
        }
        bus.write(0xFF46, 0xC1);
    }

    #[test]
    fn a_copy_moves_one_byte_every_machine_cycle() {
        let mut bus = Bus::testing();
        start_dma(&mut bus);

        bus.tick(4);
        assert_eq!(bus.ppu.read_oam(0xFE00), 0x40, "first byte landed");
        assert_eq!(bus.ppu.read_oam(0xFE01), 0, "second has not yet");

        bus.tick(4);
        assert_eq!(bus.ppu.read_oam(0xFE01), 0x41);
    }

    #[test]
    fn a_copy_ends_after_160_bytes() {
        let mut bus = Bus::testing();
        start_dma(&mut bus);
        bus.tick(4 * OAM_BYTES as u32);

        assert_eq!(bus.ppu.read_oam(0xFE9F), 0x40 + 159, "last byte landed");
        assert_eq!(bus.read(0xC100), 0x40, "bus is free again");
    }

    #[test]
    fn only_high_ram_answers_while_a_copy_runs() {
        let mut bus = Bus::testing();
        bus.write(0xFF80, 0x42);
        start_dma(&mut bus);
        bus.tick(4);

        assert_eq!(bus.read(0xC100), 0xFF, "work RAM is shut out");
        assert_eq!(bus.read(0xFF80), 0x42, "high RAM still answers");
    }

    #[test]
    fn the_block_asked_for_reads_back() {
        let mut bus = Bus::testing();
        bus.write(0xFF46, 0xC1);
        bus.tick(4 * OAM_BYTES as u32);
        assert_eq!(bus.read(0xFF46), 0xC1);
    }
}
