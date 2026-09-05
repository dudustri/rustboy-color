//! The whole console. A host never needs anything but this type.

use crate::T_CYCLES_PER_FRAME;
use crate::bus::Bus;
use crate::cartridge::{Cartridge, CartridgeError};
use crate::cpu::Cpu;
use crate::joypad::Button;

pub struct Emulator {
    pub cpu: Cpu, // the processor
    pub bus: Bus, // everything else, reached by address
}

impl Emulator {
    /// A console with no game in it. The screen keeps running, so the host shows a blank LCD.
    pub fn new() -> Self {
        Self {
            cpu: Cpu::new(),
            bus: Bus::new(None),
        }
    }

    pub fn with_cartridge(cartridge: Cartridge) -> Self {
        Self {
            cpu: Cpu::new(),
            bus: Bus::new(Some(cartridge)),
        }
    }

    pub fn load_rom(&mut self, rom: Vec<u8>) -> Result<(), CartridgeError> {
        let cartridge = Cartridge::new(rom)?;
        *self = Self::with_cartridge(cartridge);
        Ok(())
    }

    pub fn is_loaded(&self) -> bool {
        self.bus.cartridge.is_some()
    }

    /// Runs until the screen finishes a picture, which stays right even at double speed.
    pub fn run_frame(&mut self) {
        self.bus.ppu.frame_ready = false;
        // A switched-off screen never finishes a picture, so put a ceiling on the loop.
        let deadline = self.bus.cycles() + (T_CYCLES_PER_FRAME as u64) * 2;
        while !self.bus.ppu.frame_ready && self.bus.cycles() < deadline {
            if self.is_loaded() {
                self.cpu.step(&mut self.bus);
            } else {
                self.bus.tick(4);
            }
        }
    }

    pub fn framebuffer(&self) -> &[u8] {
        &self.bus.ppu.framebuffer
    }

    pub fn set_button(&mut self, button: Button, pressed: bool) {
        self.bus.joypad.set_button(button, pressed);
    }

    pub fn drain_audio(&mut self, out: &mut Vec<f32>) {
        self.bus.apu.drain(out);
    }

    /// The game's save data for the host to store. `None` when no cartridge is in.
    pub fn save_ram(&self) -> Option<&[u8]> {
        self.bus.cartridge.as_ref().map(|c| c.ram())
    }

    pub fn load_save_ram(&mut self, data: &[u8]) {
        if let Some(cartridge) = self.bus.cartridge.as_mut() {
            cartridge.load_ram(data);
        }
    }
}

impl Default for Emulator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FRAMEBUFFER_LEN;

    #[test]
    fn a_frame_completes_without_a_cartridge() {
        let mut emu = Emulator::new();
        emu.run_frame();
        assert!(emu.bus.ppu.frame_ready);
        assert_eq!(emu.framebuffer().len(), FRAMEBUFFER_LEN);
    }

    #[test]
    fn frames_take_about_the_right_number_of_cycles() {
        let mut emu = Emulator::new();
        emu.run_frame(); // the first one starts partway through a line
        let start = emu.bus.cycles();
        emu.run_frame();
        let elapsed = emu.bus.cycles() - start;
        assert_eq!(elapsed, T_CYCLES_PER_FRAME as u64);
    }
}
