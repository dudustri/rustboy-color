//! One arm per opcode, each doing the same steps the real chip does, so timing falls out for free.
//!
//! Opcode reference: <https://gbdev.io/gb-opcodes/optables/>

use super::Cpu;
use super::registers::{Reg8, Reg16};
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

    // TODO(PR-10): the bit instructions - rotates, shifts, SWAP, BIT, RES, SET.
    fn execute_cb(&mut self, opcode: u8, _bus: &mut Bus) {
        todo!("CB opcode {opcode:#04X}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
