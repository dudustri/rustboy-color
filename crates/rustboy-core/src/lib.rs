//! The Game Boy Color itself. Files, windows and speakers belong to the host crates.

pub mod apu;
pub mod bus;
pub mod cartridge;
pub mod cpu;
pub mod emulator;
pub mod joypad;
pub mod ppu;
pub mod serial;
pub mod timer;

pub use bus::Bus;
pub use cartridge::{Cartridge, CartridgeError};
pub use cpu::Cpu;
pub use emulator::Emulator;
pub use joypad::Button;

pub const SCREEN_WIDTH: usize = 160;
pub const SCREEN_HEIGHT: usize = 144;

/// Bytes in one picture: 4 per pixel, one each for red, green, blue and alpha.
pub const FRAMEBUFFER_LEN: usize = SCREEN_WIDTH * SCREEN_HEIGHT * 4;

/// The console's clock speed. A T-cycle is one tick; the CPU works in groups of 4.
pub const T_CYCLES_PER_SECOND: u32 = 4_194_304;

/// Ticks in one full screen: 154 lines of 456, which comes to about 59.7 frames a second.
pub const T_CYCLES_PER_FRAME: u32 = 70_224;

pub const AUDIO_SAMPLE_RATE: u32 = 48_000;
