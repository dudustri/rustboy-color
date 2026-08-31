//! The SM83 register file.

/// The flag register `F`, as `Z N H C` in bits 7-4.
///
/// The low nibble is wired to zero in hardware and stays zero even when a
/// program writes ones into it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Flags {
    pub z: bool,
    /// Set by subtractions. Only `DAA` reads it.
    pub n: bool,
    /// Carry out of bit 3. Only `DAA` reads it.
    pub h: bool,
    /// Carry out of bit 7, or bit 15 for 16-bit adds.
    pub c: bool,
}

impl Flags {
    pub const fn bits(self) -> u8 {
        ((self.z as u8) << 7)
            | ((self.n as u8) << 6)
            | ((self.h as u8) << 5)
            | ((self.c as u8) << 4)
    }

    /// The low nibble is discarded, as the hardware does.
    pub const fn from_bits(value: u8) -> Self {
        Self {
            z: value & 0x80 != 0,
            n: value & 0x40 != 0,
            h: value & 0x20 != 0,
            c: value & 0x10 != 0,
        }
    }
}

/// An 8-bit register operand.
///
/// Opcodes encode these in 3 bits as `B C D E H L (HL) A`, which is why
/// `0x40..=0x7F` is one nested loop over the list.
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
    pub a: u8,
    pub f: Flags,
    pub b: u8,
    pub c: u8,
    pub d: u8,
    pub e: u8,
    pub h: u8,
    pub l: u8,
    pub sp: u16,
    pub pc: u16,
}

impl Registers {
    /// State the CGB boot ROM leaves behind, for booting without it.
    ///
    /// `A = 0x11` is how a game detects a Game Boy Color rather than a DMG.
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
            pc: 0x0100, // cartridge entry point
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
