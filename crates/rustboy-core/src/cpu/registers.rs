//! The CPU's registers: the handful of values it can work on directly.

/// The flag register `F`: `Z N H C` live in the top 4 bits, the bottom 4 are always zero.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Flags {
    pub z: bool, // the last sum came out zero
    pub n: bool, // the last sum was a subtraction; only DAA looks at it
    pub h: bool, // the low 4 bits overflowed; only DAA looks at it
    pub c: bool, // the result did not fit in 8 bits, or 16 for the wider sums
}

impl Flags {
    pub const fn bits(self) -> u8 {
        ((self.z as u8) << 7)
            | ((self.n as u8) << 6)
            | ((self.h as u8) << 5)
            | ((self.c as u8) << 4)
    }

    /// Throws away the bottom 4 bits, exactly like the real chip.
    pub const fn from_bits(value: u8) -> Self {
        Self {
            z: value & 0x80 != 0,
            n: value & 0x40 != 0,
            h: value & 0x20 != 0,
            c: value & 0x10 != 0,
        }
    }
}

/// An 8-bit register. Opcodes always list them in the order `B C D E H L (HL) A`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reg8 {
    A,
    B,
    C,
    D,
    E,
    H,
    L,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reg16 {
    AF,
    BC,
    DE,
    HL,
    SP,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Registers {
    pub a: u8,    // accumulator: almost every sum ends up here
    pub f: Flags, // how the last sum turned out
    pub b: u8,    // spare byte, pairs with c
    pub c: u8,    // spare byte, pairs with b
    pub d: u8,    // spare byte, pairs with e
    pub e: u8,    // spare byte, pairs with d
    pub h: u8,    // spare byte, pairs with l to point at memory
    pub l: u8,    // spare byte, pairs with h to point at memory
    pub sp: u16,  // stack pointer: top of the scratch pile
    pub pc: u16,  // program counter: which byte the CPU runs next
}

impl Registers {
    /// Where the boot ROM leaves the CPU, so we can skip it. Games read `a = 0x11` to spot a Color.
    pub fn post_boot_cgb() -> Self {
        Self {
            a: 0x11,
            f: Flags::from_bits(0x80),
            b: 0x00,
            c: 0x00,
            d: 0xFF,
            e: 0x56,
            h: 0x00,
            l: 0x0D,
            sp: 0xFFFE,
            pc: 0x0100, // where the cartridge starts running
        }
    }

    pub fn af(&self) -> u16 {
        ((self.a as u16) << 8) | self.f.bits() as u16
    }

    pub fn bc(&self) -> u16 {
        ((self.b as u16) << 8) | self.c as u16
    }

    pub fn de(&self) -> u16 {
        ((self.d as u16) << 8) | self.e as u16
    }

    pub fn hl(&self) -> u16 {
        ((self.h as u16) << 8) | self.l as u16
    }

    pub fn set_af(&mut self, value: u16) {
        self.a = (value >> 8) as u8;
        self.f = Flags::from_bits(value as u8);
    }

    pub fn set_bc(&mut self, value: u16) {
        self.b = (value >> 8) as u8;
        self.c = value as u8;
    }

    pub fn set_de(&mut self, value: u16) {
        self.d = (value >> 8) as u8;
        self.e = value as u8;
    }

    pub fn set_hl(&mut self, value: u16) {
        self.h = (value >> 8) as u8;
        self.l = value as u8;
    }

    pub fn read8(&self, reg: Reg8) -> u8 {
        match reg {
            Reg8::A => self.a,
            Reg8::B => self.b,
            Reg8::C => self.c,
            Reg8::D => self.d,
            Reg8::E => self.e,
            Reg8::H => self.h,
            Reg8::L => self.l,
        }
    }

    pub fn write8(&mut self, reg: Reg8, value: u8) {
        match reg {
            Reg8::A => self.a = value,
            Reg8::B => self.b = value,
            Reg8::C => self.c = value,
            Reg8::D => self.d = value,
            Reg8::E => self.e = value,
            Reg8::H => self.h = value,
            Reg8::L => self.l = value,
        }
    }

    pub fn read16(&self, reg: Reg16) -> u16 {
        match reg {
            Reg16::AF => self.af(),
            Reg16::BC => self.bc(),
            Reg16::DE => self.de(),
            Reg16::HL => self.hl(),
            Reg16::SP => self.sp,
        }
    }

    pub fn write16(&mut self, reg: Reg16, value: u16) {
        match reg {
            Reg16::AF => self.set_af(value),
            Reg16::BC => self.set_bc(value),
            Reg16::DE => self.set_de(value),
            Reg16::HL => self.set_hl(value),
            Reg16::SP => self.sp = value,
        }
    }
}

impl Default for Registers {
    fn default() -> Self {
        Self::post_boot_cgb()
    }
}

#[cfg(test)]
mod tests {
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
}
