//! The eight sums on A, and the flags each one leaves behind.

use super::Cpu;
use super::registers::Flags;

// Add two bytes plus an optional carry, noting both kinds of overflow.
pub(crate) fn add(a: u8, b: u8, carry: bool) -> (u8, Flags) {
    let carry = carry as u16;
    let sum = a as u16 + b as u16 + carry;
    let result = sum as u8;
    let flags = Flags {
        z: result == 0,
        n: false,
        h: (a & 0x0F) as u16 + (b & 0x0F) as u16 + carry > 0x0F, // the low four bits overflowed
        c: sum > 0xFF,
    };
    (result, flags)
}

// Subtract a byte and an optional borrow, noting both kinds of borrow.
pub(crate) fn sub(a: u8, b: u8, carry: bool) -> (u8, Flags) {
    let carry = carry as u16;
    let result = (a as u16).wrapping_sub(b as u16).wrapping_sub(carry) as u8;
    let flags = Flags {
        z: result == 0,
        n: true,
        h: ((a & 0x0F) as u16) < (b & 0x0F) as u16 + carry, // the low four bits had to borrow
        c: (a as u16) < b as u16 + carry,
    };
    (result, flags)
}

// AND, XOR and OR never carry. AND is the odd one that always sets H.
pub(crate) fn logic(result: u8, half: bool) -> (u8, Flags) {
    let flags = Flags {
        z: result == 0,
        n: false,
        h: half,
        c: false,
    };
    (result, flags)
}

// Add one. The carry flag is left as it was.
pub(crate) fn inc(value: u8, carry: bool) -> (u8, Flags) {
    let result = value.wrapping_add(1);
    let flags = Flags {
        z: result == 0,
        n: false,
        h: value & 0x0F == 0x0F,
        c: carry,
    };
    (result, flags)
}

// Subtract one. The carry flag is left as it was.
pub(crate) fn dec(value: u8, carry: bool) -> (u8, Flags) {
    let result = value.wrapping_sub(1);
    let flags = Flags {
        z: result == 0,
        n: true,
        h: value & 0x0F == 0x00,
        c: carry,
    };
    (result, flags)
}

// Add two 16-bit numbers. H comes from bit 11, C from bit 15, and Z is left as it was.
pub(crate) fn add_wide(a: u16, b: u16, zero: bool) -> (u16, Flags) {
    let sum = a as u32 + b as u32;
    let flags = Flags {
        z: zero,
        n: false,
        h: (a & 0x0FFF) + (b & 0x0FFF) > 0x0FFF,
        c: sum > 0xFFFF,
    };
    (sum as u16, flags)
}

// SP plus a signed byte. Both carries come from the low byte, not the whole address.
pub(crate) fn add_offset(sp: u16, offset: u8) -> (u16, Flags) {
    let result = sp.wrapping_add(offset as i8 as u16);
    let low = offset as u16;
    let flags = Flags {
        z: false,
        n: false,
        h: (sp & 0x0F) + (low & 0x0F) > 0x0F,
        c: (sp & 0xFF) + low > 0xFF,
    };
    (result, flags)
}

// Fix A after adding or subtracting two decimal numbers stored one digit per nibble.
pub(crate) fn daa(a: u8, flags: Flags) -> (u8, Flags) {
    let mut adjust = 0;
    let mut carry = flags.c;
    let result = if flags.n {
        if flags.c {
            adjust |= 0x60;
        }
        if flags.h {
            adjust |= 0x06;
        }
        a.wrapping_sub(adjust)
    } else {
        if flags.c || a > 0x99 {
            adjust |= 0x60;
            carry = true;
        }
        if flags.h || a & 0x0F > 0x09 {
            adjust |= 0x06;
        }
        a.wrapping_add(adjust)
    };
    let flags = Flags {
        z: result == 0,
        n: flags.n,
        h: false,
        c: carry,
    };
    (result, flags)
}

// The eight CB rotates and shifts, picked by bits 3 to 5. C catches the bit that falls off.
pub(crate) fn shift(kind: u8, value: u8, carry: bool) -> (u8, Flags) {
    let carry = carry as u8;
    let (result, out) = match kind & 0x07 {
        0 => (value.rotate_left(1), value >> 7), // RLC: bit 7 wraps to bit 0
        1 => (value.rotate_right(1), value & 1), // RRC: bit 0 wraps to bit 7
        2 => ((value << 1) | carry, value >> 7), // RL: the old carry comes in
        3 => ((value >> 1) | (carry << 7), value & 1), // RR
        4 => (value << 1, value >> 7),           // SLA: a zero comes in
        5 => ((value >> 1) | (value & 0x80), value & 1), // SRA: bit 7 stays put
        6 => (value.rotate_left(4), 0),          // SWAP: the halves trade places
        _ => (value >> 1, value & 1),            // SRL: a zero comes in at the top
    };
    let flags = Flags {
        z: result == 0,
        n: false,
        h: false,
        c: out != 0,
    };
    (result, flags)
}

impl Cpu {
    // Bits 3 to 5 of the opcode pick the sum: ADD ADC SUB SBC AND XOR OR CP.
    pub(crate) fn alu(&mut self, kind: u8, value: u8) {
        let a = self.regs.a;
        let carry = self.regs.f.c;
        let (result, flags) = match kind & 0x07 {
            0 => add(a, value, false),
            1 => add(a, value, carry),
            2 => sub(a, value, false),
            3 => sub(a, value, carry),
            4 => logic(a & value, true),
            5 => logic(a ^ value, false),
            6 => logic(a | value, false),
            _ => (a, sub(a, value, false).1), // CP: subtract for the flags, keep A
        };
        self.regs.a = result;
        self.regs.f = flags;
    }
}

#[cfg(test)]
mod tests {
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

    // Store a number from 0 to 99 as two decimal digits, one per nibble.
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
}
