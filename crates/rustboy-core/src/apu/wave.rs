//! channel that plays a shape the game draws itself, instead of a fixed square.

/// 32 steps of 4 bits each, packed two to a byte.
pub const SAMPLES: usize = 32;

#[derive(Debug)]
pub struct Wave {
    pub ram: [u8; SAMPLES / 2], // FF30-FF3F, two steps per byte
    pub period: u16,            // 11 bits. a bigger number is a lower note
    pub enabled: bool,          // stays off until a game triggers it
    pub dac_on: bool,           // this channel has its own switch, unlike the square ones
    pub volume_code: u8,        // 0 silent, 1 full, 2 half, 3 quarter
    pub length_enabled: bool,
    length: u16, // counts down to zero, then channel stops. 256 steps, not 64
    step: u8,    // which of 32 steps we are on
    timer: u16,  // ticks left before next step
}

impl Default for Wave {
    fn default() -> Self {
        Self {
            ram: [0; SAMPLES / 2],
            period: 0,
            enabled: false,
            dac_on: false,
            volume_code: 0,
            length_enabled: false,
            length: 0,
            step: 0,
            timer: 0,
        }
    }
}

impl Wave {
    /// ticks between steps. twice as quick as a square channel, with twice as many steps.
    fn reload(&self) -> u16 {
        (2048 - self.period) * 2
    }

    pub fn tick(&mut self) {
        self.timer = self.timer.saturating_sub(1);
        if self.timer == 0 {
            self.timer = self.reload();
            self.step = (self.step + 1) % SAMPLES as u8;
        }
    }

    pub fn trigger(&mut self) {
        self.enabled = self.dac_on;
        self.timer = self.reload();
        self.step = 0; // unlike a square channel, this one starts its shape from beginning
        if self.length == 0 {
            self.length = 256;
        }
    }

    /// how long a note lasts. This channel counts to 256, so it can hold four times longer.
    pub fn set_length(&mut self, used: u8) {
        self.length = 256 - used as u16;
    }

    pub fn tick_length(&mut self) {
        if !self.length_enabled || self.length == 0 {
            return;
        }
        self.length -= 1;
        if self.length == 0 {
            self.enabled = false;
        }
    }

    /// how loud this channel is right now, 0 to 15.
    pub fn output(&self) -> u8 {
        if !self.enabled {
            return 0;
        }
        let byte = self.ram[self.step as usize / 2];
        // first step of a pair lives in top half of its byte
        let sample = if self.step % 2 == 0 {
            byte >> 4
        } else {
            byte & 0x0F
        };
        match self.volume_code {
            1 => sample,
            2 => sample >> 1,
            3 => sample >> 2,
            _ => 0, // code zero is silence, however the shape was drawn
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // a channel playing a ramp: step 0 is 0, step 1 is 1, and so on up to 15 twice over
    fn playing(period: u16) -> Wave {
        let mut wave = Wave {
            period,
            dac_on: true,
            volume_code: 1,
            ..Wave::default()
        };
        for (i, byte) in wave.ram.iter_mut().enumerate() {
            let first = (i as u8 * 2) % 16;
            *byte = (first << 4) | ((first + 1) % 16);
        }
        wave.trigger();
        wave
    }

    fn step_once(wave: &mut Wave) {
        for _ in 0..(2048 - wave.period) * 2 {
            wave.tick();
        }
    }

    #[test]
    fn it_plays_the_shape_the_game_drew() {
        let mut wave = playing(2040);
        for expected in [0, 1, 2, 3, 4, 5] {
            assert_eq!(wave.output(), expected);
            step_once(&mut wave);
        }
    }

    #[test]
    fn the_shape_repeats_after_32_steps() {
        let mut wave = playing(2040);
        let first = wave.output();
        for _ in 0..SAMPLES {
            step_once(&mut wave);
        }
        assert_eq!(wave.output(), first);
    }

    #[test]
    fn the_volume_code_halves_and_quarters_the_shape() {
        for (code, expected) in [(1, 8), (2, 4), (3, 2), (0, 0)] {
            let mut wave = playing(2040);
            wave.volume_code = code;
            for _ in 0..8 {
                step_once(&mut wave); // step 8 of ramp holds value 8
            }
            assert_eq!(wave.output(), expected, "code {code}");
        }
    }

    #[test]
    fn a_channel_nobody_triggered_is_silent() {
        let mut wave = playing(2040);
        wave.enabled = false;
        assert_eq!(wave.output(), 0);
    }

    #[test]
    fn its_own_switch_can_cut_it_off() {
        let mut wave = playing(2040);
        wave.dac_on = false;
        wave.trigger();
        assert!(!wave.enabled);
        assert_eq!(wave.output(), 0);
    }

    // this channel counts to 256, four times longer than a square one
    #[test]
    fn a_full_note_lasts_256_beats() {
        let mut wave = playing(2040);
        wave.set_length(0);
        wave.length_enabled = true;

        for beat in 0..255 {
            wave.tick_length();
            assert!(wave.enabled, "stopped early at beat {beat}");
        }
        wave.tick_length();
        assert!(!wave.enabled);
    }

    // one whole shape is 32 steps, each half as long as a square channel's
    #[test]
    fn a_bigger_period_makes_a_slower_wave() {
        for (period, rounds) in [(1024, 64), (1792, 256)] {
            let mut wave = playing(period);
            let mut laps = 0;
            let mut was_start = true;
            for _ in 0..crate::T_CYCLES_PER_SECOND {
                wave.tick();
                let at_start = wave.step == 0;
                laps += u32::from(at_start && !was_start);
                was_start = at_start;
            }
            assert_eq!(laps, rounds, "period {period}");
        }
    }
}
