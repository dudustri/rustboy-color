//! screen: only its timing so far, real pixels in M3. See `docs/architecture.md` section 5.

mod fetcher;
mod fifo;
mod oam;

pub use fetcher::{FetchStep, Fetcher};
pub use fifo::{Pixel, PixelFifo};
pub use oam::{Sprite, SpriteScan};

use crate::bus::{IF_STAT, IF_VBLANK};
use crate::{FRAMEBUFFER_LEN, SCREEN_HEIGHT, SCREEN_WIDTH};

const VRAM_BANK_SIZE: usize = 0x2000;
const OAM_SIZE: usize = 0xA0;
const DOTS_PER_LINE: u32 = 456;
const LINES_PER_FRAME: u8 = 154;
const OAM_SCAN_DOTS: u32 = 80;

/// TODO(PR-14): drawing really takes 172 to 289 dots; pinned to shortest until FIFO exists.
const DRAWING_DOTS: u32 = 172;

/// pale green-white a real screen shows when nothing has been drawn.
const BLANK: [u8; 4] = [0xE0, 0xF8, 0xD0, 0xFF];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    HBlank = 0,
    VBlank = 1,
    OamScan = 2,
    Drawing = 3,
}

pub struct Ppu {
    pub framebuffer: Vec<u8>, // finished picture, 4 bytes per pixel: red, green, blue, alpha
    pub frame_ready: bool,    // true when a picture is done: host's cue to draw it
    vram: Vec<u8>,            // 2 banks of tile pictures and maps, seen at 8000-9FFF
    oam: [u8; OAM_SIZE],      // 40 sprite entries, seen at FE00-FE9F
    mode: Mode,               // which of 4 stages of a line we are in
    dot: u32,                 // ticks into current line, 0 to 455

    // registers: knobs a game turns, each with its own address
    lcdc: u8,       // FF40 main switch: screen on, window on, sprite size
    stat: u8,       // FF41 what screen is doing, and which events interrupt
    scy: u8,        // FF42 background scrolled up by this much
    scx: u8,        // FF43 background scrolled left by this much
    ly: u8,         // FF44 line being drawn right now, 0 to 153
    lyc: u8,        // FF45 interrupt when ly reaches this line
    bgp: u8,        // FF47 4 grey shades for background, old Game Boy only
    obp0: u8,       // FF48 grey shades for sprites using palette 0
    obp1: u8,       // FF49 grey shades for sprites using palette 1
    wy: u8,         // FF4A top edge of window
    wx: u8,         // FF4B left edge of window, plus 7
    vbk: u8,        // FF4F which of 2 video RAM banks is on show
    bcps: u8,       // FF68 which background colour slot FF69 will touch
    bcpd: [u8; 64], // FF69 8 background palettes, 4 colours each
    ocps: u8,       // FF6A which sprite colour slot FF6B will touch
    ocpd: [u8; 64], // FF6B 8 sprite palettes, 4 colours each

    // pixel pipeline, all still empty
    fetcher: Fetcher,    // builds background pixels 8 at a time
    bg_fifo: PixelFifo,  // background pixels waiting their turn
    obj_fifo: PixelFifo, // sprite pixels waiting to be mixed in
    #[allow(dead_code, reason = "TODO(PR-16): read by the sprite fetcher")]
    scan: SpriteScan, // sprites picked for this line
    stat_line: bool,     // shared interrupt line; only its rise asks for an interrupt
}

