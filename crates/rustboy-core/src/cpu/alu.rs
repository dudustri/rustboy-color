//! The CPU's maths: every sum, shift and flag rule, kept apart so it can be tested alone.

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

// Fix A after adding or subtracting two decimal numbers stored one digit per half byte.
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
        3 => ((value >> 1) | (carry << 7), value & 1), // RR: the old carry comes in at the top
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
#[path = "../../tests/unit/cpu/alu.rs"]
mod tests;
