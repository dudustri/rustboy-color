//! Game cartridge: its ROM, its save RAM, and chip that swaps banks between them.

mod header;
mod mbc;

pub use header::{CgbFlag, Header, header_checksum};
pub use mbc::Mbc;

use core::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CartridgeError {
    /// file is too short to even hold a cartridge header.
    TooSmall(usize),
    UnsupportedMapper(u8),
    /// header does not add up, so a real console would refuse to start.
    BadChecksum {
        found: u8,
        expected: u8,
    },
}

impl fmt::Display for CartridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooSmall(len) => write!(f, "ROM is {len} bytes, too small to hold a header"),
            Self::UnsupportedMapper(code) => write!(f, "unsupported cartridge type {code:#04X}"),
            Self::BadChecksum { found, expected } => {
                write!(
                    f,
                    "header checksum is {found:#04X}, should be {expected:#04X}"
                )
            }
        }
    }
}

impl std::error::Error for CartridgeError {}

pub struct Cartridge {
    pub header: Header, // what game says about itself
    rom: Vec<u8>,       // whole game file
    ram: Vec<u8>,       // save data, kept alive by a battery in cart
    mbc: Mbc,           // chip that swaps banks in and out
}

impl fmt::Debug for Cartridge {
    /// written by hand on purpose: deriving it would dump entire ROM into panic messages.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Cartridge")
            .field("title", &self.header.title)
            .field("mbc", &self.mbc)
            .field("rom_len", &self.rom.len())
            .field("ram_len", &self.ram.len())
            .finish()
    }
}

impl Cartridge {
    pub fn new(rom: Vec<u8>) -> Result<Self, CartridgeError> {
        let header = Header::parse(&rom)?;
        let mbc = Mbc::from_cartridge_type(header.cartridge_type)?;
        let ram = vec![0; header.ram_size];
        Ok(Self {
            header,
            rom,
            ram,
            mbc,
        })
    }

    /// a fake 32 KiB cartridge we can write to, so tests can place opcodes anywhere.
    pub fn test_ram() -> Self {
        Self {
            header: Header {
                title: "TEST".to_string(),
                cgb: CgbFlag::Only,
                cartridge_type: 0x00,
                rom_size: 0x8000,
                ram_size: 0x2000,
            },
            rom: vec![0xFF; 0x8000],
            ram: vec![0; 0x2000],
            mbc: Mbc::None { writable: true },
        }
    }

    pub fn read_rom(&self, addr: u16) -> u8 {
        self.rom
            .get(self.mbc.rom_offset(addr, self.rom.len()))
            .copied()
            .unwrap_or(0xFF)
    }

    pub fn write_rom(&mut self, addr: u16, value: u8) {
        if let Mbc::None { writable: true } = self.mbc {
            let offset = addr as usize;
            if offset < self.rom.len() {
                self.rom[offset] = value;
            }
            return;
        }
        self.mbc.write_control(addr, value);
    }

    /// keeps a cartridge clock running, for chips that have one.
    pub fn tick(&mut self, t_cycles: u32) {
        self.mbc.tick(t_cycles);
    }

    pub fn read_ram(&self, addr: u16) -> u8 {
        if let Some(register) = self.mbc.clock_register() {
            return self.mbc.read_clock(register);
        }
        match self.mbc.ram_offset(addr, self.ram.len()) {
            Some(offset) => self.ram.get(offset).copied().unwrap_or(0xFF),
            None => 0xFF,
        }
    }

    pub fn write_ram(&mut self, addr: u16, value: u8) {
        if let Some(register) = self.mbc.clock_register() {
            return self.mbc.write_clock(register, value);
        }
        if let Some(offset) = self.mbc.ram_offset(addr, self.ram.len())
            && offset < self.ram.len()
        {
            self.ram[offset] = value;
        }
    }

    /// save data, for host to keep on disk.
    pub fn ram(&self) -> &[u8] {
        &self.ram
    }

