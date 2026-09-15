//! The SM83 processor. Each memory access ticks the rest of the machine first, as hardware does.

mod alu;
pub mod exec;
pub mod registers;

use crate::bus::Bus;
use registers::Registers;

pub struct Cpu {
    pub regs: Registers,   // the registers on the chip itself: a, f, b to l, sp, pc
    pub ime: bool,         // master switch: while false the CPU ignores every interrupt
    pub ime_pending: bool, // EI only takes effect after the next instruction
    pub halted: bool,      // asleep until an interrupt arrives
    pub stopped: bool,     // deep sleep after STOP, until a button is pressed
    pub halt_bug: bool,    // the next fetch forgets to move PC on
}

impl Cpu {
    pub fn new() -> Self {
        Self {
            regs: Registers::post_boot_cgb(),
            ime: false,
            ime_pending: false,
            halted: false,
            stopped: false,
            halt_bug: false,
        }
    }

    pub fn step(&mut self, bus: &mut Bus) {
        if self.service_interrupt(bus) {
            return;
        }
        if self.stopped && bus.joypad.any_held() {
            self.stopped = false;
            bus.timer.freeze(false);
        }
        // The screen keeps going while asleep, so the host still gets frames.
        if self.halted || self.stopped {
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

    /// Answer a waiting interrupt: save PC, then jump to that interrupt's address. 5 M-cycles.
    fn service_interrupt(&mut self, bus: &mut Bus) -> bool {
        if bus.interrupt_flag & bus.interrupt_enable & 0x1F == 0 {
            return false;
        }
        // A waiting interrupt wakes the CPU up even when interrupts are switched off.
        self.halted = false;
        if !self.ime {
            return false;
        }
        self.ime = false;

        self.idle(bus);
        self.idle(bus);
        let pc = self.regs.pc;
        self.regs.sp = self.regs.sp.wrapping_sub(1);
        self.write8(bus, self.regs.sp, (pc >> 8) as u8);

        // Only chosen now, because that write can land on IE and cancel the interrupt.
        let pending = bus.interrupt_flag & bus.interrupt_enable & 0x1F;

        self.regs.sp = self.regs.sp.wrapping_sub(1);
        self.write8(bus, self.regs.sp, pc as u8);
        self.idle(bus);

        self.regs.pc = match pending {
            0 => 0x0000, // cancelled halfway, so it lands at the very start of memory
            _ => {
                let index = pending.trailing_zeros();
                bus.interrupt_flag &= !(1 << index);
                0x0040 + 0x08 * index as u16
            }
        };
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
        if self.halt_bug {
            self.halt_bug = false; // PC stays put this once, so the same byte is read again
        } else {
            self.regs.pc = addr.wrapping_add(1);
        }
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
#[path = "../../tests/unit/cpu.rs"]
mod tests;
