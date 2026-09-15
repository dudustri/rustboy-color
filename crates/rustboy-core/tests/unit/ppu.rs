use super::*;

#[test]
fn a_line_is_456_dots_and_a_frame_is_154_lines() {
    let mut ppu = Ppu::new();
    ppu.tick(DOTS_PER_LINE * LINES_PER_FRAME as u32);
    assert_eq!(ppu.ly(), 0);
    assert_eq!(ppu.mode(), Mode::OamScan);
}

#[test]
fn vblank_is_requested_after_the_last_visible_line() {
    let mut ppu = Ppu::new();
    let irq = ppu.tick(DOTS_PER_LINE * SCREEN_HEIGHT as u32);
    assert_eq!(irq & IF_VBLANK, IF_VBLANK);
    assert!(ppu.frame_ready);
    assert_eq!(ppu.mode(), Mode::VBlank);
}

#[test]
fn a_disabled_lcd_does_not_advance() {
    let mut ppu = Ppu::new();
    ppu.write_register(0xFF40, 0x00);
    ppu.tick(DOTS_PER_LINE * 10);
    assert_eq!(ppu.ly(), 0);
}