    pub fn load_ram(&mut self, data: &[u8]) {
        let len = data.len().min(self.ram.len());
        self.ram[..len].copy_from_slice(&data[..len]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::T_CYCLES_PER_SECOND;

    // a console refuses a header that does not add up, so every fixture signs itself
    fn sign(rom: &mut [u8]) {
        rom[0x014D] = header_checksum(rom);
    }

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
        sign(&mut rom);
        assert_eq!(
            Cartridge::new(rom).unwrap_err(),
            CartridgeError::UnsupportedMapper(0xFE)
        );
    }

    // an MBC1 cartridge where first byte of every bank holds that bank's own number.
    // each bank starts with its own number, so a read says which one is in the window
    fn cart(kind: u8, banks: usize, ram_size_code: u8) -> Cartridge {
        let mut rom = vec![0; banks * 0x4000];
        for bank in 0..banks {
            rom[bank * 0x4000] = bank as u8;
            rom[bank * 0x4000 + 1] = (bank >> 8) as u8;
        }
        rom[0x0147] = kind;
        rom[0x0149] = ram_size_code;
        sign(&mut rom);
        Cartridge::new(rom).unwrap()
    }

    fn mbc1(banks: usize, ram_size_code: u8) -> Cartridge {
        cart(0x03, banks, ram_size_code) // MBC1 with RAM and a battery
    }

    // which bank the window at 4000 is showing
    fn banked(cart: &Cartridge) -> usize {
        cart.read_rom(0x4000) as usize | (cart.read_rom(0x4001) as usize) << 8
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
        cart.write_rom(0x2000, 0x20); // only low 5 bits count, and they are 0
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
        sign(&mut rom);
        let cart = Cartridge::new(rom).unwrap();
        assert_eq!(cart.header.title, "ZELDA");
    }

    #[test]
    fn mbc3_reaches_all_127_banks() {
        let mut cart = cart(0x13, 128, 0x03);
        cart.write_rom(0x2000, 0x7F);
        assert_eq!(banked(&cart), 0x7F);
    }

    #[test]
    fn mbc3_turns_a_request_for_bank_zero_into_bank_one() {
        let mut cart = cart(0x13, 4, 0x00);
        cart.write_rom(0x2000, 0x00);
        assert_eq!(banked(&cart), 1);
    }

    #[test]
    fn mbc3_switches_between_four_ram_banks() {
        let mut cart = cart(0x13, 4, 0x03); // 32 KiB of save RAM, so four banks
        cart.write_rom(0x0000, 0x0A); // switch RAM on
        for bank in 0..4 {
            cart.write_rom(0x4000, bank);
            cart.write_ram(0xA000, 0x10 + bank);
        }
        for bank in 0..4 {
            cart.write_rom(0x4000, bank);
            assert_eq!(cart.read_ram(0xA000), 0x10 + bank, "bank {bank}");
        }
    }

    // an MBC3 with a clock, switched on and ready to be asked the time
    fn clock_cart() -> Cartridge {
        let mut cart = cart(0x10, 4, 0x03); // MBC3 with clock, RAM and a battery
        cart.write_rom(0x0000, 0x0A); // one switch turns on both save RAM and clock
        cart
    }

    fn wind(cart: &mut Cartridge, seconds: u32) {
        for _ in 0..seconds {
            cart.tick(T_CYCLES_PER_SECOND);
        }
    }

    // a running clock could change mid read, so a game takes a frozen copy first
    fn latch(cart: &mut Cartridge) {
        cart.write_rom(0x6000, 0);
        cart.write_rom(0x6000, 1);
    }

    fn clock(cart: &mut Cartridge, register: u8) -> u8 {
        cart.write_rom(0x4000, register);
        cart.read_ram(0xA000)
    }

    #[test]
    fn the_clock_counts_seconds_minutes_and_hours() {
        let mut cart = clock_cart();
        wind(&mut cart, 3661); // one hour, one minute and one second
        latch(&mut cart);

        assert_eq!(clock(&mut cart, 0x08), 1, "seconds");
        assert_eq!(clock(&mut cart, 0x09), 1, "minutes");
        assert_eq!(clock(&mut cart, 0x0A), 1, "hours");
    }

    #[test]
    fn nothing_new_shows_until_the_game_takes_a_copy() {
        let mut cart = clock_cart();
        wind(&mut cart, 5);
        assert_eq!(clock(&mut cart, 0x08), 0, "no copy taken yet");

        latch(&mut cart);
        assert_eq!(clock(&mut cart, 0x08), 5);

        wind(&mut cart, 5);
        assert_eq!(
            clock(&mut cart, 0x08),
            5,
            "old copy, even though it kept counting"
        );
        latch(&mut cart);
        assert_eq!(clock(&mut cart, 0x08), 10);
    }

    // it takes a zero and then a one. a one on its own is not enough
    #[test]
    fn a_copy_needs_both_halves_of_the_signal() {
        let mut cart = clock_cart();
        latch(&mut cart);
        wind(&mut cart, 5);

        cart.write_rom(0x6000, 1);
        assert_eq!(clock(&mut cart, 0x08), 0, "a one on its own copies nothing");

        cart.write_rom(0x6000, 0);
        cart.write_rom(0x6000, 1);
        assert_eq!(clock(&mut cart, 0x08), 5);
    }

    #[test]
    fn a_game_can_set_the_time() {
        let mut cart = clock_cart();
        cart.write_rom(0x4000, 0x0A);
        cart.write_ram(0xA000, 12); // midday
        latch(&mut cart);
        assert_eq!(clock(&mut cart, 0x0A), 12);
    }

    // bit 6 stops it, which is what a game does while setting it
    #[test]
    fn a_halted_clock_stands_still() {
        let mut cart = clock_cart();
        cart.write_rom(0x4000, 0x0C);
        cart.write_ram(0xA000, 0x40);

        wind(&mut cart, 100);
        latch(&mut cart);
        assert_eq!(clock(&mut cart, 0x08), 0);
    }

    // it only counts 511 days, then it wraps and raises a flag the game has to clear
    #[test]
    fn running_out_of_days_raises_a_flag() {
        let mut cart = clock_cart();
        cart.write_rom(0x4000, 0x0B);
        cart.write_ram(0xA000, 0xFF); // day 255 ...
        cart.write_rom(0x4000, 0x0C);
        cart.write_ram(0xA000, 0x01); // ... plus the ninth bit, so day 511
        cart.write_rom(0x4000, 0x0A);
        cart.write_ram(0xA000, 23);
        cart.write_rom(0x4000, 0x09);
        cart.write_ram(0xA000, 59);
        cart.write_rom(0x4000, 0x08);
        cart.write_ram(0xA000, 59); // one second before it runs out

        wind(&mut cart, 1);
        latch(&mut cart);
        assert_eq!(clock(&mut cart, 0x0B), 0, "back to day zero");
        assert_eq!(clock(&mut cart, 0x0C), 0x80, "and the flag is up");
    }

    #[test]
    fn the_clock_answers_instead_of_save_ram() {
        let mut cart = clock_cart();
        cart.write_rom(0x4000, 0x00); // save RAM bank 0
        cart.write_ram(0xA000, 0x42);
        wind(&mut cart, 7);
        latch(&mut cart);

        assert_eq!(clock(&mut cart, 0x08), 7, "clock window");
        cart.write_rom(0x4000, 0x00);
        assert_eq!(cart.read_ram(0xA000), 0x42, "save RAM is untouched");
    }

    // this chip takes its bank number in two halves, so it can count past 255
    #[test]
    fn mbc5_reaches_banks_above_255() {
        let mut cart = cart(0x1B, 512, 0x03);
        cart.write_rom(0x2000, 0x34); // low half
        cart.write_rom(0x3000, 0x01); // high bit
        assert_eq!(banked(&cart), 0x134);
    }

    // unlike the older chips this one really does let bank 0 into the window
    #[test]
    fn mbc5_allows_bank_zero_in_the_window() {
        let mut cart = cart(0x1B, 4, 0x00);
        cart.write_rom(0x2000, 0x00);
        assert_eq!(banked(&cart), 0);
    }

    #[test]
    fn mbc5_switches_between_sixteen_ram_banks() {
        let mut cart = cart(0x1B, 4, 0x04); // 128 KiB of save RAM, so sixteen banks
        cart.write_rom(0x0000, 0x0A);
        for bank in 0..16 {
            cart.write_rom(0x4000, bank);
            cart.write_ram(0xA000, 0x20 + bank);
        }
        for bank in 0..16 {
            cart.write_rom(0x4000, bank);
            assert_eq!(cart.read_ram(0xA000), 0x20 + bank, "bank {bank}");
        }
    }

    #[test]
    fn the_newer_chips_keep_ram_shut_until_asked() {
        for kind in [0x13, 0x1B] {
            let mut cart = cart(kind, 4, 0x03);
            cart.write_ram(0xA000, 0x42);
            assert_eq!(cart.read_ram(0xA000), 0xFF, "type {kind:#04X}");
        }
    }

    #[test]
    fn rejects_a_header_that_does_not_add_up() {
        let mut rom = vec![0; 0x8000];
        rom[0x0134..0x0139].copy_from_slice(b"ZELDA");
        sign(&mut rom);
        rom[0x0135] = b'X'; // one letter changed after signing

        assert_eq!(
            Cartridge::new(rom).unwrap_err(),
            // 'E' became 'X', 19 higher, so what the header claims is now 19 too high
            CartridgeError::BadChecksum {
                found: 0x77,
                expected: 0x64,
            }
        );
    }

    // every real cartridge passes, so our sum has to match what hardware works out
    #[test]
    fn the_sum_runs_over_the_header_and_nothing_else() {
        let mut rom = vec![0; 0x8000];
        rom[0x0133] = 0xFF; // just before the part that counts
        rom[0x014D] = 0xFF; // the answer itself
        rom[0x014E] = 0xFF; // just after
        assert_eq!(header_checksum(&rom), 0xE7, "25 bytes of zero, so 0 - 25");
    }
}
