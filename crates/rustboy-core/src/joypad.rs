//! Joypad matrix. Pan Docs: FF00 P1/JOYP.

use crate::bus::IF_JOYPAD;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Right,
    Left,
    Up,
    Down,
    A,
    B,
    Select,
    Start,
}

impl Button {
    /// Bit position in the internal state: 0-3 direction, 4-7 action.
    fn bit(self) -> u8 {
        match self {
            Button::Right => 0,
            Button::Left => 1,
            Button::Up => 2,
            Button::Down => 3,
            Button::A => 4,
            Button::B => 5,
            Button::Select => 6,
            Button::Start => 7,
        }
    }
}

pub struct Joypad {
    /// One bit per button, set when held. Inverted on read.
    held: u8,
    select: u8,
    interrupt: u8,
}

impl Joypad {
    pub fn new() -> Self {
        Self {
            held: 0,
            select: 0x30,
            interrupt: 0,
        }
    }

    pub fn set_button(&mut self, button: Button, pressed: bool) {
        let mask = 1 << button.bit();
        let was_held = self.held & mask != 0;
        if pressed {
            self.held |= mask;
            if !was_held {
                self.interrupt |= IF_JOYPAD;
            }
        } else {
            self.held &= !mask;
        }
    }

    pub fn take_interrupt(&mut self) -> u8 {
        core::mem::take(&mut self.interrupt)
    }

    /// A pressed button reads as 0, which is why every nibble starts at 0x0F.
    pub fn read(&self) -> u8 {
        let mut lines = 0x0F;
        if self.select & 0x10 == 0 {
            lines &= !(self.held & 0x0F);
        }
        if self.select & 0x20 == 0 {
            lines &= !((self.held >> 4) & 0x0F);
        }
        0xC0 | (self.select & 0x30) | lines
    }

    pub fn write(&mut self, value: u8) {
        self.select = value & 0x30;
    }
}

impl Default for Joypad {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pressed_button_reads_as_zero() {
        let mut joypad = Joypad::new();
        joypad.write(0x20); // select the direction row
        joypad.set_button(Button::Right, true);
        assert_eq!(joypad.read() & 0x01, 0);
    }

    #[test]
    fn unselected_row_is_not_reported() {
        let mut joypad = Joypad::new();
        joypad.write(0x10); // select the action row
        joypad.set_button(Button::Right, true);
        assert_eq!(joypad.read() & 0x01, 0x01);
    }
}
