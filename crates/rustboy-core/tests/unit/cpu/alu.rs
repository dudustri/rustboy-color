use super::*;

fn flags(z: bool, n: bool, h: bool, c: bool) -> Flags {
    Flags { z, n, h, c }
}

#[test]
fn add_notices_the_half_carry() {
    assert_eq!(
        add(0x0F, 0x01, false),
        (0x10, flags(false, false, true, false))
    );
}

#[test]
fn add_wraps_to_zero_with_both_carries() {
    assert_eq!(
        add(0xFF, 0x01, false),
        (0x00, flags(true, false, true, true))
    );
}

#[test]
fn a_full_carry_does_not_imply_a_half_carry() {
    assert_eq!(
        add(0x80, 0x80, false),
        (0x00, flags(true, false, false, true))
    );
}

#[test]
fn adc_counts_the_incoming_carry_in_both_flags() {
    assert_eq!(
        add(0xFE, 0x01, true),
        (0x00, flags(true, false, true, true))
    );
    assert_eq!(
        add(0x0E, 0x01, true),
        (0x10, flags(false, false, true, false))
    );
}

#[test]
fn sub_notices_the_half_borrow() {
    assert_eq!(
        sub(0x10, 0x01, false),
        (0x0F, flags(false, true, true, false))
    );
}

#[test]
fn sub_below_zero_borrows_everything() {
    assert_eq!(
        sub(0x00, 0x01, false),
        (0xFF, flags(false, true, true, true))
    );
}

#[test]
fn equal_values_subtract_to_zero() {
    assert_eq!(
        sub(0x05, 0x05, false),
        (0x00, flags(true, true, false, false))
    );
}

#[test]
fn sbc_counts_the_incoming_borrow_in_both_flags() {
    assert_eq!(
        sub(0x10, 0x0F, true),
        (0x00, flags(true, true, true, false))
    );
    assert_eq!(
        sub(0x10, 0x10, true),
        (0xFF, flags(false, true, true, true))
    );
}

#[test]
fn logic_never_carries() {
    assert_eq!(logic(0x00, true), (0x00, flags(true, false, true, false)));
    assert_eq!(
        logic(0x5A, false),
        (0x5A, flags(false, false, false, false))
    );
}

#[test]
fn inc_notices_the_half_carry_and_wraps() {
    assert_eq!(inc(0x0F, false), (0x10, flags(false, false, true, false)));
    assert_eq!(inc(0xFF, false), (0x00, flags(true, false, true, false)));
}

#[test]
fn dec_notices_the_half_borrow_and_wraps() {
    assert_eq!(dec(0x10, false), (0x0F, flags(false, true, true, false)));
    assert_eq!(dec(0x01, false), (0x00, flags(true, true, false, false)));
    assert_eq!(dec(0x00, false), (0xFF, flags(false, true, true, false)));
}

// Wrapping past zero must not touch C, in either direction.
#[test]
fn inc_and_dec_leave_the_carry_alone() {
    assert!(inc(0xFF, true).1.c);
    assert!(!inc(0xFF, false).1.c);
    assert!(dec(0x00, true).1.c);
    assert!(!dec(0x00, false).1.c);
}

#[test]
fn wide_add_takes_its_half_carry_from_bit_11() {
    assert_eq!(
        add_wide(0x0FFF, 0x0001, false),
        (0x1000, flags(false, false, true, false))
    );
    assert_eq!(
        add_wide(0x00FF, 0x0001, false),
        (0x0100, flags(false, false, false, false))
    );
}

// Z is untouched even when the answer is zero.
#[test]
fn wide_add_keeps_z_as_it_was() {
    assert_eq!(
        add_wide(0xFFFF, 0x0001, false),
        (0x0000, flags(false, false, true, true))
    );
    assert!(add_wide(0x1000, 0x0001, true).1.z);
}

