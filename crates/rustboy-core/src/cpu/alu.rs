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