impl Ppu {
    pub fn new() -> Self {
        Self {
            framebuffer: vec![0xFF; FRAMEBUFFER_LEN],
            frame_ready: false,
            vram: vec![0; VRAM_BANK_SIZE * 2],
            oam: [0; OAM_SIZE],
            mode: Mode::OamScan,
            dot: 0,
            lcdc: 0x91,
            stat: 0x85,
            scy: 0,
            scx: 0,
            ly: 0,
            lyc: 0,
            bgp: 0xFC,
            obp0: 0xFF,
            obp1: 0xFF,
            wy: 0,
            wx: 0,
            vbk: 0,
            bcps: 0,
            bcpd: [0xFF; 64],
            ocps: 0,
            ocpd: [0xFF; 64],
            fetcher: Fetcher::new(),
            bg_fifo: PixelFifo::new(),
            obj_fifo: PixelFifo::new(),
            scan: SpriteScan::new(),
            stat_line: false,
        }
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn ly(&self) -> u8 {
        self.visible_ly()
    }

    // last line is odd: FF44 shows 153 for only 4 dots, then reads 0
    fn visible_ly(&self) -> u8 {
        if self.ly == LINES_PER_FRAME - 1 && self.dot >= 4 {
            0
        } else {
            self.ly
        }
    }

    // LYC keeps comparing against 153 for 8 dots, one step longer than FF44 shows it
    fn compared_ly(&self) -> u8 {
        if self.ly == LINES_PER_FRAME - 1 && self.dot >= 8 {
            0
        } else {
            self.ly
        }
    }

    pub fn tick(&mut self, t_cycles: u32) -> u8 {
        if self.lcdc & 0x80 == 0 {
            return 0;
        }
        let mut irq = 0;
        for _ in 0..t_cycles {
            irq |= self.tick_dot();
        }
        irq
    }

    fn tick_dot(&mut self) -> u8 {
        let mut irq = 0;
        self.dot += 1;

        match self.mode {
            Mode::OamScan => {
                if self.dot >= OAM_SCAN_DOTS {
                    // TODO(PR-16): pick this line's sprites here.
                    self.mode = Mode::Drawing;
                    self.fetcher.restart();
                    self.bg_fifo.clear();
                    self.obj_fifo.clear();
                }
            }
            Mode::Drawing => {
                if self.dot >= OAM_SCAN_DOTS + DRAWING_DOTS {
                    self.render_line();
                    self.mode = Mode::HBlank;
                }
            }
            Mode::HBlank => {
                if self.dot >= DOTS_PER_LINE {
                    self.dot = 0;
                    self.ly += 1;
                    self.compare_ly();
                    if self.ly as usize >= SCREEN_HEIGHT {
                        self.mode = Mode::VBlank;
                        self.frame_ready = true;
                        irq |= IF_VBLANK;
                    } else {
                        self.mode = Mode::OamScan;
                    }
                }
            }
            Mode::VBlank => {
                // partway through last line LYC starts comparing against 0 instead
                if self.ly == LINES_PER_FRAME - 1 && self.dot == 8 {
                    self.compare_ly();
                }
                if self.dot >= DOTS_PER_LINE {
                    self.dot = 0;
                    self.ly += 1;
                    if self.ly >= LINES_PER_FRAME {
                        self.ly = 0;
                        self.mode = Mode::OamScan;
                    }
                    self.compare_ly();
                }
            }
        }
        irq | self.poll_stat()
    }

    // keep LY equals LYC flag up to date. It is one of interrupt sources.
    fn compare_ly(&mut self) {
        if self.compared_ly() == self.lyc {
            self.stat |= 0x04;
        } else {
            self.stat &= !0x04;
        }
    }

    // every switched-on source is ORed together into one line.
    fn stat_sources(&self) -> bool {
        let mode_source = match self.mode {
            Mode::HBlank => self.stat & 0x08,
            Mode::VBlank => self.stat & 0x10,
            Mode::OamScan => self.stat & 0x20,
            Mode::Drawing => 0,
        };
        mode_source != 0 || (self.stat & 0x44 == 0x44)
    }

    // only a rise asks for an interrupt, so two sources at once still give one.
    fn poll_stat(&mut self) -> u8 {
        let now = self.stat_sources();
        let rose = now && !self.stat_line;
        self.stat_line = now;
        if rose { IF_STAT } else { 0 }
    }

    // TODO(PR-14..17): draw real pixels here instead of a blank line.
    fn render_line(&mut self) {
        if self.ly as usize >= SCREEN_HEIGHT {
            return;
        }
        let start = self.ly as usize * SCREEN_WIDTH * 4;
        let end = start + SCREEN_WIDTH * 4;
        for pixel in self.framebuffer[start..end].chunks_exact_mut(4) {
            pixel.copy_from_slice(&BLANK);
        }
    }

    fn vram_index(&self, addr: u16) -> usize {
        (self.vbk as usize & 1) * VRAM_BANK_SIZE + (addr as usize - 0x8000)
    }

    // while drawing, screen is reading video RAM and palettes, so CPU is shut out.
    fn drawing(&self) -> bool {
        self.mode == Mode::Drawing
    }

    // sprite table is in use from sprite search until line is drawn.
    fn oam_busy(&self) -> bool {
        matches!(self.mode, Mode::OamScan | Mode::Drawing)
    }

    /// reads 0xFF while screen is drawing.
    pub fn read_vram(&self, addr: u16) -> u8 {
        if self.drawing() {
            return 0xFF;
        }
        self.vram[self.vram_index(addr)]
    }

    /// ignored while screen is drawing.
    pub fn write_vram(&mut self, addr: u16, value: u8) {
        if !self.drawing() {
            let index = self.vram_index(addr);
            self.vram[index] = value;
        }
    }

    /// reads 0xFF during sprite search and while drawing.
    pub fn read_oam(&self, addr: u16) -> u8 {
        if self.oam_busy() {
            return 0xFF;
        }
        self.oam[(addr - 0xFE00) as usize]
    }

    /// ignored during sprite search and while drawing.
    pub fn write_oam(&mut self, addr: u16, value: u8) {
        if !self.oam_busy() {
            self.oam[(addr - 0xFE00) as usize] = value;
        }
    }

    pub fn read_register(&self, addr: u16) -> u8 {
        match addr {
            0xFF40 => self.lcdc,
            0xFF41 => self.stat | 0x80 | self.mode as u8,
            0xFF42 => self.scy,
            0xFF43 => self.scx,
            0xFF44 => self.visible_ly(),
            0xFF45 => self.lyc,
            0xFF47 => self.bgp,
            0xFF48 => self.obp0,
            0xFF49 => self.obp1,
            0xFF4A => self.wy,
            0xFF4B => self.wx,
            0xFF4F => self.vbk | 0xFE,
            0xFF68 => self.bcps,
            0xFF69 if self.drawing() => 0xFF,
            0xFF69 => self.bcpd[(self.bcps & 0x3F) as usize],
            0xFF6A => self.ocps,
            0xFF6B if self.drawing() => 0xFF,
            0xFF6B => self.ocpd[(self.ocps & 0x3F) as usize],
            _ => 0xFF,
        }
    }

    /// returns any interrupt write itself asked for.
    pub fn write_register(&mut self, addr: u16, value: u8) -> u8 {
        let mut irq = 0;
        match addr {
            0xFF40 => {
                self.lcdc = value;
                if value & 0x80 == 0 {
                    self.ly = 0;
                    self.dot = 0;
                    self.mode = Mode::HBlank;
                    self.stat_line = false; // a screen that is off asks for nothing
                }
            }
            // bottom 3 bits report status, so a game cannot write them.
            0xFF41 => {
                self.stat = (self.stat & 0x07) | (value & 0x78);
                irq |= self.poll_stat(); // switching a source on can raise line
            }
            0xFF42 => self.scy = value,
            0xFF43 => self.scx = value,
            0xFF44 => {} // current line is read-only
            0xFF45 => {
                self.lyc = value;
                self.compare_ly();
                irq |= self.poll_stat();
            }
            0xFF47 => self.bgp = value,
            0xFF48 => self.obp0 = value,
            0xFF49 => self.obp1 = value,
            0xFF4A => self.wy = value,
            0xFF4B => self.wx = value,
            0xFF4F => self.vbk = value & 0x01,
            0xFF68 => self.bcps = value,
            // while drawing write is lost, but index still moves on.
            0xFF69 => {
                if !self.drawing() {
                    self.bcpd[(self.bcps & 0x3F) as usize] = value;
                }
                if self.bcps & 0x80 != 0 {
                    self.bcps = (self.bcps & 0x80) | ((self.bcps + 1) & 0x3F);
                }
            }
            0xFF6A => self.ocps = value,
            // while drawing write is lost, but index still moves on.
            0xFF6B => {
                if !self.drawing() {
                    self.ocpd[(self.ocps & 0x3F) as usize] = value;
                }
                if self.ocps & 0x80 != 0 {
                    self.ocps = (self.ocps & 0x80) | ((self.ocps + 1) & 0x3F);
                }
            }
            _ => {}
        }
        irq
    }
}

impl Default for Ppu {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_is_456_dots_and_a_frame_is_154_lines() {
        let mut ppu = Ppu::new();
        ppu.tick(DOTS_PER_LINE * LINES_PER_FRAME as u32);
        assert_eq!(ppu.ly(), 0);
        assert_eq!(ppu.mode(), Mode::OamScan);
    }

