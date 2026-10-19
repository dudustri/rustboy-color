//! sound chip. Two square wave channels work; wave and noise arrive in PR-23.

mod pulse;

pub use pulse::Pulse;

use crate::{AUDIO_SAMPLE_RATE, T_CYCLES_PER_SECOND};

const REGISTER_BASE: u16 = 0xFF10;
const REGISTER_COUNT: usize = 0x30;

/// one beat of shared timer that paces length and volume changes: 512 a second.
const SEQUENCER_TICKS: u32 = T_CYCLES_PER_SECOND / 512;

/// about a second of sound, so a host that stops collecting cannot make this grow forever.
const MAX_QUEUED_SAMPLES: usize = (AUDIO_SAMPLE_RATE as usize) * 2;

pub struct Apu {
    registers: [u8; REGISTER_COUNT], // FF10-FF3F every sound control
    samples: Vec<f32>,               // finished sound waiting for host to collect
    sample_accumulator: u32,         // counts in whole numbers so sample timing never drifts
    pulse1: Pulse,                   // FF10-FF14, the one that can slide its note
    pulse2: Pulse,                   // FF16-FF19
    on: bool,                        // FF26 bit 7 cuts whole chip off
    sequencer: u32,                  // ticks towards next beat of shared timer
    beat: u8,                        // which of eight beats we are on
}

impl Apu {
    pub fn new() -> Self {
        Self {
            registers: [0; REGISTER_COUNT],
            samples: Vec::new(),
            sample_accumulator: 0,
            pulse1: Pulse::default(),
            pulse2: Pulse::default(),
            on: false,
            sequencer: 0,
            beat: 0,
        }
    }

    pub fn tick(&mut self, t_cycles: u32) {
        for _ in 0..t_cycles {
            self.pulse1.tick();
            self.pulse2.tick();
            self.tick_sequencer();
            self.sample_accumulator += AUDIO_SAMPLE_RATE;
            if self.sample_accumulator >= T_CYCLES_PER_SECOND {
                self.sample_accumulator -= T_CYCLES_PER_SECOND;
                if self.samples.len() < MAX_QUEUED_SAMPLES {
                    let sample = self.mix();
                    self.samples.push(sample); // same sound to both ears until PR-24
                    self.samples.push(sample);
                }
            }
        }
    }

    /// shared timer: eight beats, 512 a second. Length moves on four, volume on one.
    fn tick_sequencer(&mut self) {
        self.sequencer += 1;
        if self.sequencer < SEQUENCER_TICKS {
            return;
        }
        self.sequencer = 0;
        self.beat = (self.beat + 1) & 7;
        if self.beat % 2 == 0 {
            self.pulse1.tick_length();
            self.pulse2.tick_length();
        }
        if self.beat == 2 || self.beat == 6 {
            self.pulse1.tick_sweep(); // only channel one can slide
        }
        if self.beat == 7 {
            self.pulse1.tick_envelope();
            self.pulse2.tick_envelope();
        }
    }

    /// every channel added together, as a number between -1 and 1.
    fn mix(&self) -> f32 {
        if !self.on {
            return 0.0;
        }
        // headroom for four channels, so a full mix later still cannot clip
        (dac(&self.pulse1) + dac(&self.pulse2)) / 4.0
    }

    /// hands waiting samples to host, ordered left, right, left, right.
    pub fn drain(&mut self, out: &mut Vec<f32>) {
        out.append(&mut self.samples);
    }

    pub fn read(&self, addr: u16) -> u8 {
        self.registers[(addr - REGISTER_BASE) as usize]
    }

    pub fn write(&mut self, addr: u16, value: u8) {
        self.registers[(addr - REGISTER_BASE) as usize] = value;
        match addr {
            0xFF10 => {
                self.pulse1.sweep_period = (value >> 4) & 0x07;
                self.pulse1.sweep_down = value & 0x08 != 0;
                self.pulse1.sweep_shift = value & 0x07;
            }
            0xFF11 => {
                self.pulse1.duty = value >> 6;
                self.pulse1.set_length(value);
            }
            0xFF12 => set_volume(&mut self.pulse1, value),
            0xFF13 => set_period_low(&mut self.pulse1, value),
            0xFF14 => set_period_high(&mut self.pulse1, value),
            0xFF16 => {
                self.pulse2.duty = value >> 6;
                self.pulse2.set_length(value);
            }
            0xFF17 => set_volume(&mut self.pulse2, value),
            0xFF18 => set_period_low(&mut self.pulse2, value),
            0xFF19 => set_period_high(&mut self.pulse2, value),
            0xFF26 => self.on = value & 0x80 != 0,
            _ => {}
        }
    }
}

/// how loud to start, which way volume walks, and how fast. Upper five bits zero cuts channel.
fn set_volume(pulse: &mut Pulse, value: u8) {
    pulse.env_start = value >> 4;
    pulse.env_up = value & 0x08 != 0;
    pulse.env_period = value & 0x07;
    pulse.volume = pulse.env_start;
    pulse.dac_on = value & 0xF8 != 0;
    if !pulse.dac_on {
        pulse.enabled = false;
    }
}

fn set_period_low(pulse: &mut Pulse, value: u8) {
    pulse.period = (pulse.period & 0x700) | value as u16;
}

/// low three bits finish period off; top bit starts channel playing.
fn set_period_high(pulse: &mut Pulse, value: u8) {
    pulse.period = (pulse.period & 0xFF) | ((value as u16 & 0x07) << 8);
    pulse.length_enabled = value & 0x40 != 0;
    if value & 0x80 != 0 {
        pulse.trigger();
    }
}

