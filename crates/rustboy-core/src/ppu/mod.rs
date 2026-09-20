//! The screen: only its timing so far, real pixels in M3. See `docs/architecture.md` section 5.

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

/// TODO(PR-14): drawing really takes 172 to 289 dots; pinned to the shortest until the FIFO exists.
const DRAWING_DOTS: u32 = 172;

/// The pale green-white a real screen shows when nothing has been drawn.
const BLANK: [u8; 4] = [0xE0, 0xF8, 0xD0, 0xFF];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    HBlank = 0,
    VBlank = 1,
    OamScan = 2,
    Drawing = 3,
}

pub struct Ppu {
    pub framebuffer: Vec<u8>, // the finished picture, 4 bytes per pixel: red, green, blue, alpha
    pub frame_ready: bool,    // true when a picture is done: the host's cue to draw it
    vram: Vec<u8>,            // 2 banks of tile pictures and maps, seen at 8000-9FFF
    oam: [u8; OAM_SIZE],      // the 40 sprite entries, seen at FE00-FE9F
    mode: Mode,               // which of the 4 stages of a line we are in
    dot: u32,                 // ticks into the current line, 0 to 455

    // registers: the knobs a game turns, each with its own address
    lcdc: u8,       // FF40 main switch: screen on, window on, sprite size
    stat: u8,       // FF41 what the screen is doing, and which events interrupt
    scy: u8,        // FF42 background scrolled up by this much
    scx: u8,        // FF43 background scrolled left by this much
    ly: u8,         // FF44 the line being drawn right now, 0 to 153
    lyc: u8,        // FF45 interrupt when ly reaches this line
    bgp: u8,        // FF47 the 4 grey shades for the background, old Game Boy only
    obp0: u8,       // FF48 grey shades for sprites using palette 0
    obp1: u8,       // FF49 grey shades for sprites using palette 1
    wy: u8,         // FF4A top edge of the window
    wx: u8,         // FF4B left edge of the window, plus 7
    vbk: u8,        // FF4F which of the 2 video RAM banks is on show
    bcps: u8,       // FF68 which background colour slot FF69 will touch
    bcpd: [u8; 64], // FF69 the 8 background palettes, 4 colours each
    ocps: u8,       // FF6A which sprite colour slot FF6B will touch
    ocpd: [u8; 64], // FF6B the 8 sprite palettes, 4 colours each

    // the pixel pipeline, all still empty
    fetcher: Fetcher,    // builds background pixels 8 at a time
    bg_fifo: PixelFifo,  // background pixels waiting their turn
    obj_fifo: PixelFifo, // sprite pixels waiting to be mixed in
    #[allow(dead_code, reason = "TODO(PR-16): read by the sprite fetcher")]
    scan: SpriteScan, // the sprites picked for this line
    stat_line: bool,     // the shared interrupt line; only its rise asks for an interrupt
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
        self.ly
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

    // Keep the LY equals LYC flag up to date. It is one of the interrupt sources.
    fn compare_ly(&mut self) {
        if self.ly == self.lyc {
            self.stat |= 0x04;
        } else {
            self.stat &= !0x04;
        }
    }

    // Every switched-on source is ORed together into one line.
    fn stat_sources(&self) -> bool {
        let mode_source = match self.mode {
            Mode::HBlank => self.stat & 0x08,
            Mode::VBlank => self.stat & 0x10,
            Mode::OamScan => self.stat & 0x20,
            Mode::Drawing => 0,
        };
        mode_source != 0 || (self.stat & 0x44 == 0x44)
    }

    // Only a rise asks for an interrupt, so two sources at once still give one.
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

    // While drawing, the screen is reading video RAM and the palettes, so the CPU is shut out.
    fn drawing(&self) -> bool {
        self.mode == Mode::Drawing
    }

    // The sprite table is in use from the sprite search until the line is drawn.
    fn oam_busy(&self) -> bool {
        matches!(self.mode, Mode::OamScan | Mode::Drawing)
    }

    /// Reads 0xFF while the screen is drawing.
    pub fn read_vram(&self, addr: u16) -> u8 {
        if self.drawing() {
            return 0xFF;
        }
        self.vram[self.vram_index(addr)]
    }

    /// Ignored while the screen is drawing.
    pub fn write_vram(&mut self, addr: u16, value: u8) {
        if !self.drawing() {
            let index = self.vram_index(addr);
            self.vram[index] = value;
        }
    }

    /// Reads 0xFF during the sprite search and while drawing.
    pub fn read_oam(&self, addr: u16) -> u8 {
        if self.oam_busy() {
            return 0xFF;
        }
        self.oam[(addr - 0xFE00) as usize]
    }

    /// Ignored during the sprite search and while drawing.
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
            0xFF44 => self.ly,
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

    /// Returns any interrupt the write itself asked for.
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
            // The bottom 3 bits report status, so a game cannot write them.
            0xFF41 => {
                self.stat = (self.stat & 0x07) | (value & 0x78);
                irq |= self.poll_stat(); // switching a source on can raise the line
            }
            0xFF42 => self.scy = value,
            0xFF43 => self.scx = value,
            0xFF44 => {} // the current line is read-only
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
            // While drawing the write is lost, but the index still moves on.
            0xFF69 => {
                if !self.drawing() {
                    self.bcpd[(self.bcps & 0x3F) as usize] = value;
                }
                if self.bcps & 0x80 != 0 {
                    self.bcps = (self.bcps & 0x80) | ((self.bcps + 1) & 0x3F);
                }
            }
            0xFF6A => self.ocps = value,
            // While drawing the write is lost, but the index still moves on.
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

    // A new screen moved forward to the start of one mode, on the first line.
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

    // Reading 0xFF alone cannot prove the write was lost, so look again once it is open.
    #[test]
    fn a_write_while_drawing_is_lost() {
        let mut ppu = in_mode(Mode::Drawing);
        ppu.write_vram(0x8000, 0x42);
        ppu.write_oam(0xFE00, 0x42);
        ppu.tick(DRAWING_DOTS); // on to the blank at the end of the line
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
