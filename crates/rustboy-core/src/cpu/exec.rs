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
        _ => Some(Reg8::A), // because & 0x07
    }
}

impl Cpu {
    pub(crate) fn execute(&mut self, opcode: u8, bus: &mut Bus) {
        match opcode {
            // 0x00 NOP
            0x00 => {}

            // 0x76 HALT
            // TODO(PR-11): the HALT bug - interrupts off with one waiting must not advance PC.
            0x76 => self.halted = true,

            // 0xC3 JP a16
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

            // 0xF3 DI
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

            // ADD HL,rr
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

            // TODO(PR-07..10): the other 240 opcodes.
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

    // Move A to or from one address. Bit 4 of the opcode picks the direction.
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
mod tests {
    use super::*;
    use crate::cpu::registers::Flags;

    // Put a known value in every register, and point HL at writable memory.
    fn loaded_cpu() -> (Cpu, Bus) {
        let mut cpu = Cpu::new();
        let bus = Bus::testing();
        cpu.regs.b = 0x11;
        cpu.regs.c = 0x22;
        cpu.regs.d = 0x33;
        cpu.regs.e = 0x44;
        cpu.regs.a = 0x77;
        cpu.regs.set_hl(0xC000);
        (cpu, bus)
    }

    fn run(cpu: &mut Cpu, bus: &mut Bus, opcode: u8) -> u64 {
        bus.write(0xD000, opcode);
        cpu.regs.pc = 0xD000;
        let before = bus.cycles();
        cpu.step(bus);
        bus.cycles() - before
    }

    #[test]
    fn ld_between_registers_takes_one_m_cycle() {
        let (mut cpu, mut bus) = loaded_cpu();
        assert_eq!(run(&mut cpu, &mut bus, 0x41), 4); // LD B,C
        assert_eq!(cpu.regs.b, 0x22);
    }

    #[test]
    fn ld_through_hl_costs_an_extra_m_cycle() {
        let (mut cpu, mut bus) = loaded_cpu();
        bus.write(0xC000, 0x99);
        assert_eq!(run(&mut cpu, &mut bus, 0x46), 8); // LD B,(HL)
        assert_eq!(cpu.regs.b, 0x99);

        let (mut cpu, mut bus) = loaded_cpu();
        assert_eq!(run(&mut cpu, &mut bus, 0x70), 8); // LD (HL),B
        assert_eq!(bus.read(0xC000), 0x11);
    }

    // Every one of the 63 forms must move the right byte and cost the right time.
    #[test]
    fn the_whole_ld_block_works() {
        for opcode in 0x40..=0x7Fu8 {
            if opcode == 0x76 {
                continue; // that slot is HALT, not a load
            }
            let (mut cpu, mut bus) = loaded_cpu();
            bus.write(0xC000, 0x99);

            let destination = (opcode >> 3) & 0x07;
            let source = opcode & 0x07;
            let expected = match operand(source) {
                Some(register) => cpu.regs.read8(register),
                None => 0x99,
            };
            let through_memory = destination == 6 || source == 6;

            let cycles = run(&mut cpu, &mut bus, opcode);
            assert_eq!(cycles, if through_memory { 8 } else { 4 }, "{opcode:#04X}");

            let actual = match operand(destination) {
                Some(register) => cpu.regs.read8(register),
                None => bus.read(cpu.regs.hl()),
            };
            assert_eq!(actual, expected, "{opcode:#04X}");
        }
    }

    // Only index six may mean memory; anything above must wrap, not fall through.
    #[test]
    fn only_six_means_memory() {
        for bits in 0..=0xFFu8 {
            assert_eq!(operand(bits).is_none(), bits & 0x07 == 6, "{bits:#04X}");
        }
        assert_eq!(operand(0x08), Some(Reg8::B)); // high bits ignored
        assert_eq!(operand(0x0F), Some(Reg8::A));
    }

    // Every immediate load must take its byte and cost the right time.
    #[test]
    fn immediate_loads_work() {
        for destination in 0..=7u8 {
            let opcode = 0x06 | (destination << 3);
            let (mut cpu, mut bus) = loaded_cpu();
            bus.write(0xD001, 0x5A); // the byte the opcode will pick up

            let cycles = run(&mut cpu, &mut bus, opcode);
            let through_memory = destination == 6;
            assert_eq!(cycles, if through_memory { 12 } else { 8 }, "{opcode:#04X}");

            let actual = match operand(destination) {
                Some(register) => cpu.regs.read8(register),
                None => bus.read(cpu.regs.hl()),
            };
            assert_eq!(actual, 0x5A, "{opcode:#04X}");
            assert_eq!(cpu.regs.pc, 0xD002, "{opcode:#04X}");
        }
    }

    // All eight forms move A through a pair, and cost one fetch plus one access.
    #[test]
    fn indirect_loads_work() {
        for opcode in [0x02, 0x12, 0x22, 0x32, 0x0A, 0x1A, 0x2A, 0x3A] {
            let (mut cpu, mut bus) = loaded_cpu();
            cpu.regs.set_bc(0xC010);
            cpu.regs.set_de(0xC020);
            cpu.regs.set_hl(0xC030);
            bus.write(0xC010, 0xB1);
            bus.write(0xC020, 0xB2);
            bus.write(0xC030, 0xB3);

            let (address, stored) = match opcode & 0x30 {
                0x00 => (0xC010, 0xB1),
                0x10 => (0xC020, 0xB2),
                _ => (0xC030, 0xB3),
            };

            let cycles = run(&mut cpu, &mut bus, opcode);
            assert_eq!(cycles, 8, "{opcode:#04X}");

            if opcode & 0x08 == 0 {
                assert_eq!(bus.read(address), 0x77, "{opcode:#04X}"); // A was stored
            } else {
                assert_eq!(cpu.regs.a, stored, "{opcode:#04X}");
            }
        }
    }

    #[test]
    fn hl_steps_after_the_access_not_before() {
        let (mut cpu, mut bus) = loaded_cpu();
        cpu.regs.set_hl(0xC030);
        bus.write(0xC030, 0xB3);
        run(&mut cpu, &mut bus, 0x2A); // LD A,(HL+)
        assert_eq!(cpu.regs.a, 0xB3); // read the old address
        assert_eq!(cpu.regs.hl(), 0xC031);

        let (mut cpu, mut bus) = loaded_cpu();
        cpu.regs.set_hl(0xC030);
        run(&mut cpu, &mut bus, 0x32); // LD (HL-),A
        assert_eq!(bus.read(0xC030), 0x77);
        assert_eq!(cpu.regs.hl(), 0xC02F);
    }

    #[test]
    fn sixteen_bit_immediates_work() {
        for (opcode, register) in [
            (0x01, Reg16::BC),
            (0x11, Reg16::DE),
            (0x21, Reg16::HL),
            (0x31, Reg16::SP),
        ] {
            let (mut cpu, mut bus) = loaded_cpu();
            bus.write(0xD001, 0x34); // low byte first, as the chip stores them
            bus.write(0xD002, 0x12);
            assert_eq!(run(&mut cpu, &mut bus, opcode), 12, "{opcode:#04X}");
            assert_eq!(cpu.regs.read16(register), 0x1234, "{opcode:#04X}");
            assert_eq!(cpu.regs.pc, 0xD003, "{opcode:#04X}");
        }
    }

    #[test]
    fn the_stack_pointer_can_be_written_to_memory() {
        let (mut cpu, mut bus) = loaded_cpu();
        cpu.regs.sp = 0xBEEF;
        bus.write(0xD001, 0x00);
        bus.write(0xD002, 0xC1);
        assert_eq!(run(&mut cpu, &mut bus, 0x08), 20); // LD (C100),SP
        assert_eq!(bus.read(0xC100), 0xEF);
        assert_eq!(bus.read(0xC101), 0xBE);
    }

    #[test]
    fn hl_can_become_the_stack_pointer() {
        let (mut cpu, mut bus) = loaded_cpu();
        cpu.regs.set_hl(0xC0DE);
        assert_eq!(run(&mut cpu, &mut bus, 0xF9), 8); // LD SP,HL
        assert_eq!(cpu.regs.sp, 0xC0DE);
    }

    #[test]
    fn push_and_pop_round_trip_every_pair() {
        for (push, pop, register) in [
            (0xC5, 0xC1, Reg16::BC),
            (0xD5, 0xD1, Reg16::DE),
            (0xE5, 0xE1, Reg16::HL),
            (0xF5, 0xF1, Reg16::AF),
        ] {
            let (mut cpu, mut bus) = loaded_cpu();
            cpu.regs.sp = 0xDFF0;
            let before = cpu.regs.read16(register);

            assert_eq!(run(&mut cpu, &mut bus, push), 16, "{push:#04X}");
            assert_eq!(cpu.regs.sp, 0xDFEE, "{push:#04X}");

            cpu.regs.write16(register, 0x0000);
            assert_eq!(run(&mut cpu, &mut bus, pop), 12, "{pop:#04X}");
            assert_eq!(cpu.regs.read16(register), before, "{pop:#04X}");
            assert_eq!(cpu.regs.sp, 0xDFF0, "{pop:#04X}");
        }
    }

    // The flag register has no low nibble, so popping must not invent one.
    #[test]
    fn popping_af_keeps_the_flag_bits_clean() {
        let (mut cpu, mut bus) = loaded_cpu();
        cpu.regs.sp = 0xDFEE; // pop reads from here upward
        bus.write(0xDFEE, 0xFF);
        bus.write(0xDFEF, 0xFF);
        run(&mut cpu, &mut bus, 0xF1); // POP AF
        assert_eq!(cpu.regs.read16(Reg16::AF), 0xFFF0);
        assert_eq!(cpu.regs.sp, 0xDFF0);
    }

    #[test]
    fn full_address_loads_work() {
        let (mut cpu, mut bus) = loaded_cpu();
        bus.write(0xD001, 0x00);
        bus.write(0xD002, 0xC1);
        assert_eq!(run(&mut cpu, &mut bus, 0xEA), 16); // LD (C100),A
        assert_eq!(bus.read(0xC100), 0x77);

        let (mut cpu, mut bus) = loaded_cpu();
        bus.write(0xD001, 0x00);
        bus.write(0xD002, 0xC1);
        bus.write(0xC100, 0x5E);
        assert_eq!(run(&mut cpu, &mut bus, 0xFA), 16); // LD A,(C100)
        assert_eq!(cpu.regs.a, 0x5E);
    }

    // The FF00 page is where the hardware registers live.
    #[test]
    fn the_high_page_is_reachable_by_one_byte() {
        let (mut cpu, mut bus) = loaded_cpu();
        bus.write(0xD001, 0x80); // FF80, the start of high RAM
        assert_eq!(run(&mut cpu, &mut bus, 0xE0), 12); // LDH (80),A
        assert_eq!(bus.read(0xFF80), 0x77);

        let (mut cpu, mut bus) = loaded_cpu();
        bus.write(0xD001, 0x80);
        bus.write(0xFF80, 0x3C);
        assert_eq!(run(&mut cpu, &mut bus, 0xF0), 12); // LDH A,(80)
        assert_eq!(cpu.regs.a, 0x3C);
    }

    #[test]
    fn c_can_name_a_spot_on_the_high_page() {
        let (mut cpu, mut bus) = loaded_cpu();
        cpu.regs.c = 0x81;
        assert_eq!(run(&mut cpu, &mut bus, 0xE2), 8); // LDH (C),A
        assert_eq!(bus.read(0xFF81), 0x77);

        let (mut cpu, mut bus) = loaded_cpu();
        cpu.regs.c = 0x81;
        bus.write(0xFF81, 0x2D);
        assert_eq!(run(&mut cpu, &mut bus, 0xF2), 8); // LDH A,(C)
        assert_eq!(cpu.regs.a, 0x2D);
    }

    #[test]
    fn adding_to_the_stack_pointer_can_go_backwards() {
        let (mut cpu, mut bus) = loaded_cpu();
        cpu.regs.sp = 0xC100;
        bus.write(0xD001, 0xFF); // -1 as a signed byte
        assert_eq!(run(&mut cpu, &mut bus, 0xF8), 12);
        assert_eq!(cpu.regs.hl(), 0xC0FF);
    }

    // Both carries come from the low bytes, which is the easy part to get wrong.
    #[test]
    fn the_stack_offset_carries_come_from_the_low_byte() {
        let cases = [
            (0xC00Fu16, 0x01u8, true, false),  // low nibble overflows
            (0xC0FFu16, 0x01u8, true, true),   // low byte overflows too
            (0xC000u16, 0x01u8, false, false), // neither
        ];
        for (sp, offset, half, carry) in cases {
            let (mut cpu, mut bus) = loaded_cpu();
            cpu.regs.sp = sp;
            bus.write(0xD001, offset);
            run(&mut cpu, &mut bus, 0xF8);
            assert_eq!(cpu.regs.f.h, half, "{sp:#06X} + {offset:#04X}");
            assert_eq!(cpu.regs.f.c, carry, "{sp:#06X} + {offset:#04X}");
            assert!(!cpu.regs.f.z && !cpu.regs.f.n);
        }
    }

    // Every one of the 64 sums must cost one M-cycle, plus one more through HL.
    #[test]
    fn the_whole_alu_block_has_the_right_timing() {
        for opcode in 0x80..=0xBFu8 {
            let (mut cpu, mut bus) = loaded_cpu();
            let through_memory = opcode & 0x07 == 6;
            let cycles = run(&mut cpu, &mut bus, opcode);
            assert_eq!(cycles, if through_memory { 8 } else { 4 }, "{opcode:#04X}");
        }
    }

    #[test]
    fn sums_use_the_right_operand() {
        let (mut cpu, mut bus) = loaded_cpu();
        run(&mut cpu, &mut bus, 0x80); // ADD A,B
        assert_eq!(cpu.regs.a, 0x77 + 0x11);

        let (mut cpu, mut bus) = loaded_cpu();
        bus.write(0xC000, 0x07);
        run(&mut cpu, &mut bus, 0x96); // SUB (HL)
        assert_eq!(cpu.regs.a, 0x70);
    }

    // An immediate sum must give exactly what the register version gives.
    #[test]
    fn immediate_sums_match_the_register_ones() {
        for kind in 0..=7u8 {
            let immediate = 0xC6 | (kind << 3);
            let register = 0x80 | (kind << 3); // against B

            let (mut by_register, mut bus) = loaded_cpu();
            by_register.regs.f.c = true; // so ADC and SBC have a carry to use
            by_register.regs.b = 0x3C;
            assert_eq!(run(&mut by_register, &mut bus, register), 4);

            let (mut by_immediate, mut bus) = loaded_cpu();
            by_immediate.regs.f.c = true;
            bus.write(0xD001, 0x3C);
            assert_eq!(
                run(&mut by_immediate, &mut bus, immediate),
                8,
                "{immediate:#04X}"
            );

            assert_eq!(by_immediate.regs.a, by_register.regs.a, "{immediate:#04X}");
            assert_eq!(by_immediate.regs.f, by_register.regs.f, "{immediate:#04X}");
            assert_eq!(by_immediate.regs.pc, 0xD002, "{immediate:#04X}");
        }
    }

    // Registers cost one M-cycle; through HL it reads and then writes, so three.
    #[test]
    fn inc_and_dec_work_on_every_target() {
        for target in 0..=7u8 {
            for (base, step) in [(0x04u8, 1u8), (0x05, 0xFF)] {
                let opcode = base | (target << 3);
                let (mut cpu, mut bus) = loaded_cpu();
                bus.write(0xC000, 0x99);
                cpu.regs.f.c = true;

                let before = match operand(target) {
                    Some(register) => cpu.regs.read8(register),
                    None => 0x99,
                };
                let cycles = run(&mut cpu, &mut bus, opcode);
                assert_eq!(cycles, if target == 6 { 12 } else { 4 }, "{opcode:#04X}");

                let after = match operand(target) {
                    Some(register) => cpu.regs.read8(register),
                    None => bus.read(cpu.regs.hl()),
                };
                assert_eq!(after, before.wrapping_add(step), "{opcode:#04X}");
                assert!(cpu.regs.f.c, "{opcode:#04X} must keep the carry");
            }
        }
    }

    // Counting a pair up or down is invisible to the flags.
    #[test]
    fn wide_inc_and_dec_touch_no_flags() {
        for register in [Reg16::BC, Reg16::DE, Reg16::HL, Reg16::SP] {
            let bits = match register {
                Reg16::BC => 0x00,
                Reg16::DE => 0x10,
                Reg16::HL => 0x20,
                _ => 0x30,
            };
            for (opcode, step) in [(0x03 | bits, 1u16), (0x0B | bits, 0xFFFF)] {
                let (mut cpu, mut bus) = loaded_cpu();
                cpu.regs.write16(register, 0xFFFF);
                cpu.regs.f = Flags::from_bits(0xF0);

                assert_eq!(run(&mut cpu, &mut bus, opcode), 8, "{opcode:#04X}");
                assert_eq!(cpu.regs.read16(register), 0xFFFFu16.wrapping_add(step));
                assert_eq!(
                    cpu.regs.f.bits(),
                    0xF0,
                    "{opcode:#04X} must keep every flag"
                );
            }
        }
    }

    #[test]
    fn add_hl_works_for_every_pair() {
        for (opcode, register) in [(0x09, Reg16::BC), (0x19, Reg16::DE), (0x39, Reg16::SP)] {
            let (mut cpu, mut bus) = loaded_cpu();
            cpu.regs.set_hl(0x1000);
            cpu.regs.write16(register, 0x0234);
            assert_eq!(run(&mut cpu, &mut bus, opcode), 8, "{opcode:#04X}");
            assert_eq!(cpu.regs.hl(), 0x1234, "{opcode:#04X}");
        }

        let (mut cpu, mut bus) = loaded_cpu();
        cpu.regs.set_hl(0x1234);
        run(&mut cpu, &mut bus, 0x29); // ADD HL,HL doubles it
        assert_eq!(cpu.regs.hl(), 0x2468);
    }

    #[test]
    fn add_sp_moves_the_stack_and_costs_two_idle_cycles() {
        let (mut cpu, mut bus) = loaded_cpu();
        cpu.regs.sp = 0xC100;
        bus.write(0xD001, 0xFE); // -2
        assert_eq!(run(&mut cpu, &mut bus, 0xE8), 16);
        assert_eq!(cpu.regs.sp, 0xC0FE);
        assert!(!cpu.regs.f.z && !cpu.regs.f.n);
    }

    #[test]
    fn cpl_flips_a_and_keeps_z_and_c() {
        let (mut cpu, mut bus) = loaded_cpu();
        cpu.regs.a = 0b1010_0101;
        cpu.regs.f = Flags::from_bits(0x90); // Z and C on
        assert_eq!(run(&mut cpu, &mut bus, 0x2F), 4);
        assert_eq!(cpu.regs.a, 0b0101_1010);
        assert_eq!(cpu.regs.f.bits(), 0xF0);
    }

    #[test]
    fn scf_and_ccf_only_touch_the_carry_side() {
        let (mut cpu, mut bus) = loaded_cpu();
        cpu.regs.f = Flags::from_bits(0xE0); // Z N H on, C off
        assert_eq!(run(&mut cpu, &mut bus, 0x37), 4); // SCF
        assert_eq!(cpu.regs.f.bits(), 0x90); // Z kept, C set, N and H cleared

        assert_eq!(run(&mut cpu, &mut bus, 0x3F), 4); // CCF
        assert_eq!(cpu.regs.f.bits(), 0x80); // C flipped off
        run(&mut cpu, &mut bus, 0x3F);
        assert_eq!(cpu.regs.f.bits(), 0x90); // and back on
    }

    // A score of 19 plus 1 must read 20 on screen, not 1A.
    #[test]
    fn daa_after_add_gives_a_decimal_score() {
        let (mut cpu, mut bus) = loaded_cpu();
        cpu.regs.a = 0x19;
        cpu.regs.b = 0x01;
        run(&mut cpu, &mut bus, 0x80); // ADD A,B
        assert_eq!(cpu.regs.a, 0x1A);
        assert_eq!(run(&mut cpu, &mut bus, 0x27), 4); // DAA
        assert_eq!(cpu.regs.a, 0x20);
    }

    // Each test flag on and off: a jump that happens costs 16, one that does not costs 12.
    #[test]
    fn conditional_jumps_follow_their_flag() {
        for (opcode, z, c) in [
            (0xC2u8, false, false), // NZ
            (0xCA, true, false),    // Z
            (0xD2, false, false),   // NC
            (0xDA, false, true),    // C
        ] {
            for passes in [true, false] {
                let (mut cpu, mut bus) = loaded_cpu();
                cpu.regs.f.z = if passes { z } else { !z };
                cpu.regs.f.c = if passes { c } else { !c };
                if opcode == 0xC2 || opcode == 0xCA {
                    cpu.regs.f.c = false; // only Z matters here
                } else {
                    cpu.regs.f.z = false; // only C matters here
                }
                bus.write(0xD001, 0x34);
                bus.write(0xD002, 0x12);

                let cycles = run(&mut cpu, &mut bus, opcode);
                if passes {
                    assert_eq!(cycles, 16, "{opcode:#04X} taken");
                    assert_eq!(cpu.regs.pc, 0x1234, "{opcode:#04X} taken");
                } else {
                    assert_eq!(cycles, 12, "{opcode:#04X} skipped");
                    assert_eq!(cpu.regs.pc, 0xD003, "{opcode:#04X} skipped");
                }
            }
        }
    }

    #[test]
    fn jr_counts_from_the_next_instruction() {
        let (mut cpu, mut bus) = loaded_cpu();
        bus.write(0xD001, 0x05);
        assert_eq!(run(&mut cpu, &mut bus, 0x18), 12);
        assert_eq!(cpu.regs.pc, 0xD007); // 0xD002 plus 5
    }

    // Minus two lands back on the JR itself, the usual way to wait forever.
    #[test]
    fn jr_can_jump_backwards() {
        let (mut cpu, mut bus) = loaded_cpu();
        bus.write(0xD001, 0xFE);
        run(&mut cpu, &mut bus, 0x18);
        assert_eq!(cpu.regs.pc, 0xD000);
    }

    #[test]
    fn conditional_jr_follows_its_flag() {
        for (opcode, flag_is_z, wants_on) in [
            (0x20u8, true, false), // NZ
            (0x28, true, true),    // Z
            (0x30, false, false),  // NC
            (0x38, false, true),   // C
        ] {
            for passes in [true, false] {
                let (mut cpu, mut bus) = loaded_cpu();
                let on = if passes { wants_on } else { !wants_on };
                cpu.regs.f.z = flag_is_z && on;
                cpu.regs.f.c = !flag_is_z && on;
                bus.write(0xD001, 0x10);

                let cycles = run(&mut cpu, &mut bus, opcode);
                if passes {
                    assert_eq!(cycles, 12, "{opcode:#04X} taken");
                    assert_eq!(cpu.regs.pc, 0xD012, "{opcode:#04X} taken");
                } else {
                    assert_eq!(cycles, 8, "{opcode:#04X} skipped");
                    assert_eq!(cpu.regs.pc, 0xD002, "{opcode:#04X} skipped");
                }
            }
        }
    }

    // The return address is the instruction after the CALL, saved low byte on top.
    #[test]
    fn call_saves_the_way_back_and_jumps() {
        let (mut cpu, mut bus) = loaded_cpu();
        cpu.regs.sp = 0xDFF0;
        bus.write(0xD001, 0x34);
        bus.write(0xD002, 0x12);

        assert_eq!(run(&mut cpu, &mut bus, 0xCD), 24);
        assert_eq!(cpu.regs.pc, 0x1234);
        assert_eq!(cpu.regs.sp, 0xDFEE);
        assert_eq!(bus.read(0xDFEE), 0x03);
        assert_eq!(bus.read(0xDFEF), 0xD0);
    }

    #[test]
    fn conditional_call_follows_its_flag() {
        for (opcode, flag_is_z, wants_on) in [
            (0xC4u8, true, false), // NZ
            (0xCC, true, true),    // Z
            (0xD4, false, false),  // NC
            (0xDC, false, true),   // C
        ] {
            for passes in [true, false] {
                let (mut cpu, mut bus) = loaded_cpu();
                cpu.regs.sp = 0xDFF0;
                let on = if passes { wants_on } else { !wants_on };
                cpu.regs.f.z = flag_is_z && on;
                cpu.regs.f.c = !flag_is_z && on;
                bus.write(0xD001, 0x34);
                bus.write(0xD002, 0x12);

                let cycles = run(&mut cpu, &mut bus, opcode);
                if passes {
                    assert_eq!(cycles, 24, "{opcode:#04X} taken");
                    assert_eq!(cpu.regs.pc, 0x1234, "{opcode:#04X} taken");
                    assert_eq!(cpu.regs.sp, 0xDFEE, "{opcode:#04X} taken");
                } else {
                    assert_eq!(cycles, 12, "{opcode:#04X} skipped");
                    assert_eq!(cpu.regs.pc, 0xD003, "{opcode:#04X} skipped");
                    assert_eq!(cpu.regs.sp, 0xDFF0, "{opcode:#04X} must not push");
                }
            }
        }
    }

    // A CALL followed by a RET must land on the instruction after the CALL.
    #[test]
    fn ret_comes_back_to_after_the_call() {
        let (mut cpu, mut bus) = loaded_cpu();
        cpu.regs.sp = 0xDFF0;
        bus.write(0xD001, 0x34);
        bus.write(0xD002, 0x12);
        bus.write(0x1234, 0xC9); // RET waiting at the destination

        run(&mut cpu, &mut bus, 0xCD); // CALL 1234
        let before = bus.cycles();
        cpu.step(&mut bus);

        assert_eq!(bus.cycles() - before, 16);
        assert_eq!(cpu.regs.pc, 0xD003);
        assert_eq!(cpu.regs.sp, 0xDFF0);
    }

    #[test]
    fn conditional_ret_follows_its_flag() {
        for (opcode, flag_is_z, wants_on) in [
            (0xC0u8, true, false), // NZ
            (0xC8, true, true),    // Z
            (0xD0, false, false),  // NC
            (0xD8, false, true),   // C
        ] {
            for passes in [true, false] {
                let (mut cpu, mut bus) = loaded_cpu();
                cpu.regs.sp = 0xDFEE;
                bus.write(0xDFEE, 0x78); // a saved address of 0x5678
                bus.write(0xDFEF, 0x56);
                let on = if passes { wants_on } else { !wants_on };
                cpu.regs.f.z = flag_is_z && on;
                cpu.regs.f.c = !flag_is_z && on;

                let cycles = run(&mut cpu, &mut bus, opcode);
                if passes {
                    assert_eq!(cycles, 20, "{opcode:#04X} taken");
                    assert_eq!(cpu.regs.pc, 0x5678, "{opcode:#04X} taken");
                    assert_eq!(cpu.regs.sp, 0xDFF0, "{opcode:#04X} taken");
                } else {
                    assert_eq!(cycles, 8, "{opcode:#04X} skipped");
                    assert_eq!(cpu.regs.pc, 0xD001, "{opcode:#04X} skipped");
                    assert_eq!(cpu.regs.sp, 0xDFEE, "{opcode:#04X} must not pop");
                }
            }
        }
    }

    #[test]
    fn reti_returns_and_switches_interrupts_on_at_once() {
        let (mut cpu, mut bus) = loaded_cpu();
        cpu.regs.sp = 0xDFEE;
        bus.write(0xDFEE, 0x78);
        bus.write(0xDFEF, 0x56);
        cpu.ime = false;

        assert_eq!(run(&mut cpu, &mut bus, 0xD9), 16);
        assert_eq!(cpu.regs.pc, 0x5678);
        assert!(cpu.ime, "unlike EI there is no one-instruction wait");
    }

    #[test]
    fn rst_calls_its_fixed_address() {
        for (opcode, target) in [
            (0xC7u8, 0x00u16),
            (0xCF, 0x08),
            (0xD7, 0x10),
            (0xDF, 0x18),
            (0xE7, 0x20),
            (0xEF, 0x28),
            (0xF7, 0x30),
            (0xFF, 0x38),
        ] {
            let (mut cpu, mut bus) = loaded_cpu();
            cpu.regs.sp = 0xDFF0;

            assert_eq!(run(&mut cpu, &mut bus, opcode), 16, "{opcode:#04X}");
            assert_eq!(cpu.regs.pc, target, "{opcode:#04X}");
            assert_eq!(bus.read(0xDFEE), 0x01, "{opcode:#04X} saves 0xD001");
            assert_eq!(bus.read(0xDFEF), 0xD0, "{opcode:#04X} saves 0xD001");
        }
    }

    // Run CB followed by one opcode, and report how long it took.
    fn run_cb(cpu: &mut Cpu, bus: &mut Bus, opcode: u8) -> u64 {
        bus.write(0xD000, 0xCB);
        bus.write(0xD001, opcode);
        cpu.regs.pc = 0xD000;
        let before = bus.cycles();
        cpu.step(bus);
        bus.cycles() - before
    }

    // Put the same pattern in one target, leaving HL pointing at 0xC000.
    fn with_pattern(target: u8, pattern: u8) -> (Cpu, Bus) {
        let (mut cpu, mut bus) = loaded_cpu();
        match operand(target) {
            Some(register) => cpu.regs.write8(register, pattern),
            None => bus.write(0xC000, pattern),
        }
        (cpu, bus)
    }

    fn read_target(cpu: &Cpu, bus: &mut Bus, target: u8) -> u8 {
        match operand(target) {
            Some(register) => cpu.regs.read8(register),
            None => bus.read(0xC000),
        }
    }

    // All 64 BIT opcodes: Z reports the bit, H is on, C is left alone.
    #[test]
    fn bit_tests_every_bit_of_every_target() {
        for opcode in 0x40..=0x7Fu8 {
            let (bit, target) = ((opcode >> 3) & 0x07, opcode & 0x07);
            let (mut cpu, mut bus) = with_pattern(target, 0b1010_0101);
            cpu.regs.f.c = true;

            let cycles = run_cb(&mut cpu, &mut bus, opcode);
            assert_eq!(cycles, if target == 6 { 12 } else { 8 }, "CB {opcode:#04X}");

            let is_on = 0b1010_0101 & (1 << bit) != 0;
            assert_eq!(cpu.regs.f.z, !is_on, "CB {opcode:#04X}");
            assert!(
                cpu.regs.f.h && !cpu.regs.f.n && cpu.regs.f.c,
                "CB {opcode:#04X}"
            );
            assert_eq!(read_target(&cpu, &mut bus, target), 0b1010_0101);
        }
    }

    // All 128 RES and SET opcodes change exactly one bit and no flags.
    #[test]
    fn res_and_set_change_one_bit_and_no_flags() {
        for opcode in 0x80..=0xFFu8 {
            let (bit, target) = ((opcode >> 3) & 0x07, opcode & 0x07);
            let (mut cpu, mut bus) = with_pattern(target, 0b1010_0101);
            cpu.regs.f = Flags::from_bits(0xF0);

            let cycles = run_cb(&mut cpu, &mut bus, opcode);
            assert_eq!(cycles, if target == 6 { 16 } else { 8 }, "CB {opcode:#04X}");

            let expected = if opcode < 0xC0 {
                0b1010_0101 & !(1 << bit)
            } else {
                0b1010_0101 | (1 << bit)
            };
            assert_eq!(
                read_target(&cpu, &mut bus, target),
                expected,
                "CB {opcode:#04X}"
            );
            assert_eq!(cpu.regs.f.bits(), 0xF0, "CB {opcode:#04X}");
        }
    }

    // All 64 must match the pure shift function, on every target, with the right timing.
    #[test]
    fn every_shift_opcode_works_on_every_target() {
        for opcode in 0x00..=0x3Fu8 {
            let target = opcode & 0x07;
            for carry in [false, true] {
                let (mut cpu, mut bus) = with_pattern(target, 0b1001_0110);
                cpu.regs.f.c = carry;

                let cycles = run_cb(&mut cpu, &mut bus, opcode);
                assert_eq!(cycles, if target == 6 { 16 } else { 8 }, "CB {opcode:#04X}");

                let (expected, flags) = alu::shift(opcode >> 3, 0b1001_0110, carry);
                assert_eq!(
                    read_target(&cpu, &mut bus, target),
                    expected,
                    "CB {opcode:#04X}"
                );
                assert_eq!(cpu.regs.f, flags, "CB {opcode:#04X}");
            }
        }
    }

    // The CB versions happen to use the very same numbers after the CB byte.
    #[test]
    fn fast_rotates_match_the_cb_ones_except_for_z() {
        for opcode in [0x07u8, 0x0F, 0x17, 0x1F] {
            for (value, carry) in [(0b1001_0110u8, false), (0b1001_0110, true), (0, false)] {
                let (mut fast, mut bus) = loaded_cpu();
                fast.regs.a = value;
                fast.regs.f.c = carry;
                assert_eq!(run(&mut fast, &mut bus, opcode), 4, "{opcode:#04X}");

                let (mut slow, mut bus) = loaded_cpu();
                slow.regs.a = value;
                slow.regs.f.c = carry;
                assert_eq!(run_cb(&mut slow, &mut bus, opcode), 8, "CB {opcode:#04X}");

                assert_eq!(fast.regs.a, slow.regs.a, "{opcode:#04X}");
                assert_eq!(fast.regs.f.c, slow.regs.f.c, "{opcode:#04X}");
                assert!(!fast.regs.f.z, "{opcode:#04X} must never set Z");
            }
        }
    }

    #[test]
    fn jp_hl_is_a_single_cycle() {
        let (mut cpu, mut bus) = loaded_cpu();
        cpu.regs.set_hl(0x4567);
        assert_eq!(run(&mut cpu, &mut bus, 0xE9), 4);
        assert_eq!(cpu.regs.pc, 0x4567);
    }

    #[test]
    fn ld_a_a_changes_nothing() {
        let (mut cpu, mut bus) = loaded_cpu();
        assert_eq!(run(&mut cpu, &mut bus, 0x7F), 4);
        assert_eq!(cpu.regs.a, 0x77);
    }

    #[test]
    fn halt_still_owns_its_slot() {
        let (mut cpu, mut bus) = loaded_cpu();
        run(&mut cpu, &mut bus, 0x76);
        assert!(cpu.halted);
    }

    #[test]
    fn jp_a16_takes_four_m_cycles() {
        let mut cpu = Cpu::new();
        let mut bus = Bus::testing();
        bus.write(0xC000, 0xC3);
        bus.write(0xC001, 0x34);
        bus.write(0xC002, 0x12);
        cpu.regs.pc = 0xC000;

        let before = bus.cycles();
        cpu.step(&mut bus);

        assert_eq!(cpu.regs.pc, 0x1234);
        assert_eq!(bus.cycles() - before, 16);
    }

    #[test]
    fn ei_is_delayed_by_one_instruction() {
        let mut cpu = Cpu::new();
        let mut bus = Bus::testing();
        bus.write(0xC000, 0xFB); // EI
        bus.write(0xC001, 0x00); // NOP
        cpu.regs.pc = 0xC000;

        cpu.step(&mut bus);
        assert!(!cpu.ime, "EI must not take effect on its own instruction");
        cpu.step(&mut bus);
        assert!(cpu.ime);
    }
}
