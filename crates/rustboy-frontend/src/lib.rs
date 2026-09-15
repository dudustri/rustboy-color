//! The shared host layer: a platform says how to draw, this crate decides what goes on screen.
//!
//! [`Frontend::tick`] is the only way to get a frame, so every platform gets the title screen.

use rustboy_core::{Button, Cartridge, CartridgeError, Emulator, FRAMEBUFFER_LEN};

/// What every platform has to provide.
pub trait Host {
    /// Seconds since the program started. Only differences matter.
    fn elapsed(&self) -> f32;

    /// The buffer to paint into. Must be [`FRAMEBUFFER_LEN`] bytes.
    fn frame(&mut self) -> &mut [u8];

    /// Put the painted buffer on the screen.
    fn present(&mut self);

    /// Take finished sound. Platforms without audio can leave this alone.
    fn queue_audio(&mut self, _samples: &[f32]) {}
}

/// The console, the title screen, and the rules for which one is showing.
pub struct Frontend {
    emulator: Emulator,
    audio: Vec<f32>,
    splash_over: bool,
}

impl Frontend {
    pub fn new() -> Self {
        Self {
            emulator: Emulator::new(),
            audio: Vec::new(),
            splash_over: false,
        }
    }

    /// Put a game in. Anything already running is thrown away.
    pub fn load_rom(&mut self, rom: Vec<u8>) -> Result<(), CartridgeError> {
        let cartridge = Cartridge::new(rom)?;
        self.emulator = Emulator::with_cartridge(cartridge);
        Ok(())
    }

    pub fn is_loaded(&self) -> bool {
        self.emulator.is_loaded()
    }

    /// The name the cartridge gives itself, or `None` when nothing is loaded.
    pub fn title(&self) -> Option<&str> {
        self.emulator
            .bus
            .cartridge
            .as_ref()
            .map(|cartridge| cartridge.header.title.as_str())
    }

    pub fn set_button(&mut self, button: Button, pressed: bool) {
        self.emulator.set_button(button, pressed);
    }

    /// Cut the title screen short.
    pub fn skip_splash(&mut self) {
        self.splash_over = true;
    }

    pub fn save_ram(&self) -> Option<&[u8]> {
        self.emulator.save_ram()
    }

    pub fn load_save_ram(&mut self, data: &[u8]) {
        self.emulator.load_save_ram(data);
    }

    /// Paint one frame and show it. Title screen first, then the console.
    pub fn tick<H: Host>(&mut self, host: &mut H) {
        let seconds = host.elapsed();
        {
            let frame = host.frame();
            debug_assert_eq!(frame.len(), FRAMEBUFFER_LEN);
            if self.splash_over || !rustboy_splash::render(seconds, frame) {
                self.splash_over = true;
                self.emulator.run_frame();
                frame.copy_from_slice(self.emulator.framebuffer());
            }
        }

        self.audio.clear();
        self.emulator.drain_audio(&mut self.audio);
        if !self.audio.is_empty() {
            host.queue_audio(&self.audio);
        }
        host.present();
    }
}

impl Default for Frontend {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "../tests/unit/root.rs"]
mod tests;
