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
mod tests {
    use super::*;
    use crate::bus::{IF_JOYPAD, IF_STAT, IF_TIMER, IF_VBLANK};

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

    // A CPU sitting on a NOP at D000, interrupts on, stack at DFF0.
    fn ready() -> (Cpu, Bus) {
        let mut cpu = Cpu::new();
        let mut bus = Bus::testing();
        bus.write(0xD000, 0x00);
        cpu.regs.pc = 0xD000;
        cpu.regs.sp = 0xDFF0;
        cpu.ime = true;
        (cpu, bus)
    }

    #[test]
    fn dispatch_takes_five_m_cycles() {
        let (mut cpu, mut bus) = ready();
        bus.interrupt_enable = IF_VBLANK;
        bus.interrupt_flag = IF_VBLANK;
        let before = bus.cycles();
        cpu.step(&mut bus);
        assert_eq!(bus.cycles() - before, 20);
    }

    #[test]
    fn each_interrupt_has_its_own_address() {
        for index in 0..5u8 {
            let (mut cpu, mut bus) = ready();
            bus.interrupt_enable = 1 << index;
            bus.interrupt_flag = 1 << index;
            cpu.step(&mut bus);

            assert_eq!(
                cpu.regs.pc,
                0x0040 + 0x08 * index as u16,
                "interrupt {index}"
            );
            assert_eq!(
                bus.interrupt_flag, 0,
                "interrupt {index} must clear its own bit"
            );
            assert!(!cpu.ime, "interrupt {index} must switch interrupts off");
            assert_eq!(bus.read(0xDFEE), 0x00, "interrupt {index} saves D000");
            assert_eq!(bus.read(0xDFEF), 0xD0, "interrupt {index} saves D000");
        }
    }

    // With several waiting, the lowest bit goes first and the rest keep waiting.
    #[test]
    fn the_lowest_bit_wins() {
        let (mut cpu, mut bus) = ready();
        bus.interrupt_enable = 0x1F;
        bus.interrupt_flag = IF_TIMER | IF_STAT | IF_JOYPAD;
        cpu.step(&mut bus);
        assert_eq!(cpu.regs.pc, 0x0048);
        assert_eq!(bus.interrupt_flag, IF_TIMER | IF_JOYPAD);
    }

    #[test]
    fn an_interrupt_not_enabled_in_ie_is_ignored() {
        let (mut cpu, mut bus) = ready();
        bus.interrupt_enable = IF_VBLANK;
        bus.interrupt_flag = IF_TIMER;
        cpu.step(&mut bus);
        assert_eq!(cpu.regs.pc, 0xD001); // the NOP ran instead
        assert_eq!(bus.interrupt_flag, IF_TIMER);
    }

    #[test]
    fn nothing_is_answered_while_interrupts_are_off() {
        let (mut cpu, mut bus) = ready();
        cpu.ime = false;
        bus.interrupt_enable = IF_VBLANK;
        bus.interrupt_flag = IF_VBLANK;
        cpu.step(&mut bus);
        assert_eq!(cpu.regs.pc, 0xD001);
        assert_eq!(bus.interrupt_flag, IF_VBLANK, "it stays waiting");
    }

    // The first push lands on IE and switches the timer off halfway, so the CPU ends up at 0000.
    #[test]
    fn pushing_onto_ie_can_cancel_the_interrupt() {
        let (mut cpu, mut bus) = ready();
        cpu.regs.pc = 0x0100;
        cpu.regs.sp = 0x0000;
        bus.interrupt_enable = IF_TIMER;
        bus.interrupt_flag = IF_TIMER;
        cpu.step(&mut bus);
        assert_eq!(cpu.regs.pc, 0x0000);
        assert_eq!(
            bus.interrupt_flag, IF_TIMER,
            "a cancelled interrupt is not cleared"
        );
    }

    // HALT at D000 followed by INC A, with interrupts on.
    fn halting() -> (Cpu, Bus) {
        let (mut cpu, mut bus) = ready();
        bus.write(0xD000, 0x76); // HALT
        bus.write(0xD001, 0x3C); // INC A
        cpu.regs.a = 0;
        (cpu, bus)
    }

