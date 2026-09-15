use super::*;

// Running every 16 ticks, DIV at 0, TMA at 80, and TIMA one step from overflowing.
fn about_to_overflow() -> Timer {
    let mut timer = Timer::new();
    timer.write(0xFF04, 0);
    timer.write(0xFF07, 0x05);
    timer.write(0xFF06, 0x80);
    timer.write(0xFF05, 0xFF);
    timer
}

#[test]
fn an_overflow_reads_zero_for_one_m_cycle_then_reloads() {
    let mut timer = about_to_overflow();
    assert_eq!(timer.tick(16), 0, "no interrupt yet");
    assert_eq!(timer.read(0xFF05), 0x00);
    assert_eq!(
        timer.tick(4),
        IF_TIMER,
        "the interrupt arrives one M-cycle late"
    );
    assert_eq!(timer.read(0xFF05), 0x80);
}

#[test]
fn writing_tima_in_the_zero_cycle_cancels_the_overflow() {
    let mut timer = about_to_overflow();
    timer.tick(16);
    timer.write(0xFF05, 0x42);
    assert_eq!(timer.tick(4), 0, "no interrupt");
    assert_eq!(timer.read(0xFF05), 0x42, "TMA is not copied");
}

#[test]
fn writing_tima_as_it_reloads_is_ignored() {
    let mut timer = about_to_overflow();
    timer.tick(16);
    timer.tick(4);
    timer.write(0xFF05, 0x42);
    assert_eq!(timer.read(0xFF05), 0x80);
}

#[test]
fn writing_tma_as_it_reloads_goes_into_tima_too() {
    let mut timer = about_to_overflow();
    timer.tick(16);
    timer.tick(4);
    timer.write(0xFF06, 0x33);
    assert_eq!(timer.read(0xFF05), 0x33);
}

#[test]
fn the_reload_window_lasts_one_m_cycle() {
    let mut timer = about_to_overflow();
    timer.tick(16);
    timer.tick(4);
    timer.tick(4);
    timer.write(0xFF05, 0x42);
    assert_eq!(timer.read(0xFF05), 0x42);
}

#[test]
fn resetting_div_ticks_tima_only_if_the_selected_bit_was_on() {
    let mut timer = about_to_overflow();
    timer.write(0xFF05, 0x10);
    timer.tick(8); // bit 3 is now on
    timer.write(0xFF04, 0);
    assert_eq!(timer.read(0xFF05), 0x11);

    let mut timer = about_to_overflow();
    timer.write(0xFF05, 0x10);
    timer.tick(4); // bit 3 is still off
    timer.write(0xFF04, 0);
    assert_eq!(timer.read(0xFF05), 0x10);
}

#[test]
fn picking_another_speed_can_tick_tima() {
    let mut timer = about_to_overflow();
    timer.write(0xFF05, 0x10);
    timer.tick(8); // bit 3 on, bit 5 off
    timer.write(0xFF07, 0x06); // move from bit 3 to bit 5
    assert_eq!(timer.read(0xFF05), 0x11);
}

// An older Game Boy would tick here. The Color does not.
#[test]
fn switching_the_timer_off_does_not_tick_on_the_color() {
    let mut timer = about_to_overflow();
    timer.write(0xFF05, 0x10);
    timer.tick(8);
    timer.write(0xFF07, 0x01); // same speed, switched off
    assert_eq!(timer.read(0xFF05), 0x10);
}
