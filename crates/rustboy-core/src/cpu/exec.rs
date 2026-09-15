//! One arm per opcode, each doing the same steps the real chip does, so timing falls out for free.
//!
//! Opcode reference: <https://gbdev.io/gb-opcodes/optables/>

use super::registers::{Reg8, Reg16};
use super::{Cpu, alu};
use crate::bus::Bus;

// Opcodes number the pairs BC DE HL SP, but push and pop use AF in place of SP.
fn pair(bits: u8, stack: bool) -> Reg16 {
    match bits & 0x03 {
        0 => Reg16::BC,
        1 => Reg16::DE,
        2 => Reg16::HL,
        _ if stack => Reg16::AF,
        _ => Reg16::SP,
    }
}

// Opcodes number the registers B C D E H L (HL) A. The mask keeps this to 0 to 7.
fn operand(bits: u8) -> Option<Reg8> {
    match bits & 0x07 {
        0 => Some(Reg8::B),
        1 => Some(Reg8::C),
        2 => Some(Reg8::D),
        3 => Some(Reg8::E),
        4 => Some(Reg8::H),
        5 => Some(Reg8::L),
        6 => None,          // the byte HL points at, not a register
        _ => Some(Reg8::A), // 7, the only value left after & 0x07
    }
}

impl Cpu {
    pub(crate) fn execute(&mut self, opcode: u8, bus: &mut Bus) {
        match opcode {
            // 0x00 NOP
            0x00 => {}

            // 0x10 STOP - changes speed if a game asked for it, otherwise sleeps until a button.
            0x10 => {
                self.fetch8(bus); // STOP is followed by a byte that nothing uses
                bus.timer.reset_div();
                if bus.speed_switch_armed() {
                    bus.switch_speed();
                } else {
                    self.stopped = true;
                    bus.timer.freeze(true);
                }
            }

            // 0x76 HALT - sleeps, unless interrupts are off and one is waiting, then it trips.
            0x76 => {
                let waiting = bus.interrupt_flag & bus.interrupt_enable & 0x1F != 0;
                if !self.ime && waiting {
                    self.halt_bug = true;
                } else {
                    self.halted = true;
                }
            }

            // 0xC3 JP nn - jump to the address after the opcode.
            0xC3 => {
                let addr = self.fetch16(bus);
                self.idle(bus);
                self.regs.pc = addr;
            }

            // JP cc,nn - the address is always read; a jump that happens costs one more cycle.
            0xC2 | 0xCA | 0xD2 | 0xDA => {
                let address = self.fetch16(bus);
                if self.condition(opcode) {
                    self.idle(bus);
                    self.regs.pc = address;
                }
            }

            // 0x18 JR e8 - jump a short way forwards or back.
            0x18 => self.jump_relative(bus, true),

            // JR cc,e8 - the same, only if the flag test passes.
            0x20 | 0x28 | 0x30 | 0x38 => {
                let take = self.condition(opcode);
                self.jump_relative(bus, take);
            }

            // 0xCD CALL nn - save where to come back to, then jump.
            0xCD => self.call(bus, true),

            // CALL cc,nn - the same, only if the flag test passes.
            0xC4 | 0xCC | 0xD4 | 0xDC => {
                let take = self.condition(opcode);
                self.call(bus, take);
            }

            // 0xC9 RET - take the saved address off the stack and go back.
            0xC9 => self.ret(bus),

            // RET cc - checking the flag costs a cycle, whether it returns or not.
            0xC0 | 0xC8 | 0xD0 | 0xD8 => {
                self.idle(bus);
                if self.condition(opcode) {
                    self.ret(bus);
                }
            }

            // 0xD9 RETI - return, and switch interrupts on at once, with no delay.
            0xD9 => {
                self.ret(bus);
                self.ime = true;
            }

            // RST - a one-byte CALL to one of eight fixed addresses, named by the opcode bits.
            0xC7 | 0xCF | 0xD7 | 0xDF | 0xE7 | 0xEF | 0xF7 | 0xFF => {
                self.idle(bus);
                let back = self.regs.pc;
                self.push16(bus, back);
                self.regs.pc = (opcode & 0x38) as u16;
            }

            // 0xE9 JP HL - the address is already in HL, so nothing is read.
            0xE9 => self.regs.pc = self.regs.hl(),

            // 0xF3 DI - switch interrupts off at once.
            0xF3 => {
                self.ime = false;
                self.ime_pending = false;
            }

            // 0xFB EI - switches interrupts on after the next instruction
            0xFB => self.ime_pending = true,

            // LD rr,nn - the two bytes after the opcode go into a pair.
            0x01 | 0x11 | 0x21 | 0x31 => {
                let value = self.fetch16(bus);
                self.regs.write16(pair(opcode >> 4, false), value);
            }

            // 0x08 LD (nn),SP - the stack pointer goes to memory, low byte first.
            0x08 => {
                let address = self.fetch16(bus);
                let sp = self.regs.sp;
                self.write8(bus, address, sp as u8);
                self.write8(bus, address.wrapping_add(1), (sp >> 8) as u8);
            }

            // 0xF9 LD SP,HL - the idle cycle is the 16-bit value moving across.
            0xF9 => {
                self.idle(bus);
                self.regs.sp = self.regs.hl();
            }

            // PUSH rr - the idle cycle is SP being stepped down before the writes.
            0xC5 | 0xD5 | 0xE5 | 0xF5 => {
                self.idle(bus);
                let value = self.regs.read16(pair(opcode >> 4, true));
                self.push16(bus, value);
            }

            // POP rr
            0xC1 | 0xD1 | 0xE1 | 0xF1 => {
                let value = self.pop16(bus);
                self.regs.write16(pair(opcode >> 4, true), value);
            }

            // 0xEA / 0xFA LD (nn),A and LD A,(nn) - a full 16-bit address.
            0xEA | 0xFA => {
                let address = self.fetch16(bus);
                self.move_a(bus, address, opcode & 0x10 != 0);
            }

            // 0xE0 / 0xF0 LDH (n),A and LDH A,(n) - one byte names a spot on the FF00 page.
            0xE0 | 0xF0 => {
                let offset = self.fetch8(bus) as u16;
                self.move_a(bus, 0xFF00 + offset, opcode & 0x10 != 0);
            }

            // 0xE2 / 0xF2 LDH (C),A and LDH A,(C) - the same page, addressed by C.
            0xE2 | 0xF2 => {
                let address = 0xFF00 + self.regs.c as u16;
                self.move_a(bus, address, opcode & 0x10 != 0);
            }

            // 0xF8 LD HL,SP+e8 - the only load that touches the flags.
            0xF8 => {
                let offset = self.fetch8(bus);
                self.idle(bus);
                let (result, flags) = alu::add_offset(self.regs.sp, offset);
                self.regs.set_hl(result);
                self.regs.f = flags;
            }

            // 0xE8 ADD SP,e8 - the same sum, kept in SP. One more idle cycle than 0xF8.
            0xE8 => {
                let offset = self.fetch8(bus);
                self.idle(bus);
                self.idle(bus);
                let (result, flags) = alu::add_offset(self.regs.sp, offset);
                self.regs.sp = result;
                self.regs.f = flags;
            }

            // INC rr - no flags change at all, unlike the 8-bit version.
            0x03 | 0x13 | 0x23 | 0x33 => {
                let register = pair(opcode >> 4, false);
                let value = self.regs.read16(register);
                self.idle(bus);
                self.regs.write16(register, value.wrapping_add(1));
            }

            // DEC rr
            0x0B | 0x1B | 0x2B | 0x3B => {
                let register = pair(opcode >> 4, false);
                let value = self.regs.read16(register);
                self.idle(bus);
                self.regs.write16(register, value.wrapping_sub(1));
            }

            // ADD HL,rr - H and C come from bits 11 and 15, and Z is left alone.
            0x09 | 0x19 | 0x29 | 0x39 => {
                let value = self.regs.read16(pair(opcode >> 4, false));
                self.idle(bus);
                let (result, flags) = alu::add_wide(self.regs.hl(), value, self.regs.f.z);
                self.regs.set_hl(result);
                self.regs.f = flags;
            }

            // LD A,(rr) and LD (rr),A - A moves to or from the byte a pair points at.
            0x02 | 0x12 | 0x22 | 0x32 | 0x0A | 0x1A | 0x2A | 0x3A => {
                let address = self.pointer(opcode);
                if opcode & 0x08 == 0 {
                    let value = self.regs.a;
                    self.write8(bus, address, value);
                } else {
                    self.regs.a = self.read8(bus, address);
                }
            }

            // LD r,n - the byte after the opcode goes straight into a register.
            0x06 | 0x0E | 0x16 | 0x1E | 0x26 | 0x2E | 0x36 | 0x3E => {
                let value = self.fetch8(bus);
                self.write_operand(bus, opcode >> 3, value);
            }

            // 0x40-0x7F LD r,r' - both operands come from the opcode's own bits.
            0x40..=0x7F => {
                let value = self.read_operand(bus, opcode);
                self.write_operand(bus, opcode >> 3, value);
            }

            // 0x80-0xBF - eight sums on A, each against a register or the byte HL points at.
            0x80..=0xBF => {
                let value = self.read_operand(bus, opcode);
                self.alu(opcode >> 3, value);
            }

            // INC r - bits 3 to 5 name the register, as in the LD block.
            0x04 | 0x0C | 0x14 | 0x1C | 0x24 | 0x2C | 0x34 | 0x3C => {
                let value = self.read_operand(bus, opcode >> 3);
                let (result, flags) = alu::inc(value, self.regs.f.c);
                self.regs.f = flags;
                self.write_operand(bus, opcode >> 3, result);
            }

            // DEC r
            0x05 | 0x0D | 0x15 | 0x1D | 0x25 | 0x2D | 0x35 | 0x3D => {
                let value = self.read_operand(bus, opcode >> 3);
                let (result, flags) = alu::dec(value, self.regs.f.c);
                self.regs.f = flags;
                self.write_operand(bus, opcode >> 3, result);
            }

            // RLCA RRCA RLA RRA - the first four CB rotates, on A only, and Z always stays off.
            0x07 | 0x0F | 0x17 | 0x1F => {
                let (result, mut flags) = alu::shift(opcode >> 3, self.regs.a, self.regs.f.c);
                flags.z = false;
                self.regs.a = result;
                self.regs.f = flags;
            }

            // 0x27 DAA - turn the last sum back into two decimal digits.
            0x27 => {
                let (result, flags) = alu::daa(self.regs.a, self.regs.f);
                self.regs.a = result;
                self.regs.f = flags;
            }

            // 0x2F CPL - flip every bit of A.
            0x2F => {
                self.regs.a = !self.regs.a;
                self.regs.f.n = true;
                self.regs.f.h = true;
            }

            // 0x37 SCF - set the carry flag.
            0x37 => {
                self.regs.f.n = false;
                self.regs.f.h = false;
                self.regs.f.c = true;
            }

            // 0x3F CCF - flip the carry flag.
            0x3F => {
                self.regs.f.n = false;
                self.regs.f.h = false;
                self.regs.f.c = !self.regs.f.c;
            }

            // ADD A,n through CP n - the same eight sums, against the byte after the opcode.
            0xC6 | 0xCE | 0xD6 | 0xDE | 0xE6 | 0xEE | 0xF6 | 0xFE => {
                let value = self.fetch8(bus);
                self.alu(opcode >> 3, value);
            }

            0xCB => {
                let cb_opcode = self.fetch8(bus);
                self.execute_cb(cb_opcode, bus);
            }

            // Only the 11 empty slots land here. The real chip locks up on them.
            _ => todo!(
                "opcode {opcode:#04X} at pc {:#06X}",
                self.regs.pc.wrapping_sub(1)
            ),
        }
    }

