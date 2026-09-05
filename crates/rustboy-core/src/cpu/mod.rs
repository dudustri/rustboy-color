//! The SM83 processor. Each memory access ticks the rest of the machine first, as hardware does.

pub mod exec;
pub mod registers;

use crate::bus::Bus;
use registers::Registers;

pub struct Cpu {
    pub regs: Registers,   // the registers on the chip itself: a, f, b to l, sp, pc
    pub ime: bool,         // master switch: while false the CPU ignores every interrupt
    pub ime_pending: bool, // EI only takes effect after the next instruction
    pub halted: bool,      // asleep until an interrupt arrives
    pub stopped: bool,     // deeper sleep, also how the CGB changes speed
}

impl Cpu {
    pub fn new() -> Self {
        Self {
            regs: Registers::post_boot_cgb(),
            ime: false,
            ime_pending: false,
            halted: false,
            stopped: false,
        }
    }

    pub fn step(&mut self, bus: &mut Bus) {
        if self.service_interrupt(bus) {
            return;
        }
        if self.halted {
            bus.tick(4);
            return;
        }
        let ime_was_pending = self.ime_pending;
        let opcode = self.fetch8(bus);
        self.execute(opcode, bus);
        if ime_was_pending {
            self.ime = true;
            self.ime_pending = false;
        }
    }

    /// Jumping to a handler costs 5 M-cycles: 2 waiting, 2 saving where we were, 1 jumping.
    fn service_interrupt(&mut self, bus: &mut Bus) -> bool {
        let pending = bus.interrupt_flag & bus.interrupt_enable & 0x1F;
        if pending == 0 {
            return false;
        }
        // A waiting interrupt wakes the CPU up even when interrupts are switched off.
        self.halted = false;
        if !self.ime {
            return false;
        }
        self.ime = false;

        let index = pending.trailing_zeros() as u8;
        bus.interrupt_flag &= !(1 << index);

        self.idle(bus);
        self.idle(bus);
        let pc = self.regs.pc;
        self.push16(bus, pc);
        self.idle(bus);
        self.regs.pc = 0x0040 + 0x08 * index as u16;
        true
    }

    pub(crate) fn read8(&mut self, bus: &mut Bus, addr: u16) -> u8 {
        bus.tick(4);
        bus.read(addr)
    }

    pub(crate) fn write8(&mut self, bus: &mut Bus, addr: u16, value: u8) {
        bus.tick(4);
        bus.write(addr, value);
    }

    /// One M-cycle where the CPU thinks instead of touching memory.
    pub(crate) fn idle(&mut self, bus: &mut Bus) {
        bus.tick(4);
    }

    pub(crate) fn fetch8(&mut self, bus: &mut Bus) -> u8 {
        let addr = self.regs.pc;
        self.regs.pc = addr.wrapping_add(1);
        self.read8(bus, addr)
    }

    pub(crate) fn fetch16(&mut self, bus: &mut Bus) -> u16 {
        let low = self.fetch8(bus) as u16;
        let high = self.fetch8(bus) as u16;
        (high << 8) | low
    }

    pub(crate) fn push16(&mut self, bus: &mut Bus, value: u16) {
        self.regs.sp = self.regs.sp.wrapping_sub(1);
        self.write8(bus, self.regs.sp, (value >> 8) as u8);
        self.regs.sp = self.regs.sp.wrapping_sub(1);
        self.write8(bus, self.regs.sp, value as u8);
    }

    #[allow(dead_code, reason = "TODO(PR-09): used by RET and POP")]
    pub(crate) fn pop16(&mut self, bus: &mut Bus) -> u16 {
        let low = self.read8(bus, self.regs.sp) as u16;
        self.regs.sp = self.regs.sp.wrapping_add(1);
        let high = self.read8(bus, self.regs.sp) as u16;
        self.regs.sp = self.regs.sp.wrapping_add(1);
        (high << 8) | low
    }
}

impl Default for Cpu {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_memory_access_costs_one_m_cycle() {
        let mut cpu = Cpu::new();
        let mut bus = Bus::testing();
        let before = bus.cycles();
        cpu.read8(&mut bus, 0xC000);
        assert_eq!(bus.cycles() - before, 4);
    }

    #[test]
    fn push_then_pop_round_trips() {
        let mut cpu = Cpu::new();
        let mut bus = Bus::testing();
        cpu.regs.sp = 0xDFF0;
        cpu.push16(&mut bus, 0xBEEF);
        assert_eq!(cpu.regs.sp, 0xDFEE);
        assert_eq!(cpu.pop16(&mut bus), 0xBEEF);
        assert_eq!(cpu.regs.sp, 0xDFF0);
    }

    #[test]
    fn nop_costs_four_cycles() {
        let mut cpu = Cpu::new();
        let mut bus = Bus::testing();
        bus.write(0xC000, 0x00);
        cpu.regs.pc = 0xC000;
        let before = bus.cycles();
        cpu.step(&mut bus);
        assert_eq!(bus.cycles() - before, 4);
        assert_eq!(cpu.regs.pc, 0xC001);
    }
}
