//! DIV and TIMA, the two counters a game uses to measure time.

use crate::bus::IF_TIMER;

pub struct Timer {
    div: u16,         // FF04 counts up forever; a game only sees its top byte
    tima: u8,         // FF05 counts up at the speed tac picks
    tma: u8,          // FF06 what tima restarts from after it overflows
    tac: u8,          // FF07 timer on or off, and how fast
    reload_in: u8,    // ticks until an overflowed tima is refilled from tma
    reloaded_for: u8, // ticks left in the cycle where tima was just refilled
    frozen: bool,     // true during STOP, when the divider does not count
}

impl Timer {
    pub fn new() -> Self {
        Self {
            div: 0xABCC,
            tima: 0,
            tma: 0,
            tac: 0xF8,
            reload_in: 0,
            reloaded_for: 0,
            frozen: false,
        }
    }

    /// Set DIV back to 0. A write to FF04 does this, and so does STOP.
    pub fn reset_div(&mut self) {
        let before = self.selected_bit();
        self.div = 0;
        self.step_on_falling_edge(before); // dropping the selected bit to 0 counts as a tick
    }

    /// Stop or restart counting. The CPU freezes the timer for as long as it is in STOP.
    pub fn freeze(&mut self, frozen: bool) {
        self.frozen = frozen;
    }

    pub fn tick(&mut self, t_cycles: u32) -> u8 {
        if self.frozen {
            return 0;
        }
        let mut irq = 0;
        for _ in 0..t_cycles {
            irq |= self.finish_reload();
            let before = self.selected_bit();
            self.div = self.div.wrapping_add(1);
            self.step_on_falling_edge(before);
        }
        irq
    }

    // An overflow refills TIMA one M-cycle late, and only then asks for the interrupt.
    fn finish_reload(&mut self) -> u8 {
        self.reloaded_for = self.reloaded_for.saturating_sub(1);
        if self.reload_in == 0 {
            return 0;
        }
        self.reload_in -= 1;
        if self.reload_in > 0 {
            return 0;
        }
        self.tima = self.tma;
        self.reloaded_for = 4;
        IF_TIMER
    }

    // The bit of DIV that TAC picks. TIMA steps each time it falls from 1 to 0.
    fn selected_bit(&self) -> bool {
        let bit = match self.tac & 0x03 {
            0 => 9,
            1 => 3,
            2 => 5,
            _ => 7,
        };
        (self.div >> bit) & 1 == 1
    }

    // Step TIMA when the picked bit drops to 0. On the Color, switching the timer off never counts.
    fn step_on_falling_edge(&mut self, before: bool) {
        if before && !self.selected_bit() && self.tac & 0x04 != 0 {
            let (next, overflow) = self.tima.overflowing_add(1);
            self.tima = next;
            if overflow {
                self.reload_in = 4; // tima reads 00 until then
            }
        }
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
            0xFF04 => self.reset_div(),
            // In the 00 cycle a write cancels the reload; right after a reload it is ignored.
            0xFF05 => {
                if self.reload_in > 0 {
                    self.reload_in = 0;
                    self.tima = value;
                } else if self.reloaded_for == 0 {
                    self.tima = value;
                }
            }
            // Right after a reload, a new TMA goes straight into TIMA too.
            0xFF06 => {
                self.tma = value;
                if self.reloaded_for > 0 {
                    self.tima = value;
                }
            }
            // Picking a different bit can also look like a fall from 1 to 0.
            0xFF07 => {
                let before = self.selected_bit();
                self.tac = value & 0x07;
                self.step_on_falling_edge(before);
            }
            _ => {}
        }
    }
}

impl Default for Timer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Running every 16 ticks, DIV at 0, TMA at 80, and TIMA one step from overflowing.
    fn about_to_overflow() -> Timer {
        let mut timer = Timer::new();
        timer.write(0xFF04, 0);
        timer.write(0xFF07, 0x05);
        timer.write(0xFF06, 0x80);
        timer.write(0xFF05, 0xFF);
        timer
    }

    #[test]
    fn an_overflow_reads_zero_for_one_m_cycle_then_reloads() {
        let mut timer = about_to_overflow();
        assert_eq!(timer.tick(16), 0, "no interrupt yet");
        assert_eq!(timer.read(0xFF05), 0x00);
        assert_eq!(
            timer.tick(4),
            IF_TIMER,
            "the interrupt arrives one M-cycle late"
        );
        assert_eq!(timer.read(0xFF05), 0x80);
    }

    #[test]
    fn writing_tima_in_the_zero_cycle_cancels_the_overflow() {
        let mut timer = about_to_overflow();
        timer.tick(16);
        timer.write(0xFF05, 0x42);
        assert_eq!(timer.tick(4), 0, "no interrupt");
        assert_eq!(timer.read(0xFF05), 0x42, "TMA is not copied");
    }

    #[test]
    fn writing_tima_as_it_reloads_is_ignored() {
        let mut timer = about_to_overflow();
        timer.tick(16);
        timer.tick(4);
        timer.write(0xFF05, 0x42);
        assert_eq!(timer.read(0xFF05), 0x80);
    }

    #[test]
    fn writing_tma_as_it_reloads_goes_into_tima_too() {
        let mut timer = about_to_overflow();
        timer.tick(16);
        timer.tick(4);
        timer.write(0xFF06, 0x33);
        assert_eq!(timer.read(0xFF05), 0x33);
    }

    #[test]
    fn the_reload_window_lasts_one_m_cycle() {
        let mut timer = about_to_overflow();
        timer.tick(16);
        timer.tick(4);
        timer.tick(4);
        timer.write(0xFF05, 0x42);
        assert_eq!(timer.read(0xFF05), 0x42);
    }

    #[test]
    fn resetting_div_ticks_tima_only_if_the_selected_bit_was_on() {
        let mut timer = about_to_overflow();
        timer.write(0xFF05, 0x10);
        timer.tick(8); // bit 3 is now on
        timer.write(0xFF04, 0);
        assert_eq!(timer.read(0xFF05), 0x11);

        let mut timer = about_to_overflow();
        timer.write(0xFF05, 0x10);
        timer.tick(4); // bit 3 is still off
        timer.write(0xFF04, 0);
        assert_eq!(timer.read(0xFF05), 0x10);
    }

    #[test]
    fn picking_another_speed_can_tick_tima() {
        let mut timer = about_to_overflow();
        timer.write(0xFF05, 0x10);
        timer.tick(8); // bit 3 on, bit 5 off
        timer.write(0xFF07, 0x06); // move from bit 3 to bit 5
        assert_eq!(timer.read(0xFF05), 0x11);
    }

    // An older Game Boy would tick here. The Color does not.
    #[test]
    fn switching_the_timer_off_does_not_tick_on_the_color() {
        let mut timer = about_to_overflow();
        timer.write(0xFF05, 0x10);
        timer.tick(8);
        timer.write(0xFF07, 0x01); // same speed, switched off
        assert_eq!(timer.read(0xFF05), 0x10);
    }
}