#[test]
fn a_negative_offset_moves_backwards() {
    assert_eq!(add_offset(0xC100, 0xFF).0, 0xC0FF);
    assert_eq!(add_offset(0xC100, 0x80).0, 0xC080); // -128
}

// Store a number from 0 to 99 as two decimal digits, one per half byte.
fn decimal(n: u32) -> u8 {
    (((n / 10) << 4) | (n % 10)) as u8
}

// Every pair of two-digit numbers, added and then fixed, must give the decimal answer.
#[test]
fn daa_fixes_every_decimal_addition() {
    for a in 0..100 {
        for b in 0..100 {
            let (sum, flags) = add(decimal(a), decimal(b), false);
            let (fixed, flags) = daa(sum, flags);
            assert_eq!(fixed, decimal((a + b) % 100), "{a} + {b}");
            assert_eq!(flags.c, a + b >= 100, "{a} + {b}");
            assert_eq!(flags.z, (a + b) % 100 == 0, "{a} + {b}");
        }
    }
}

#[test]
fn daa_fixes_every_decimal_subtraction() {
    for a in 0..100 {
        for b in 0..100 {
            let (difference, flags) = sub(decimal(a), decimal(b), false);
            let (fixed, flags) = daa(difference, flags);
            assert_eq!(fixed, decimal((a + 100 - b) % 100), "{a} - {b}");
            assert_eq!(flags.c, a < b, "{a} - {b}");
            assert!(flags.n, "{a} - {b}");
        }
    }
}

// One pattern through all eight, so each one's difference is visible side by side.
#[test]
fn each_shift_moves_the_bits_its_own_way() {
    let v = 0b1000_0001;
    assert_eq!(
        shift(0, v, false),
        (0b0000_0011, flags(false, false, false, true))
    ); // RLC
    assert_eq!(
        shift(1, v, false),
        (0b1100_0000, flags(false, false, false, true))
    ); // RRC
    assert_eq!(
        shift(2, v, false),
        (0b0000_0010, flags(false, false, false, true))
    ); // RL
    assert_eq!(
        shift(3, v, false),
        (0b0100_0000, flags(false, false, false, true))
    ); // RR
    assert_eq!(
        shift(4, v, false),
        (0b0000_0010, flags(false, false, false, true))
    ); // SLA
    assert_eq!(
        shift(5, v, false),
        (0b1100_0000, flags(false, false, false, true))
    ); // SRA
    assert_eq!(
        shift(6, v, false),
        (0b0001_1000, flags(false, false, false, false))
    ); // SWAP
    assert_eq!(
        shift(7, v, false),
        (0b0100_0000, flags(false, false, false, true))
    ); // SRL
}

// Only RL and RR take the old carry in; the rest ignore it.
#[test]
fn only_rl_and_rr_use_the_incoming_carry() {
    assert_eq!(shift(2, 0, true).0, 0b0000_0001);
    assert_eq!(shift(3, 0, true).0, 0b1000_0000);
    for kind in [0, 1, 4, 5, 6, 7] {
        assert_eq!(shift(kind, 0, true).0, 0, "kind {kind}");
    }
}

#[test]
fn shifting_everything_out_sets_zero() {
    assert_eq!(
        shift(4, 0x80, false),
        (0x00, flags(true, false, false, true))
    ); // SLA
    assert_eq!(
        shift(7, 0x01, false),
        (0x00, flags(true, false, false, true))
    ); // SRL
}

#[test]
fn compare_keeps_a_but_sets_the_flags() {
    let mut cpu = Cpu::new();
    cpu.regs.a = 0x42;
    cpu.alu(7, 0x42);
    assert_eq!(cpu.regs.a, 0x42);
    assert!(cpu.regs.f.z && cpu.regs.f.n);
}

#[test]
fn xor_a_with_itself_clears_it() {
    let mut cpu = Cpu::new();
    cpu.regs.a = 0x9C;
    cpu.alu(5, 0x9C);
    assert_eq!(cpu.regs.a, 0);
    assert!(cpu.regs.f.z);
}
