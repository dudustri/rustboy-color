use super::*;

#[test]
fn f_low_nibble_is_always_zero() {
    let mut regs = Registers::post_boot_cgb();
    regs.set_af(0xFFFF);
    assert_eq!(regs.af(), 0xFFF0);
}

#[test]
fn pairs_round_trip() {
    let mut regs = Registers::post_boot_cgb();
    regs.set_hl(0xC0DE);
    assert_eq!(regs.h, 0xC0);
    assert_eq!(regs.l, 0xDE);
    assert_eq!(regs.hl(), 0xC0DE);
}