    #[test]
    fn vblank_is_requested_after_the_last_visible_line() {
        let mut ppu = Ppu::new();
        let irq = ppu.tick(DOTS_PER_LINE * SCREEN_HEIGHT as u32);
        assert_eq!(irq & IF_VBLANK, IF_VBLANK);
        assert!(ppu.frame_ready);
        assert_eq!(ppu.mode(), Mode::VBlank);
    }

    #[test]
    fn a_disabled_lcd_does_not_advance() {
        let mut ppu = Ppu::new();
        ppu.write_register(0xFF40, 0x00);
        ppu.tick(DOTS_PER_LINE * 10);
        assert_eq!(ppu.ly(), 0);
    }

    // a new screen moved forward to start of one mode, on first line.
    fn in_mode(mode: Mode) -> Ppu {
        let mut ppu = Ppu::new();
        ppu.tick(match mode {
            Mode::OamScan => 0,
            Mode::Drawing => OAM_SCAN_DOTS,
            Mode::HBlank => OAM_SCAN_DOTS + DRAWING_DOTS,
            Mode::VBlank => DOTS_PER_LINE * SCREEN_HEIGHT as u32,
        });
        assert_eq!(ppu.mode(), mode);
        ppu
    }

    // both HBlank source and LY match are on, so only first rise counts
    // straight from TCAGBD: on last line LY reads 153 for 4 dots, then 0
    #[test]
    fn ly_shows_153_for_only_four_dots() {
        let mut ppu = Ppu::new();
        ppu.tick(DOTS_PER_LINE * (LINES_PER_FRAME as u32 - 1)); // start of last line
        assert_eq!(ppu.ly(), 153);

        ppu.tick(4);
        assert_eq!(ppu.ly(), 0, "reads 0 for rest of line");
        assert_eq!(ppu.mode(), Mode::VBlank, "still on last line though");
    }

