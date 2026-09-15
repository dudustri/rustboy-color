use super::*;

#[test]
fn rejects_a_rom_shorter_than_its_header() {
    assert_eq!(
        Cartridge::new(vec![0; 16]).unwrap_err(),
        CartridgeError::TooSmall(16)
    );
}

#[test]
fn rejects_an_unknown_mapper() {
    let mut rom = vec![0; 0x8000];
    rom[0x0147] = 0xFE;
    assert_eq!(
        Cartridge::new(rom).unwrap_err(),
        CartridgeError::UnsupportedMapper(0xFE)
    );
}

// An MBC1 cartridge where the first byte of every bank holds that bank's own number.
fn mbc1(banks: usize, ram_size_code: u8) -> Cartridge {
    let mut rom = vec![0; banks * 0x4000];
    for bank in 0..banks {
        rom[bank * 0x4000] = bank as u8;
    }
    rom[0x0147] = 0x03; // MBC1 with RAM and a battery
    rom[0x0149] = ram_size_code;
    Cartridge::new(rom).unwrap()
}

#[test]
fn mbc1_starts_with_bank_one_in_the_window() {
    let cart = mbc1(4, 0);
    assert_eq!(cart.read_rom(0x0000), 0);
    assert_eq!(cart.read_rom(0x4000), 1);
}

#[test]
fn mbc1_switches_rom_banks() {
    let mut cart = mbc1(4, 0);
    cart.write_rom(0x2000, 2);
    assert_eq!(cart.read_rom(0x4000), 2);
    cart.write_rom(0x3FFF, 3); // anywhere in 2000-3FFF works
    assert_eq!(cart.read_rom(0x4000), 3);
}

#[test]
fn mbc1_turns_a_request_for_bank_zero_into_bank_one() {
    let mut cart = mbc1(4, 0);
    cart.write_rom(0x2000, 0);
    assert_eq!(cart.read_rom(0x4000), 1);
    cart.write_rom(0x2000, 0x20); // only the low 5 bits count, and they are 0
    assert_eq!(cart.read_rom(0x4000), 1);
}

#[test]
fn mbc1_wraps_a_bank_past_the_end_of_the_rom() {
    let mut cart = mbc1(4, 0);
    cart.write_rom(0x2000, 5);
    assert_eq!(cart.read_rom(0x4000), 1);
    cart.write_rom(0x2000, 6);
    assert_eq!(cart.read_rom(0x4000), 2);
}

#[test]
fn mbc1_reaches_high_banks_with_the_upper_bits() {
    let mut cart = mbc1(64, 0);
    cart.write_rom(0x4000, 1);
    cart.write_rom(0x2000, 3);
    assert_eq!(cart.read_rom(0x4000), 0x23);
}

#[test]
fn mbc1_advanced_mode_can_move_bank_zero() {
    let mut cart = mbc1(64, 0);
    cart.write_rom(0x4000, 1);
    assert_eq!(cart.read_rom(0x0000), 0, "not yet");
    cart.write_rom(0x6000, 1);
    assert_eq!(cart.read_rom(0x0000), 0x20);
}

#[test]
fn mbc1_ram_only_answers_once_switched_on() {
    let mut cart = mbc1(2, 0x02); // 8 KB of save RAM
    cart.write_ram(0xA000, 0x42);
    assert_eq!(cart.read_ram(0xA000), 0xFF, "off, so the write was lost");

    cart.write_rom(0x0000, 0x0A);
    cart.write_ram(0xA000, 0x42);
    assert_eq!(cart.read_ram(0xA000), 0x42);

    cart.write_rom(0x0000, 0x00);
    assert_eq!(cart.read_ram(0xA000), 0xFF, "off again");
}

#[test]
fn reads_the_title() {
    let mut rom = vec![0; 0x8000];
    rom[0x0134..0x0139].copy_from_slice(b"ZELDA");
    let cart = Cartridge::new(rom).unwrap();
    assert_eq!(cart.header.title, "ZELDA");
}
