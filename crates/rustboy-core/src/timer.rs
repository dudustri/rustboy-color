//! DIV and TIMA, the two counters a game uses to measure time.

use crate::bus::IF_TIMER;

pub struct Timer {
    div: u16, // FF04 counts up forever; a game only sees its top byte
    tima: u8, // FF05 counts up at the speed tac picks
    tma: u8,  // FF06 what tima restarts from after it overflows
    tac: u8,  // FF07 timer on or off, and how fast
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
                // TODO(PR-11): hardware waits 4 ticks to reload, ignoring writes in that gap.
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

    /// TIMA steps when a chosen bit of DIV flips 1 to 0, so resetting DIV can tick it.
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
