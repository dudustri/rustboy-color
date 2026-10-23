//! square wave channel. two of these carry most of a Game Boy's music.

/// four shapes a pulse can take, written out as eight steps each
const DUTIES: [[u8; 8]; 4] = [
    [0, 0, 0, 0, 0, 0, 0, 1], // high one step in eight: thin and reedy
    [1, 0, 0, 0, 0, 0, 0, 1], // one in four
    [1, 0, 0, 0, 0, 1, 1, 1], // half and half, fullest sound
    [0, 1, 1, 1, 1, 1, 1, 0], // three in four
];

#[derive(Debug, Default)]
pub struct Pulse {
    pub duty: u8,      // which of four shapes, 0 to 3
    pub period: u16,   // 11 bits. a bigger number is a lower note
    pub volume: u8,    // 0 to 15
    pub enabled: bool, // stays off until a game triggers it
    pub dac_on: bool,  // a game can cut channel off entirely, whatever else is set
    step: u8,          // where we are in eight step shape
    timer: u16,        // ticks left before next step

    // a note can be told to stop by itself after a set time
    pub length_enabled: bool,
    length: u8, // counts down to zero, then channel stops

    // volume can walk up or down on its own, which is how notes fade
    pub env_start: u8,  // volume to begin at
    pub env_up: bool,   // true grows louder, false fades
    pub env_period: u8, // steps between each change. zero means never change
    env_timer: u8,      // steps left before next change

    // channel one only: note can slide up or down by itself
    pub sweep_period: u8, // beats between each slide. zero means it never slides
    pub sweep_down: bool, // true shrinks period number, which raises note
    pub sweep_shift: u8,  // how big a step: period divided by two this many times
    sweep_timer: u8,      // beats left before next slide
    sweep_on: bool,       // set up when note starts
    shadow: u16,          // period slide works on, kept apart from one being played
}

impl Pulse {
    /// ticks between steps. eight of these make one whole wave.
    fn reload(&self) -> u16 {
        (2048 - self.period) * 4
    }

    /// one tick of system clock.
    pub fn tick(&mut self) {
        self.timer = self.timer.saturating_sub(1);
        if self.timer == 0 {
            self.timer = self.reload();
            self.step = (self.step + 1) & 7;
        }
    }

    /// start this channel over, which is what writing top bit of its last register does.
    pub fn trigger(&mut self) {
        self.enabled = self.dac_on;
        self.timer = self.reload();
        if self.length == 0 {
            self.length = 64; // a note triggered with no time left gets a full one
        }
        self.volume = self.env_start;
        self.env_timer = self.env_period;

        self.shadow = self.period;
        self.sweep_timer = self.sweep_reload();
        self.sweep_on = self.sweep_period != 0 || self.sweep_shift != 0;
        // a note that would slide straight off the top never gets to play
        if self.sweep_shift != 0 && self.swept() > 2047 {
            self.enabled = false;
        }
    }

    fn sweep_reload(&self) -> u8 {
        if self.sweep_period == 0 {
            8 // zero behaves as eight here, so timer keeps turning over
        } else {
            self.sweep_period
        }
    }

    /// where note would land on next slide. A smaller number is a higher note.
    fn swept(&self) -> u16 {
        let step = self.shadow >> self.sweep_shift;
        if self.sweep_down {
            self.shadow.saturating_sub(step)
        } else {
            self.shadow + step
        }
    }

    /// 128 times a second. Slides note along, and stops channel if it runs off the top.
    pub fn tick_sweep(&mut self) {
        if !self.sweep_on {
            return;
        }
        self.sweep_timer = self.sweep_timer.saturating_sub(1);
        if self.sweep_timer > 0 {
            return;
        }
        self.sweep_timer = self.sweep_reload();
        if self.sweep_period == 0 {
            return; // timer keeps turning, but nothing slides
        }

        let next = self.swept();
        if next > 2047 {
            self.enabled = false; // slid past lowest note there is
            return;
        }
        if self.sweep_shift != 0 {
            self.shadow = next;
            self.period = next;
            // hardware looks one slide further ahead, and stops early if that would overflow
            if self.swept() > 2047 {
                self.enabled = false;
            }
        }
    }

    /// how long a note lasts, counted down 256 times a second. Written as time already used.
    pub fn set_length(&mut self, used: u8) {
        self.length = 64 - (used & 0x3F);
    }

    /// 256 times a second. A note that runs out of time stops itself.
    pub fn tick_length(&mut self) {
        if !self.length_enabled || self.length == 0 {
            return;
        }
        self.length -= 1;
        if self.length == 0 {
            self.enabled = false;
        }
    }