    #[test]
    fn halt_sleeps_until_an_interrupt_arrives() {
        let (mut cpu, mut bus) = halting();
        cpu.ime = false;
        cpu.step(&mut bus);
        assert!(cpu.halted);

        for _ in 0..3 {
            let before = bus.cycles();
            cpu.step(&mut bus);
            assert_eq!(bus.cycles() - before, 4, "time keeps passing while asleep");
            assert_eq!(cpu.regs.pc, 0xD001, "nothing runs while asleep");
        }

        bus.interrupt_enable = IF_TIMER;
        bus.interrupt_flag = IF_TIMER;
        cpu.step(&mut bus);
        assert!(!cpu.halted);
        assert_eq!(
            cpu.regs.a, 1,
            "with interrupts off it just carries on after HALT"
        );
    }

    #[test]
    fn halt_with_interrupts_on_answers_the_interrupt() {
        let (mut cpu, mut bus) = halting();
        cpu.step(&mut bus);
        bus.interrupt_enable = IF_VBLANK;
        bus.interrupt_flag = IF_VBLANK;
        cpu.step(&mut bus);
        assert_eq!(cpu.regs.pc, 0x0040);
        assert_eq!(
            bus.read(0xDFEE),
            0x01,
            "it will come back to after the HALT"
        );
    }

    // Interrupts off with one already waiting: HALT does not sleep, and the next byte runs twice.
    #[test]
    fn the_halt_bug_runs_the_next_byte_twice() {
        let (mut cpu, mut bus) = halting();
        cpu.ime = false;
        bus.interrupt_enable = IF_TIMER;
        bus.interrupt_flag = IF_TIMER;

        cpu.step(&mut bus); // HALT
        assert!(!cpu.halted);

        cpu.step(&mut bus); // INC A, but PC does not move on
        assert_eq!((cpu.regs.a, cpu.regs.pc), (1, 0xD001));

        cpu.step(&mut bus); // INC A again, and now PC moves
        assert_eq!((cpu.regs.a, cpu.regs.pc), (2, 0xD002));
    }

    // STOP at D000, the unused byte after it, then INC A.
    fn stopping() -> (Cpu, Bus) {
        let (mut cpu, mut bus) = ready();
        bus.write(0xD000, 0x10);
        bus.write(0xD001, 0x00);
        bus.write(0xD002, 0x3C);
        cpu.regs.a = 0;
        (cpu, bus)
    }

    #[test]
    fn stop_changes_speed_when_a_game_asked_for_it() {
        let (mut cpu, mut bus) = stopping();
        bus.write(0xFF4D, 0x01); // ask for the switch
        cpu.step(&mut bus);

        assert!(bus.double_speed);
        assert_eq!(
            bus.read(0xFF4D),
            0xFE,
            "top bit reports double speed, the request is cleared"
        );
        assert!(!cpu.stopped, "a speed switch does not wait for a button");
        assert_eq!(cpu.regs.pc, 0xD002, "the byte after STOP is skipped");
    }

    #[test]
    fn a_second_switch_goes_back_to_normal_speed() {
        let (mut cpu, mut bus) = stopping();
        bus.double_speed = true;
        bus.write(0xFF4D, 0x01);
        cpu.step(&mut bus);
        assert!(!bus.double_speed);
        assert_eq!(bus.read(0xFF4D), 0x7E);
    }

    #[test]
    fn stop_without_a_request_sleeps_until_a_button() {
        let (mut cpu, mut bus) = stopping();
        cpu.step(&mut bus);
        assert!(cpu.stopped);

        for _ in 0..3 {
            cpu.step(&mut bus);
            assert_eq!(cpu.regs.pc, 0xD002, "nothing runs while stopped");
        }

        bus.joypad.set_button(crate::joypad::Button::Start, true);
        cpu.step(&mut bus);
        assert!(!cpu.stopped);
        assert_eq!(cpu.regs.a, 1, "the next instruction runs after waking");
    }

    #[test]
    fn stop_resets_the_divider() {
        let (mut cpu, mut bus) = stopping();
        bus.tick(0x4000); // let DIV count up well past zero
        assert_ne!(bus.read(0xFF04), 0);
        cpu.step(&mut bus);
        assert_eq!(bus.read(0xFF04), 0);
    }

    #[test]
    fn the_divider_stays_at_zero_until_stop_ends() {
        let (mut cpu, mut bus) = stopping();
        cpu.step(&mut bus);
        for _ in 0..0x400 {
            cpu.step(&mut bus); // plenty of time for DIV to count, if it could
        }
        assert_eq!(bus.read(0xFF04), 0);

        bus.joypad.set_button(crate::joypad::Button::Start, true);
        for _ in 0..0x400 {
            cpu.step(&mut bus);
        }
        assert_ne!(bus.read(0xFF04), 0, "it counts again once awake");
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