    // LYC lags by 4 dots, so 153 and 0 can both match on one line
    #[test]
    fn both_153_and_0_can_match_on_the_last_line() {
        let mut ppu = Ppu::new();
        ppu.write_register(0xFF41, 0x40); // LY match source
        ppu.write_register(0xFF45, 153);
        let start = ppu.tick(DOTS_PER_LINE * (LINES_PER_FRAME as u32 - 1));
        assert_eq!(start & IF_STAT, IF_STAT, "153 matches as line begins");

        let mut ppu = Ppu::new();
        ppu.write_register(0xFF41, 0x40);
        ppu.write_register(0xFF45, 0);
        ppu.tick(DOTS_PER_LINE * (LINES_PER_FRAME as u32 - 1));
        let after = ppu.tick(8);
        assert_eq!(after & IF_STAT, IF_STAT, "0 matches 8 dots later");
    }

    #[test]
    fn two_sources_at_once_still_give_one_interrupt() {
        let mut ppu = Ppu::new();
        ppu.write_register(0xFF41, 0x48); // HBlank and LY match sources on
        ppu.write_register(0xFF45, 0x00); // match on line 0, which we are on

        let irq = ppu.tick(OAM_SCAN_DOTS + DRAWING_DOTS); // into blank at end of line 0
        assert_eq!(irq & IF_STAT, 0, "line was already up from LY match");
    }

    #[test]
    fn line_has_to_drop_before_it_can_rise_again() {
        let mut ppu = Ppu::new();
        ppu.write_register(0xFF41, 0x08); // HBlank only
        ppu.write_register(0xFF45, 0xFF); // never matches

        let first = ppu.tick(OAM_SCAN_DOTS + DRAWING_DOTS);
        assert_eq!(first & IF_STAT, IF_STAT, "first blank asks for one");

        let same_blank = ppu.tick(DOTS_PER_LINE - OAM_SCAN_DOTS - DRAWING_DOTS);
        assert_eq!(same_blank & IF_STAT, 0, "still inside same blank");

        let next = ppu.tick(OAM_SCAN_DOTS + DRAWING_DOTS); // drawing drops it, next blank raises it
        assert_eq!(next & IF_STAT, IF_STAT);
    }