    // Read a signed byte and, if asked, move PC by it, counting from the next instruction.
    fn jump_relative(&mut self, bus: &mut Bus, take: bool) {
        let offset = self.fetch8(bus) as i8;
        if take {
            self.idle(bus);
            self.regs.pc = self.regs.pc.wrapping_add(offset as u16);
        }
    }

    // Read an address and, if asked, push the way back and jump there.
    fn call(&mut self, bus: &mut Bus, take: bool) {
        let address = self.fetch16(bus);
        if take {
            self.idle(bus);
            let back = self.regs.pc; // already past the address, so this is the next instruction
            self.push16(bus, back);
            self.regs.pc = address;
        }
    }

    // Take the saved address off the stack and jump back to it.
    fn ret(&mut self, bus: &mut Bus) {
        let address = self.pop16(bus);
        self.idle(bus);
        self.regs.pc = address;
    }

    // Bits 3 and 4 of the opcode name the test: NZ Z NC C.
    fn condition(&self, opcode: u8) -> bool {
        match (opcode >> 3) & 0x03 {
            0 => !self.regs.f.z,
            1 => self.regs.f.z,
            2 => !self.regs.f.c,
            _ => self.regs.f.c,
        }
    }

    // Move A to or from one address. Callers pick the direction from bit 4 of the opcode.
    fn move_a(&mut self, bus: &mut Bus, address: u16, into_a: bool) {
        if into_a {
            self.regs.a = self.read8(bus, address);
        } else {
            let value = self.regs.a;
            self.write8(bus, address, value);
        }
    }