/// a channel's output as a wave around zero. A silent channel sits flat at zero.
fn dac(pulse: &Pulse) -> f32 {
    if !pulse.enabled {
        return 0.0;
    }
    pulse.output() as f32 / 7.5 - 1.0
}

impl Default for Apu {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn produces_roughly_the_sample_rate() {
        let mut apu = Apu::new();
        apu.tick(T_CYCLES_PER_SECOND);
        let mut out = Vec::new();
        apu.drain(&mut out);
        // two numbers per sample, one per ear.
        assert_eq!(out.len(), AUDIO_SAMPLE_RATE as usize * 2);
    }

    // switch chip on, then set one channel to a half and half shape at full volume
    fn playing_apu(period: u16) -> Apu {
        let mut apu = Apu::new();
        apu.write(0xFF26, 0x80); // whole chip on
        apu.write(0xFF11, 0x80); // half and half shape
        apu.write(0xFF12, 0xF0); // loudest, and DAC on
        apu.write(0xFF13, period as u8);
        apu.write(0xFF14, 0x80 | (period >> 8) as u8); // top bit starts it
        apu
    }

    fn collect(apu: &mut Apu, t_cycles: u32) -> Vec<f32> {
        apu.tick(t_cycles);
        let mut out = Vec::new();
        apu.drain(&mut out);
        out
    }

    #[test]
    fn a_triggered_channel_makes_a_wave_not_silence() {
        let mut apu = playing_apu(1024);
        let samples = collect(&mut apu, T_CYCLES_PER_SECOND / 10);

        let loudest = samples.iter().cloned().fold(f32::MIN, f32::max);
        let quietest = samples.iter().cloned().fold(f32::MAX, f32::min);
        assert!(loudest > 0.2, "nothing loud came out: {loudest}");
        assert!(quietest < -0.2, "wave never went below zero: {quietest}");
    }

    #[test]
    fn nothing_comes_out_with_the_chip_switched_off() {
        let mut apu = playing_apu(1024);
        apu.write(0xFF26, 0x00);
        let samples = collect(&mut apu, T_CYCLES_PER_SECOND / 10);

        assert!(samples.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn both_ears_get_the_same_sound_for_now() {
        let mut apu = playing_apu(1024);
        let samples = collect(&mut apu, T_CYCLES_PER_SECOND / 100);
        assert!(samples.chunks_exact(2).all(|pair| pair[0] == pair[1]));
    }

    // writing a zero into the volume register cuts a playing channel dead
    #[test]
    fn taking_the_dac_away_silences_a_playing_channel() {
        let mut apu = playing_apu(1024);
        apu.write(0xFF12, 0x00);
        let samples = collect(&mut apu, T_CYCLES_PER_SECOND / 100);
        assert!(samples.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn the_second_channel_works_the_same_way() {
        let mut apu = Apu::new();
        apu.write(0xFF26, 0x80);
        apu.write(0xFF16, 0x80);
        apu.write(0xFF17, 0xF0);
        apu.write(0xFF18, 0x00);
        apu.write(0xFF19, 0x84); // period 1024, and start
        let samples = collect(&mut apu, T_CYCLES_PER_SECOND / 10);

        let loudest = samples.iter().cloned().fold(f32::MIN, f32::max);
        assert!(loudest > 0.2, "second channel stayed quiet: {loudest}");
    }

    // a note set to its shortest lasts one beat of the 256 a second timer
    #[test]
    fn a_note_with_a_time_limit_goes_quiet_by_itself() {
        let mut apu = playing_apu(1024);
        apu.write(0xFF11, 0xBF); // half and half shape, one beat of time
        apu.write(0xFF14, 0xC4); // start it, and respect the time limit

        let early = collect(&mut apu, T_CYCLES_PER_SECOND / 1000);
        assert!(early.iter().any(|&s| s != 0.0), "should still be playing");

        collect(&mut apu, T_CYCLES_PER_SECOND / 10); // it goes quiet somewhere in here
        let later = collect(&mut apu, T_CYCLES_PER_SECOND / 100);
        assert!(
            later.iter().all(|&s| s == 0.0),
            "should have stopped by now"
        );
    }

    #[test]
    fn a_note_with_no_time_limit_keeps_going() {
        let mut apu = playing_apu(1024);
        apu.write(0xFF11, 0xBF); // same short time, but ...
        apu.write(0xFF14, 0x84); // ... top bit only, so time limit is ignored

        let later = collect(&mut apu, T_CYCLES_PER_SECOND);
        assert!(later.iter().any(|&s| s != 0.0), "should still be playing");
    }

    // volume walks down 64 times a second, so a full fade from loudest takes about a quarter second
    #[test]
    fn a_fading_note_is_quieter_later_than_it_was_at_first() {
        let mut apu = playing_apu(1024);
        apu.write(0xFF12, 0xF1); // loudest, fading, one beat between steps
        apu.write(0xFF14, 0x84); // start it

        // volume moves top of wave; its bottom stays at the same floor, so take peak
        let peak = |samples: &[f32]| samples.iter().cloned().fold(f32::MIN, f32::max);
        let first = peak(&collect(&mut apu, T_CYCLES_PER_SECOND / 1000));
        collect(&mut apu, T_CYCLES_PER_SECOND / 10); // several volume steps go by
        let last = peak(&collect(&mut apu, T_CYCLES_PER_SECOND / 1000));

        assert!(last < first, "got quieter: {first} then {last}");
    }

    #[test]
    fn the_shared_timer_beats_512_times_a_second() {
        let mut apu = Apu::new();
        apu.tick(T_CYCLES_PER_SECOND);
        assert_eq!(apu.beat, 0, "512 beats is a whole number of eights");
        assert_eq!(
            apu.sequencer, 0,
            "and lands exactly, with nothing left over"
        );
    }
}
