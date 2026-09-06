//! One arm per opcode, each doing the same steps the real chip does, so timing falls out for free.
//!
//! Opcode reference: <https://gbdev.io/gb-opcodes/optables/>

use super::Cpu;
use super::registers::Reg8;
use crate::bus::Bus;

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