    // The pair named by bits 4 and 5. HL steps after being used, never before.
    fn pointer(&mut self, opcode: u8) -> u16 {
        let hl = self.regs.hl();
        match (opcode >> 4) & 0x03 {
            0 => self.regs.bc(),
            1 => self.regs.de(),
            2 => {
                self.regs.set_hl(hl.wrapping_add(1));
                hl
            }
            _ => {
                self.regs.set_hl(hl.wrapping_sub(1));
                hl
            }
        }
    }

    // Reading a register is free; reading through HL costs an M-cycle.
    fn read_operand(&mut self, bus: &mut Bus, bits: u8) -> u8 {
        match operand(bits) {
            Some(register) => self.regs.read8(register),
            None => {
                let address = self.regs.hl();
                self.read8(bus, address)
            }
        }
    }

    // Writing a register is free; writing through HL costs an M-cycle.
    fn write_operand(&mut self, bus: &mut Bus, bits: u8, value: u8) {
        match operand(bits) {
            Some(register) => self.regs.write8(register, value),
            None => {
                let address = self.regs.hl();
                self.write8(bus, address, value);
            }
        }
    }

    // CB opcodes split into two bits of kind, three bits of bit number, three bits of target.
    fn execute_cb(&mut self, opcode: u8, bus: &mut Bus) {
        let bit = 1 << ((opcode >> 3) & 0x07);
        match opcode >> 6 {
            // BIT b,r - Z says whether that bit is off. Nothing is written back.
            1 => {
                let value = self.read_operand(bus, opcode);
                self.regs.f.z = value & bit == 0;
                self.regs.f.n = false;
                self.regs.f.h = true;
            }

            // RES b,r - turn one bit off.
            2 => {
                let value = self.read_operand(bus, opcode);
                self.write_operand(bus, opcode, value & !bit);
            }

            // SET b,r - turn one bit on.
            3 => {
                let value = self.read_operand(bus, opcode);
                self.write_operand(bus, opcode, value | bit);
            }

            // Rotates and shifts - bits 3 to 5 pick which of the eight.
            _ => {
                let value = self.read_operand(bus, opcode);
                let (result, flags) = alu::shift(opcode >> 3, value, self.regs.f.c);
                self.regs.f = flags;
                self.write_operand(bus, opcode, result);
            }
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/cpu/exec.rs"]
mod tests;
