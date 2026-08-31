//! DIV / TIMA. Pan Docs: Timer and Divider Registers.

use crate::bus::IF_TIMER;

pub struct Timer {
    /// The 16-bit counter behind DIV. Reading DIV returns its high byte.
    div: u16,
    tima: u8,
    tma: u8,
    tac: u8,
}

impl Timer {
    pub fn new() -> Self {
        Self {
            div: 0xABCC,
            tima: 0,
            tma: 0,
            tac: 0xF8,
        }
    }

    pub fn tick(&mut self, t_cycles: u32) -> u8 {
        let mut irq = 0;
        for _ in 0..t_cycles {
            let before = self.div;
            self.div = self.div.wrapping_add(1);
            if self.falling_edge(before, self.div) {
                // TODO(PR-11): TIMA reload is delayed by 4 T-cycles on hardware,
                // and writes during that window are ignored.
                let (next, overflow) = self.tima.overflowing_add(1);
                if overflow {
                    self.tima = self.tma;
                    irq |= IF_TIMER;
                } else {
                    self.tima = next;
                }
            }
        }
        irq
    }

    /// TIMA increments on the falling edge of a bit of the DIV counter, not on
    /// a division of the clock. That is why writing DIV can tick TIMA.
    fn falling_edge(&self, before: u16, after: u16) -> bool {
        if self.tac & 0x04 == 0 {
            return false;
        }
        let bit = match self.tac & 0x03 {
            0 => 9,
            1 => 3,
            2 => 5,
            _ => 7,
        };
        (before >> bit) & 1 == 1 && (after >> bit) & 1 == 0
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0xFF04 => (self.div >> 8) as u8,
            0xFF05 => self.tima,
            0xFF06 => self.tma,
            0xFF07 => self.tac | 0xF8,
            _ => 0xFF,
        }
    }

    pub fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0xFF04 => self.div = 0,
            0xFF05 => self.tima = value,
            0xFF06 => self.tma = value,
            0xFF07 => self.tac = value & 0x07,
            _ => {}
        }
    }
}

impl Default for Timer {
    fn default() -> Self {
        Self::new()
    }
}