    // switching a source on while its condition already holds is itself a rise
    #[test]
    fn switching_a_source_on_can_ask_for_an_interrupt() {
        let mut ppu = Ppu::new();
        ppu.tick(OAM_SCAN_DOTS + DRAWING_DOTS); // sit in blank at end of line
        assert_eq!(ppu.write_register(0xFF41, 0x08), IF_STAT);
        assert_eq!(ppu.write_register(0xFF41, 0x08), 0, "no second rise");
    }

    #[test]
    fn a_new_lyc_that_matches_asks_for_an_interrupt() {
        let mut ppu = Ppu::new();
        ppu.write_register(0xFF41, 0x40); // LY match source only
        ppu.tick(DOTS_PER_LINE * 3); // now on line 3
        assert_eq!(ppu.write_register(0xFF45, 3), IF_STAT);
        assert_eq!(ppu.write_register(0xFF45, 9), 0, "no longer a match");
    }

    #[test]
    fn a_screen_that_is_off_asks_for_nothing() {
        let mut ppu = Ppu::new();
        ppu.write_register(0xFF41, 0x48);
        ppu.tick(OAM_SCAN_DOTS + DRAWING_DOTS);
        ppu.write_register(0xFF40, 0x00);
        assert_eq!(ppu.tick(DOTS_PER_LINE * 4), 0);
    }

    #[test]
    fn video_ram_is_shut_only_while_drawing() {
        for (mode, open) in [
            (Mode::OamScan, true),
            (Mode::Drawing, false),
            (Mode::HBlank, true),
            (Mode::VBlank, true),
        ] {
            let mut ppu = in_mode(mode);
            ppu.write_vram(0x8000, 0x42);
            assert_eq!(
                ppu.read_vram(0x8000),
                if open { 0x42 } else { 0xFF },
                "{mode:?}"
            );
        }
    }

    // reading 0xFF alone cannot prove write was lost, so look again once it is open.
    #[test]
    fn a_write_while_drawing_is_lost() {
        let mut ppu = in_mode(Mode::Drawing);
        ppu.write_vram(0x8000, 0x42);
        ppu.write_oam(0xFE00, 0x42);
        ppu.tick(DRAWING_DOTS); // on to blank at end of line
        assert_eq!(ppu.read_vram(0x8000), 0x00);
        assert_eq!(ppu.read_oam(0xFE00), 0x00);
    }

    #[test]
    fn the_sprite_table_is_shut_from_the_search_until_drawing_ends() {
        for (mode, open) in [
            (Mode::OamScan, false),
            (Mode::Drawing, false),
            (Mode::HBlank, true),
            (Mode::VBlank, true),
        ] {
            let mut ppu = in_mode(mode);
            ppu.write_oam(0xFE00, 0x42);
            assert_eq!(
                ppu.read_oam(0xFE00),
                if open { 0x42 } else { 0xFF },
                "{mode:?}"
            );
        }
    }

    #[test]
    fn palettes_are_shut_while_drawing_but_the_index_still_moves() {
        let mut ppu = in_mode(Mode::Drawing);
        ppu.write_register(0xFF68, 0x80); // index 0, step after each write
        ppu.write_register(0xFF69, 0x12);
        assert_eq!(ppu.read_register(0xFF69), 0xFF);
        assert_eq!(
            ppu.read_register(0xFF68) & 0x3F,
            1,
            "the index moved anyway"
        );

        ppu.tick(DRAWING_DOTS);
        ppu.write_register(0xFF68, 0x00);
        assert_eq!(ppu.read_register(0xFF69), 0xFF, "slot 0 kept its old value");
    }

    #[test]
    fn everything_is_open_with_the_screen_off() {
        let mut ppu = in_mode(Mode::Drawing);
        ppu.write_register(0xFF40, 0x00);
        ppu.write_vram(0x8000, 0x42);
        ppu.write_oam(0xFE00, 0x42);
        assert_eq!(ppu.read_vram(0x8000), 0x42);
        assert_eq!(ppu.read_oam(0xFE00), 0x42);
    }
}
