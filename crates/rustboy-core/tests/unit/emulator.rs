use super::*;
use crate::FRAMEBUFFER_LEN;

#[test]
fn a_frame_completes_without_a_cartridge() {
    let mut emu = Emulator::new();
    emu.run_frame();
    assert!(emu.bus.ppu.frame_ready);
    assert_eq!(emu.framebuffer().len(), FRAMEBUFFER_LEN);
}

#[test]
fn frames_take_about_the_right_number_of_cycles() {
    let mut emu = Emulator::new();
    emu.run_frame(); // the first one starts partway through a line
    let start = emu.bus.cycles();
    emu.run_frame();
    let elapsed = emu.bus.cycles() - start;
    assert_eq!(elapsed, T_CYCLES_PER_FRAME as u64);
}
