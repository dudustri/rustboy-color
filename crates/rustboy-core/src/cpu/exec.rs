//! One arm per opcode, each doing the same steps the real chip does, so timing falls out for free.
//!
//! Opcode reference: <https://gbdev.io/gb-opcodes/optables/>

use super::Cpu;
use crate::bus::Bus;

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

    // TODO(PR-10): the bit instructions - rotates, shifts, SWAP, BIT, RES, SET.
    fn execute_cb(&mut self, opcode: u8, _bus: &mut Bus) {
        todo!("CB opcode {opcode:#04X}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
