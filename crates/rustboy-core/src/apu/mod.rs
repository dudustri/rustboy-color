//! sound chip. Two square wave channels work; wave and noise arrive in PR-23.

mod pulse;

pub use pulse::Pulse;

use crate::{AUDIO_SAMPLE_RATE, T_CYCLES_PER_SECOND};

const REGISTER_BASE: u16 = 0xFF10;
const REGISTER_COUNT: usize = 0x30;

/// about a second of sound, so a host that stops collecting cannot make this grow forever.
const MAX_QUEUED_SAMPLES: usize = (AUDIO_SAMPLE_RATE as usize) * 2;

pub struct Apu {
    registers: [u8; REGISTER_COUNT], // FF10-FF3F every sound control
    samples: Vec<f32>,               // finished sound waiting for host to collect
    sample_accumulator: u32,         // counts in whole numbers so sample timing never drifts
    pulse1: Pulse,                   // FF10-FF14, the one that can slide its note
    pulse2: Pulse,                   // FF16-FF19
    on: bool,                        // FF26 bit 7 cuts whole chip off
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
        }
    }

    pub fn tick(&mut self, t_cycles: u32) {
        for _ in 0..t_cycles {
            self.pulse1.tick();
            self.pulse2.tick();
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
            0xFF11 => self.pulse1.duty = value >> 6,
            0xFF12 => set_volume(&mut self.pulse1, value),
            0xFF13 => set_period_low(&mut self.pulse1, value),
            0xFF14 => set_period_high(&mut self.pulse1, value),
            0xFF16 => self.pulse2.duty = value >> 6,
            0xFF17 => set_volume(&mut self.pulse2, value),
            0xFF18 => set_period_low(&mut self.pulse2, value),
            0xFF19 => set_period_high(&mut self.pulse2, value),
            0xFF26 => self.on = value & 0x80 != 0,
            _ => {}
        }
    }
}

/// top four bits are how loud to start. All five upper bits zero cuts channel off.
fn set_volume(pulse: &mut Pulse, value: u8) {
    pulse.volume = value >> 4;
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
}
