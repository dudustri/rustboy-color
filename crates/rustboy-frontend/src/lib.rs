//! shared host layer: a platform says how to draw, this crate decides what goes on screen.
//!
//! [`Frontend::tick`] is only way to get a frame, so every platform gets title screen.

use rustboy_core::{Button, Cartridge, CartridgeError, Emulator, FRAMEBUFFER_LEN};

/// what every platform has to provide.
pub trait Host {
    /// seconds since program started. Only differences matter.
    fn elapsed(&self) -> f32;

    /// buffer to paint into. Must be [`FRAMEBUFFER_LEN`] bytes.
    fn frame(&mut self) -> &mut [u8];

    /// put painted buffer on screen.
    fn present(&mut self);

    /// take finished sound. Platforms without audio can leave this alone.
    fn queue_audio(&mut self, _samples: &[f32]) {}

    /// put save data somewhere it will survive. Platforms without storage can skip it.
    fn write_save(&mut self, _data: &[u8]) {}
}

/// how long to wait after a game writes its save before putting it away.
const SAVE_AFTER: f32 = 1.0;

/// console, title screen, and rules for which one is showing.
pub struct Frontend {
    emulator: Emulator,
    audio: Vec<f32>,
    splash_over: bool,
    save_due: Option<f32>, // when to write save out, set once a game changes it
}

impl Frontend {
    pub fn new() -> Self {
        Self {
            emulator: Emulator::new(),
            audio: Vec::new(),
            splash_over: false,
            save_due: None,
        }
    }

    /// put a game in. Anything already running is thrown away.
    /// put a game in. Host hands back its old save with [`Frontend::load_save_ram`].
    pub fn load_rom(&mut self, rom: Vec<u8>) -> Result<(), CartridgeError> {
        let cartridge = Cartridge::new(rom)?;
        self.emulator = Emulator::with_cartridge(cartridge);
        self.save_due = None;
        Ok(())
    }

    pub fn is_loaded(&self) -> bool {
        self.emulator.is_loaded()
    }

    /// name cartridge gives itself, or `None` when nothing is loaded.
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

    /// cut title screen short.
    pub fn skip_splash(&mut self) {
        self.splash_over = true;
    }

    pub fn save_ram(&self) -> Option<&[u8]> {
        self.emulator.save_ram()
    }

    /// give a game back what it saved last time. Does nothing if nothing is loaded.
    pub fn load_save_ram(&mut self, data: &[u8]) {
        self.emulator.load_save_ram(data);
    }

    // a game writes its save many times a second, so wait for it to settle before storing
    fn store_save<H: Host>(&mut self, host: &mut H, seconds: f32) {
        if self.emulator.save_changed() && self.save_due.is_none() {
            self.save_due = Some(seconds + SAVE_AFTER);
        }
        if self.save_due.is_some_and(|due| seconds >= due) {
            self.save_due = None;
            if let Some(data) = self.emulator.save_ram() {
                let data = data.to_vec();
                host.write_save(&data);
                self.emulator.mark_saved();
            }
        }
    }

    /// paint one frame and show it. Title screen first, then console.
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

        self.store_save(host, seconds);
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
mod tests {
    use super::*;

    // a pretend platform that records what it was asked to do.
    struct Fake {
        seconds: f32,
        frame: Vec<u8>,
        presented: usize,
        stored: Option<Vec<u8>>, // what it was told to keep, standing in for a file
        writes: usize,
    }

    impl Fake {
        fn new() -> Self {
            Self {
                seconds: 0.0,
                frame: vec![0; FRAMEBUFFER_LEN],
                presented: 0,
                stored: None,
                writes: 0,
            }
        }
    }

    impl Host for Fake {
        fn elapsed(&self) -> f32 {
            self.seconds
        }
        fn frame(&mut self) -> &mut [u8] {
            &mut self.frame
        }
        fn present(&mut self) {
            self.presented += 1;
        }
        fn write_save(&mut self, data: &[u8]) {
            self.stored = Some(data.to_vec());
            self.writes += 1;
        }
    }

