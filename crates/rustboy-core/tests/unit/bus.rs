use super::*;

#[test]
fn echo_ram_mirrors_work_ram() {
    let mut bus = Bus::testing();
    bus.write(0xC123, 0x42);
    assert_eq!(bus.read(0xE123), 0x42);
}

#[test]
fn unusable_region_reads_back_ff() {
    let mut bus = Bus::testing();
    bus.write(0xFEA0, 0x42);
    assert_eq!(bus.read(0xFEA0), 0xFF);
}

#[test]
fn hram_and_ie_are_distinct() {
    let mut bus = Bus::testing();
    bus.write(0xFFFE, 0x11);
    bus.write(0xFFFF, 0x22);
    assert_eq!(bus.read(0xFFFE), 0x11);
    assert_eq!(bus.read(0xFFFF), 0x22);
}