    /// 64 times a second. Moves volume one step towards loud or quiet.
    pub fn tick_envelope(&mut self) {
        if self.env_period == 0 {
            return; // a game that asks for no change gets none
        }
        self.env_timer = self.env_timer.saturating_sub(1);
        if self.env_timer > 0 {
            return;
        }
        self.env_timer = self.env_period;
        match (self.env_up, self.volume) {
            (true, v) if v < 15 => self.volume += 1,
            (false, v) if v > 0 => self.volume -= 1,
            _ => {} // already as loud or as quiet as it goes, so it stays there
        }
    }

    /// how loud this channel is right now, 0 to 15.
    pub fn output(&self) -> u8 {
        if !self.enabled {
            return 0;
        }
        DUTIES[self.duty as usize & 3][self.step as usize] * self.volume
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // a channel playing at full volume, with a period that keeps test numbers small
    fn playing(duty: u8, period: u16) -> Pulse {
        let mut pulse = Pulse {
            duty,
            period,
            env_start: 15, // trigger copies this into volume, as a real write does
            dac_on: true,
            ..Pulse::default()
        };
        pulse.trigger();
        pulse
    }

    // one whole wave is eight steps, each lasting (2048 - period) * 4 ticks
    fn ticks_per_wave(period: u16) -> u32 {
        (2048 - period) as u32 * 4 * 8
    }

    #[test]
    fn a_half_and_half_shape_is_loud_half_the_time() {
        let mut pulse = playing(2, 2040);
        let mut loud = 0;
        for _ in 0..ticks_per_wave(2040) {
            loud += u32::from(pulse.output() > 0);
            pulse.tick();
        }
        assert_eq!(loud, ticks_per_wave(2040) / 2);
    }

    #[test]
    fn a_thin_shape_is_loud_one_step_in_eight() {
        let mut pulse = playing(0, 2040);
        let mut loud = 0;
        for _ in 0..ticks_per_wave(2040) {
            loud += u32::from(pulse.output() > 0);
            pulse.tick();
        }
        assert_eq!(loud, ticks_per_wave(2040) / 8);
    }

    // a bigger period means fewer waves per second, which is a lower note
    #[test]
    fn a_bigger_period_makes_a_slower_wave() {
        for (period, waves) in [(1024, 128), (1792, 512), (1984, 2048)] {
            let mut pulse = playing(2, period);
            let mut edges = 0;
            let mut was_loud = pulse.output() > 0;
            for _ in 0..crate::T_CYCLES_PER_SECOND {
                pulse.tick();
                let loud = pulse.output() > 0;
                edges += u32::from(loud && !was_loud);
                was_loud = loud;
            }
            assert_eq!(edges, waves, "period {period}");
        }
    }

    #[test]
    fn a_channel_nobody_triggered_is_silent() {
        let pulse = Pulse {
            duty: 2,
            period: 2040,
            volume: 15,
            dac_on: true,
            ..Pulse::default()
        }; // never triggered, so it stays quiet whatever else is set
        assert_eq!(pulse.output(), 0);
    }

    // volume and direction both zero cuts channel, however loud it was asked to be
    #[test]
    fn a_channel_with_its_dac_off_cannot_be_triggered() {
        let mut pulse = playing(2, 2040);
        pulse.dac_on = false;
        pulse.trigger();
        assert!(!pulse.enabled);
        assert_eq!(pulse.output(), 0);
    }

    #[test]
    fn volume_scales_how_loud_a_step_is() {
        let mut pulse = playing(2, 2040);
        pulse.volume = 4;
        let loudest = (0..ticks_per_wave(2040))
            .map(|_| {
                let out = pulse.output();
                pulse.tick();
                out
            })
            .max()
            .unwrap();
        assert_eq!(loudest, 4);
    }

    #[test]
    fn a_note_with_a_time_limit_stops_itself() {
        let mut pulse = playing(2, 2040);
        pulse.set_length(63); // one beat of time left
        pulse.length_enabled = true;

        pulse.tick_length();
        assert!(!pulse.enabled, "its time ran out");
    }

    #[test]
    fn a_note_with_no_time_limit_plays_on() {
        let mut pulse = playing(2, 2040);
        pulse.set_length(63);
        pulse.length_enabled = false;

        for _ in 0..100 {
            pulse.tick_length();
        }
        assert!(pulse.enabled);
    }

    // a full note is 64 beats, which is a quarter of a second
    #[test]
    fn a_fresh_note_lasts_sixty_four_beats() {
        let mut pulse = playing(2, 2040);
        pulse.set_length(0);
        pulse.length_enabled = true;

        for beat in 0..63 {
            pulse.tick_length();
            assert!(pulse.enabled, "stopped early at beat {beat}");
        }
        pulse.tick_length();
        assert!(!pulse.enabled);
    }

    #[test]
    fn a_fading_note_loses_volume_a_step_at_a_time() {
        let mut pulse = playing(2, 2040);
        pulse.env_period = 1;
        pulse.env_up = false;
        pulse.trigger();

        assert_eq!(pulse.volume, 15);
        pulse.tick_envelope();
        assert_eq!(pulse.volume, 14);
        pulse.tick_envelope();
        assert_eq!(pulse.volume, 13);
    }

    #[test]
    fn a_fade_stops_at_silence_and_stays_there() {
        let mut pulse = playing(2, 2040);
        pulse.env_start = 2;
        pulse.env_period = 1;
        pulse.trigger();

        for _ in 0..10 {
            pulse.tick_envelope();
        }
        assert_eq!(pulse.volume, 0);
    }

    #[test]
    fn a_swelling_note_stops_at_the_loudest() {
        let mut pulse = playing(2, 2040);
        pulse.env_start = 13;
        pulse.env_period = 1;
        pulse.env_up = true;
        pulse.trigger();

        for _ in 0..10 {
            pulse.tick_envelope();
        }
        assert_eq!(pulse.volume, 15);
    }

    // a bigger period means it waits that many beats between each change
    #[test]
    fn the_envelope_period_sets_how_fast_it_fades() {
        let mut pulse = playing(2, 2040);
        pulse.env_period = 3;
        pulse.trigger();

        pulse.tick_envelope();
        pulse.tick_envelope();
        assert_eq!(pulse.volume, 15, "not yet");
        pulse.tick_envelope();
        assert_eq!(pulse.volume, 14, "third beat moves it");
    }

    #[test]
    fn a_volume_that_never_changes_is_left_alone() {
        let mut pulse = playing(2, 2040);
        pulse.env_period = 0;
        pulse.trigger();

        for _ in 0..100 {
            pulse.tick_envelope();
        }
        assert_eq!(pulse.volume, 15);
    }

    // a sliding note, starting at period 1024
    fn sliding(down: bool, period: u8, shift: u8) -> Pulse {
        let mut pulse = playing(2, 1024);
        pulse.sweep_down = down;
        pulse.sweep_period = period;
        pulse.sweep_shift = shift;
        pulse.trigger();
        pulse
    }

    // period divided by four is 256, so each slide drops that much off period number
    #[test]
    fn a_note_sliding_up_loses_period_each_time() {
        let mut pulse = sliding(true, 1, 2);
        pulse.tick_sweep();
        assert_eq!(pulse.period, 1024 - 256);
        pulse.tick_sweep();
        assert_eq!(pulse.period, 768 - 192, "step shrinks with period");
    }

    #[test]
    fn a_note_sliding_down_gains_period_each_time() {
        let mut pulse = sliding(false, 1, 2);
        pulse.tick_sweep();
        assert_eq!(pulse.period, 1024 + 256);
    }

    // a bigger shift means a smaller step, since it divides more times
    #[test]
    fn the_shift_sets_how_far_each_slide_moves() {
        for (shift, step) in [(1, 512), (2, 256), (4, 64)] {
            let mut pulse = sliding(true, 1, shift);
            pulse.tick_sweep();
            assert_eq!(pulse.period, 1024 - step, "shift {shift}");
        }
    }

    // period here is how many beats to wait, not how far to move
    #[test]
    fn the_sweep_period_sets_how_often_it_slides() {
        let mut pulse = sliding(true, 3, 2);
        pulse.tick_sweep();
        pulse.tick_sweep();
        assert_eq!(pulse.period, 1024, "not yet");
        pulse.tick_sweep();
        assert_eq!(pulse.period, 768, "third beat moves it");
    }

    #[test]
    fn a_note_told_never_to_slide_stays_put() {
        let mut pulse = sliding(true, 0, 2);
        for _ in 0..50 {
            pulse.tick_sweep();
        }
        assert_eq!(pulse.period, 1024);
        assert!(pulse.enabled);
    }

    // sliding down past the lowest note there is stops channel instead of wrapping
    #[test]
    fn sliding_off_the_bottom_stops_the_note() {
        let mut pulse = sliding(false, 1, 1);
        for _ in 0..10 {
            pulse.tick_sweep();
        }
        assert!(!pulse.enabled);
        assert!(
            pulse.period <= 2047,
            "it never played a wrapped around note"
        );
    }

    // hardware checks before playing, so a note that cannot survive one slide never starts
    #[test]
    fn a_note_that_would_overflow_at_once_never_plays() {
        let mut pulse = playing(2, 2000);
        pulse.sweep_down = false;
        pulse.sweep_period = 1;
        pulse.sweep_shift = 1; // 2000 plus 1000 is past the top
        pulse.trigger();
        assert!(!pulse.enabled);
    }

    #[test]
    fn channel_two_never_slides() {
        let mut apu = crate::apu::Apu::new();
        apu.write(0xFF26, 0x80);
        apu.write(0xFF10, 0x11); // a slide setting, which only channel one listens to
        apu.write(0xFF17, 0xF0);
        apu.write(0xFF18, 0x00);
        apu.write(0xFF19, 0x84);
        apu.tick(crate::T_CYCLES_PER_SECOND / 4);
        assert_eq!(apu.pulse2.period, 1024);
    }
}
