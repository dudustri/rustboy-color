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