    // a 32 KiB game with a battery, so it has save RAM worth keeping
    fn game_with_a_battery() -> Vec<u8> {
        let mut rom = vec![0; 0x8000];
        rom[0x0147] = 0x03; // MBC1 with RAM and a battery
        rom[0x0149] = 0x02; // 8 KiB of it
        rom[0x014D] = rustboy_core::header_checksum(&rom);
        rom
    }

    fn game_without_a_battery() -> Vec<u8> {
        let mut rom = vec![0; 0x8000];
        rom[0x0147] = 0x01; // MBC1, no battery
        rom[0x014D] = rustboy_core::header_checksum(&rom);
        rom
    }

    // pretend a game switched save RAM on and wrote a byte
    fn game_writes_its_save(frontend: &mut Frontend, value: u8) {
        frontend.emulator.bus.write(0x0000, 0x0A);
        frontend.emulator.bus.write(0xA000, value);
    }

    // one tick notices a change, then time passes, then a later tick puts it away
    fn settle(frontend: &mut Frontend, host: &mut Fake) {
        frontend.tick(host);
        host.seconds += SAVE_AFTER + 0.1;
        frontend.tick(host);
    }

    #[test]
    fn a_save_is_kept_once_the_game_stops_writing() {
        let mut frontend = Frontend::new();
        let mut host = Fake::new();
        frontend.load_rom(game_with_a_battery()).unwrap();
        frontend.skip_splash();

        game_writes_its_save(&mut frontend, 0x42);
        frontend.tick(&mut host);
        assert_eq!(host.stored, None, "too soon, it may still be writing");

        host.seconds = SAVE_AFTER + 0.1;
        frontend.tick(&mut host);
        assert_eq!(host.stored.as_ref().unwrap()[0], 0x42);
    }

    #[test]
    fn a_quiet_game_is_not_written_over_and_over() {
        let mut frontend = Frontend::new();
        let mut host = Fake::new();
        frontend.load_rom(game_with_a_battery()).unwrap();
        frontend.skip_splash();

        game_writes_its_save(&mut frontend, 0x42);
        settle(&mut frontend, &mut host);
        assert_eq!(host.writes, 1);

        for step in 1..5 {
            host.seconds += SAVE_AFTER;
            frontend.tick(&mut host);
            assert_eq!(host.writes, 1, "still one, {step} ticks later");
        }
    }

    #[test]
    fn a_game_with_no_battery_is_never_stored() {
        let mut frontend = Frontend::new();
        let mut host = Fake::new();
        frontend.load_rom(game_without_a_battery()).unwrap();
        frontend.skip_splash();

        game_writes_its_save(&mut frontend, 0x42);
        settle(&mut frontend, &mut host);
        assert_eq!(host.writes, 0);
    }

    #[test]
    fn a_save_comes_back_when_the_game_loads_again() {
        let mut frontend = Frontend::new();
        let mut host = Fake::new();
        frontend.load_rom(game_with_a_battery()).unwrap();
        frontend.skip_splash();
        game_writes_its_save(&mut frontend, 0x42);
        settle(&mut frontend, &mut host);

        // same game started over, as if the program had been closed and opened
        let mut again = Frontend::new();
        again.load_rom(game_with_a_battery()).unwrap();
        again.load_save_ram(&host.stored.clone().unwrap());
        again.emulator.bus.write(0x0000, 0x0A);
        assert_eq!(again.emulator.bus.read(0xA000), 0x42);
    }

    #[test]
    fn the_first_frames_are_the_title_screen() {
        let mut frontend = Frontend::new();
        let mut host = Fake::new();
        host.seconds = rustboy_splash::SECONDS / 2.0;
        frontend.tick(&mut host);
        assert!(!frontend.splash_over);
        assert_eq!(host.presented, 1);
    }

    #[test]
    fn the_console_takes_over_when_the_title_ends() {
        let mut frontend = Frontend::new();
        let mut host = Fake::new();
        host.seconds = rustboy_splash::SECONDS;
        frontend.tick(&mut host);
        assert!(frontend.splash_over);
    }

    // once title is over it must not come back, even if clock says so.
    #[test]
    fn the_title_screen_never_returns() {
        let mut frontend = Frontend::new();
        let mut host = Fake::new();
        frontend.skip_splash();
        host.seconds = 0.0;
        frontend.tick(&mut host);
        assert!(frontend.splash_over);
    }
}
