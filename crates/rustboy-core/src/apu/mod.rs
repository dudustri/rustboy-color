//! The sound chip. It only remembers register values for now; real sound arrives in M5.

use crate::{AUDIO_SAMPLE_RATE, T_CYCLES_PER_SECOND};

const REGISTER_BASE: u16 = 0xFF10;
const REGISTER_COUNT: usize = 0x30;

/// About a second of sound, so a host that stops collecting cannot make this grow forever.
const MAX_QUEUED_SAMPLES: usize = (AUDIO_SAMPLE_RATE as usize) * 2;

pub struct Apu {
    registers: [u8; REGISTER_COUNT],
    samples: Vec<f32>,
    /// Counts in whole numbers, so deciding when to emit a sample never drifts.
    sample_accumulator: u32,
}

impl Apu {
    pub fn new() -> Self {
        Self {
            registers: [0; REGISTER_COUNT],
            samples: Vec::new(),
            sample_accumulator: 0,
        }
    }

    pub fn tick(&mut self, t_cycles: u32) {
        for _ in 0..t_cycles {
            self.sample_accumulator += AUDIO_SAMPLE_RATE;
            if self.sample_accumulator >= T_CYCLES_PER_SECOND {
                self.sample_accumulator -= T_CYCLES_PER_SECOND;
                if self.samples.len() < MAX_QUEUED_SAMPLES {
                    // TODO(PR-22..24): mix the four sound channels instead of silence.
                    self.samples.push(0.0);
                    self.samples.push(0.0);
                }
            }
        }
    }

    /// Hands the waiting samples to the host, ordered left, right, left, right.
    pub fn drain(&mut self, out: &mut Vec<f32>) {
        out.append(&mut self.samples);
    }

    pub fn read(&self, addr: u16) -> u8 {
        self.registers[(addr - REGISTER_BASE) as usize]
    }

    pub fn write(&mut self, addr: u16, value: u8) {
        self.registers[(addr - REGISTER_BASE) as usize] = value;
    }
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
        // Two numbers per sample, one per ear.
        assert_eq!(out.len(), AUDIO_SAMPLE_RATE as usize * 2);
    }
}
