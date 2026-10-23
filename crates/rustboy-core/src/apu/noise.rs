//! channel that makes hiss rather than a note: drums, explosions, footsteps.

/// how many ticks one step takes, before shift is applied. Code 0 is a half step.
const DIVISORS: [u16; 8] = [8, 16, 32, 48, 64, 80, 96, 112];

#[derive(Debug)]
pub struct Noise {
    pub volume: u8,
    pub enabled: bool,
    pub dac_on: bool,

    pub env_start: u8,
    pub env_up: bool,
    pub env_period: u8,
    env_timer: u8,

    pub length_enabled: bool,
    length: u8,

    pub shift: u8,   // bigger shift means slower, deeper hiss
    pub divisor: u8, // 0 to 7, picks base speed
    pub short: bool, // true makes pattern repeat sooner, which sounds more like a tone
    register: u16,   // 15 bits that shuffle to make randomness
    timer: u16,      // ticks left before next shuffle
}

impl Default for Noise {
    fn default() -> Self {
        Self {
            volume: 0,
            enabled: false,
            dac_on: false,
            env_start: 0,
            env_up: false,
            env_period: 0,
            env_timer: 0,
            length_enabled: false,
            length: 0,
            shift: 0,
            divisor: 0,
            short: false,
            register: 0x7FFF,
            timer: 0,
        }
    }
}

impl Noise {
    fn reload(&self) -> u16 {
        DIVISORS[self.divisor as usize & 7] << self.shift
    }

    /// one shuffle: two bits are compared, and answer goes in at top.
    pub fn tick(&mut self) {
        self.timer = self.timer.saturating_sub(1);
        if self.timer > 0 {
            return;
        }
        self.timer = self.reload();

        let feedback = (self.register ^ (self.register >> 1)) & 1;
        self.register = (self.register >> 1) | (feedback << 14);
        if self.short {
            // same answer also goes in halfway up, so pattern comes round much sooner
            self.register = (self.register & !(1 << 6)) | (feedback << 6);
        }
    }

    pub fn trigger(&mut self) {
        self.enabled = self.dac_on;
        self.timer = self.reload();
        self.register = 0x7FFF; // every bit set, which is where hardware starts
        if self.length == 0 {
            self.length = 64;
        }
        self.volume = self.env_start;
        self.env_timer = self.env_period;
    }

    pub fn set_length(&mut self, used: u8) {
        self.length = 64 - (used & 0x3F);
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

    pub fn tick_envelope(&mut self) {
        if self.env_period == 0 {
            return;
        }
        self.env_timer = self.env_timer.saturating_sub(1);
        if self.env_timer > 0 {
            return;
        }
        self.env_timer = self.env_period;
        match (self.env_up, self.volume) {
            (true, v) if v < 15 => self.volume += 1,
            (false, v) if v > 0 => self.volume -= 1,
            _ => {}
        }
    }

    /// loud when lowest bit is clear, so output flips about unpredictably.
    pub fn output(&self) -> u8 {
        if !self.enabled {
            return 0;
        }
        (!self.register & 1) as u8 * self.volume
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playing(shift: u8, divisor: u8, short: bool) -> Noise {
        let mut noise = Noise {
            env_start: 15,
            dac_on: true,
            shift,
            divisor,
            short,
            ..Noise::default()
        };
        noise.trigger();
        noise
    }

    // one shuffle per reload, so stepping it this many ticks moves it exactly once
    fn shuffle(noise: &mut Noise) {
        let ticks = DIVISORS[noise.divisor as usize] << noise.shift;
        for _ in 0..ticks {
            noise.tick();
        }
    }

    #[test]
    fn the_output_flips_about_instead_of_repeating_evenly() {
        // it starts every bit set, so first fifteen shuffles are a run. Look past those.
        let mut noise = playing(0, 0, false);
        let mut pattern = Vec::new();
        for _ in 0..200 {
            pattern.push(noise.output() > 0);
            shuffle(&mut noise);
        }
        let flips = pattern.windows(2).filter(|p| p[0] != p[1]).count();
        assert!(flips > 50, "too regular to be noise: {flips} flips in 200");
    }

    // the long pattern takes 32767 shuffles to come round, which is why it sounds like hiss
    #[test]
    fn the_long_pattern_does_not_repeat_quickly() {
        let mut noise = playing(0, 0, false);
        let start = noise.register;
        let mut came_round = None;
        for step in 1..=1000 {
            shuffle(&mut noise);
            if noise.register == start {
                came_round = Some(step);
                break;
            }
        }
        assert_eq!(came_round, None, "pattern repeated far too soon");
    }

    // the short pattern comes round after 127, close enough to hear as a buzz
    #[test]
    fn the_short_pattern_comes_round_after_127() {
        // only low seven bits go round in this mode; ones above keep shifting along
        let mut noise = playing(0, 0, true);
        let start = noise.register & 0x7F;
        let mut came_round = 0;
        for step in 1..=400 {
            shuffle(&mut noise);
            if noise.register & 0x7F == start {
                came_round = step;
                break;
            }
        }
        assert_eq!(came_round, 127);
    }

    #[test]
    fn a_bigger_shift_shuffles_more_slowly() {
        for shift in [0, 1, 4] {
            let mut noise = playing(shift, 0, false);
            let before = noise.register;
            for _ in 0..(DIVISORS[0] << shift) - 1 {
                noise.tick();
            }
            assert_eq!(noise.register, before, "moved early at shift {shift}");
            noise.tick();
            assert_ne!(noise.register, before, "did not move at shift {shift}");
        }
    }

    #[test]
    fn the_divisor_sets_the_base_speed() {
        for divisor in 0..8u8 {
            let mut noise = playing(0, divisor, false);
            let before = noise.register;
            for _ in 0..DIVISORS[divisor as usize] - 1 {
                noise.tick();
            }
            assert_eq!(noise.register, before, "moved early at divisor {divisor}");
            noise.tick();
            assert_ne!(noise.register, before, "did not move at divisor {divisor}");
        }
    }

    #[test]
    fn a_channel_with_its_dac_off_cannot_be_triggered() {
        let mut noise = playing(0, 0, false);
        noise.dac_on = false;
        noise.trigger();
        assert!(!noise.enabled);
        assert_eq!(noise.output(), 0);
    }

    #[test]
    fn a_fading_hiss_loses_volume_a_step_at_a_time() {
        let mut noise = playing(0, 0, false);
        noise.env_period = 1;
        noise.trigger();

        assert_eq!(noise.volume, 15);
        noise.tick_envelope();
        assert_eq!(noise.volume, 14);
    }

    #[test]
    fn a_hiss_with_a_time_limit_stops_itself() {
        let mut noise = playing(0, 0, false);
        noise.set_length(63);
        noise.length_enabled = true;

        noise.tick_length();
        assert!(!noise.enabled);
    }
}
