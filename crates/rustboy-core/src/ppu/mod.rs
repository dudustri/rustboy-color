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

// shortest drawing: 160 pixels plus 12 dots of warm up, stalls make it longer
#[cfg(test)]
const MIN_DRAWING_DOTS: u32 = 172;

/// pale green-white a real screen shows when nothing has been drawn.
const BLANK: [u8; 4] = [0xE0, 0xF8, 0xD0, 0xFF];

// four shades a screen can show, lightest first
const SHADES: [[u8; 4]; 4] = [
    BLANK,
    [0x88, 0xC0, 0x70, 0xFF],
    [0x34, 0x68, 0x56, 0xFF],
    [0x08, 0x18, 0x20, 0xFF],
];

// leftmost pixel is bit 7. low byte gives bottom bit of colour, high byte top bit.
pub(crate) fn pixel_colour(low: u8, high: u8, bit: u8) -> u8 {
    let low_bit = (low >> bit) & 1;
    let high_bit = (high >> bit) & 1;
    high_bit << 1 | low_bit
}

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
    drawn: u8,           // pixels put on screen so far this line
    discard: u8,         // leftmost pixels to throw away, for fine scrolling
    on_window: bool,     // fetcher has switched over to window for rest of line
    fetcher: Fetcher,    // builds background pixels 8 at a time
    bg_fifo: PixelFifo,  // background pixels waiting their turn
    obj_fifo: PixelFifo, // sprite pixels waiting to be mixed in
    scan: SpriteScan,    // sprites picked for this line
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
            drawn: 0,
            discard: 0,
            on_window: false,
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
                    self.scan.scan(&self.oam, self.ly, self.lcdc & 0x04 != 0);
                    self.mode = Mode::Drawing;
                    self.fetcher.restart();
                    self.bg_fifo.clear();
                    self.drawn = 0;
                    self.discard = self.scx % 8; // scrolling within a tile
                    self.on_window = false;
                    self.obj_fifo.clear();
                }
            }
            Mode::Drawing => {
                self.start_window();
                self.fetch_sprites();
                self.draw_dot();
                self.fetch_dot();
                if self.drawn as usize >= SCREEN_WIDTH {
                    self.mode = Mode::HBlank; // line is done whenever last pixel is out
                }
            }
            Mode::HBlank => {
                if self.dot >= DOTS_PER_LINE {
                    if self.on_window {
                        self.fetcher.window_line += 1;
                    }
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
                        self.fetcher.window_line = 0;
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
    // take one pixel off queue and put it on screen, once line has started properly
    // every sprite starting at this pixel drops its 8 pixels into sprite queue
    fn fetch_sprites(&mut self) {
        if self.lcdc & 0x02 == 0 {
            return; // sprites switched off
        }
        let here = self.drawn + 8;
        let tall = self.lcdc & 0x04 != 0;
        for index in 0..self.scan.visible().len() {
            let sprite = self.scan.visible()[index];
            if sprite.x != here {
                continue;
            }
            let height = if tall { 16 } else { 8 };
            let mut row = (self.ly + 16 - sprite.y) % height;
            if sprite.attributes & 0x40 != 0 {
                row = height - 1 - row; // flipped top to bottom
            }
            let tile = if tall {
                sprite.tile & 0xFE
            } else {
                sprite.tile
            };
            // sprites always count tiles up from 8000, never backwards like background can
            let at = 0x8000 + tile as u16 * 16 + row as u16 * 2;
            let (low, high) = (self.vram_at(0, at), self.vram_at(0, at + 1));
            let palette = (sprite.attributes >> 4) & 1;
            let flipped = sprite.attributes & 0x20 != 0; // mirrored left to right
            self.obj_fifo.overlay(std::array::from_fn(|step| Pixel {
                color: pixel_colour(low, high, if flipped { step as u8 } else { 7 - step as u8 }),
                palette,
                priority: sprite.attributes & 0x80 != 0,
            }));
        }
    }

    fn draw_dot(&mut self) {
        if self.drawn as usize >= SCREEN_WIDTH {
            return;
        }
        let Some(pixel) = self.bg_fifo.pop() else {
            return; // queue is empty, so fetcher is still catching up
        };
        if self.discard > 0 {
            self.discard -= 1; // scrolled past, so this one never shows
            return;
        }

        // background switched off leaves screen blank, and sprites then always win
        let (behind, background) = if self.lcdc & 0x01 == 0 {
            (0, 0)
        } else {
            (pixel.color, (self.bgp >> (pixel.color * 2)) & 0x03)
        };
        // a sprite's colour 0 is see through, and one marked low can also hide behind
        let shade = match self.obj_fifo.pop() {
            Some(sprite) if sprite.color != 0 && !(sprite.priority && behind != 0) => {
                let palette = if sprite.palette == 0 {
                    self.obp0
                } else {
                    self.obp1
                };
                (palette >> (sprite.color * 2)) & 0x03
            }
            _ => background,
        };
        let at = (self.ly as usize * SCREEN_WIDTH + self.drawn as usize) * 4;
        self.framebuffer[at..at + 4].copy_from_slice(&SHADES[shade as usize]);
        self.drawn += 1;
    }

    // screen reads its own memory directly, never blocked like CPU is
    // one dot of fetcher. four steps of two dots, then a push retried until there is room.
    fn fetch_dot(&mut self) {
        if self.fetcher.step != FetchStep::Push && !self.fetcher.ready() {
            return;
        }
        // window has its own row count and never scrolls, so it looks nothing up same way
        let line = if self.on_window {
            self.fetcher.window_line
        } else {
            self.ly.wrapping_add(self.scy)
        };
        match self.fetcher.step {
            FetchStep::TileNumber => {
                let (base, tile_x) = if self.on_window {
                    (self.window_map_base(), self.fetcher.tile_x)
                } else {
                    (
                        self.bg_map_base(),
                        (self.scx / 8).wrapping_add(self.fetcher.tile_x),
                    )
                };
                self.fetcher.tile_number = self.map_tile(base, tile_x, line / 8);
                self.fetcher.next();
            }
            FetchStep::TileDataLow => {
                self.fetcher.data_low = self.tile_row(self.fetcher.tile_number, line % 8, 0).0;
                self.fetcher.next();
            }
            FetchStep::TileDataHigh => {
                self.fetcher.data_high = self.tile_row(self.fetcher.tile_number, line % 8, 0).1;
                self.fetcher.next();
                self.push_row(); // try straight away, so a tile takes six dots
            }
            FetchStep::Push => self.push_row(),
        }
    }

    // hand 8 pixels to queue, leftmost first. a queue with no room keeps fetcher waiting.
    fn push_row(&mut self) {
        if self.fetcher.warming_up {
            self.fetcher.warming_up = false;
            self.fetcher.step = FetchStep::TileNumber; // first fetch is thrown away
            return;
        }
        if self.bg_fifo.len() > 8 {
            return;
        }
        for bit in (0..8).rev() {
            self.bg_fifo.push(Pixel {
                color: pixel_colour(self.fetcher.data_low, self.fetcher.data_high, bit),
                palette: 0,
                priority: false,
            });
        }
        self.fetcher.tile_x = self.fetcher.tile_x.wrapping_add(1);
        self.fetcher.next();
    }

    fn vram_at(&self, bank: usize, addr: u16) -> u8 {
        self.vram[bank * VRAM_BANK_SIZE + (addr as usize - 0x8000)]
    }

    // window takes over once line reaches its corner, and stays for rest of line
    fn start_window(&mut self) {
        let reached = self.ly >= self.wy && self.drawn + 7 >= self.wx;
        // line's own warm up happens first, even when window starts at left edge
        if self.on_window || self.fetcher.warming_up || self.lcdc & 0x20 == 0 || !reached {
            return;
        }
        self.on_window = true;
        self.bg_fifo.clear(); // background pixels already queued never show
        self.discard = 0; // window ignores scrolling
        self.fetcher.restart();
        self.fetcher.warming_up = false; // switching costs six dots, not twelve
    }

    // LCDC bit 6 picks which of two maps holds window tile numbers
    fn window_map_base(&self) -> u16 {
        if self.lcdc & 0x40 != 0 {
            0x9C00
        } else {
            0x9800
        }
    }

    // LCDC bit 3 picks which of two maps holds background tile numbers
    fn bg_map_base(&self) -> u16 {
        if self.lcdc & 0x08 != 0 {
            0x9C00
        } else {
            0x9800
        }
    }

    // one entry of a 32 by 32 map, wrapping round at edges
    fn map_tile(&self, base: u16, tile_x: u8, tile_y: u8) -> u8 {
        let offset = (tile_y as u16 % 32) * 32 + (tile_x as u16 % 32);
        self.vram_at(0, base + offset)
    }

    // two bytes holding one row of a tile. LCDC bit 4 picks how tile numbers are counted.
    fn tile_row(&self, tile: u8, row: u8, bank: usize) -> (u8, u8) {
        let address = if self.lcdc & 0x10 != 0 {
            0x8000 + tile as u16 * 16 // counted from 8000 upwards
        } else {
            // counted from 9000, and numbers above 127 mean backwards
            0x9000u16.wrapping_add((tile as i8 as i16 * 16) as u16)
        } + row as u16 * 2;
        (self.vram_at(bank, address), self.vram_at(bank, address + 1))
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
            Mode::HBlank => OAM_SCAN_DOTS + MIN_DRAWING_DOTS,
            Mode::VBlank => DOTS_PER_LINE * SCREEN_HEIGHT as u32,
        });
        assert_eq!(ppu.mode(), mode);
        ppu
    }

    // both HBlank source and LY match are on, so only first rise counts
    // straight from TCAGBD: on last line LY reads 153 for 4 dots, then 0
    // a map whose top left tile has a known first row, with plain shades
    fn ppu_with_one_tile() -> Ppu {
        let mut ppu = Ppu::new();
        ppu.write_register(0xFF40, 0x91); // screen on, background on, tiles from 8000
        ppu.write_register(0xFF47, 0xE4); // shades straight through: 0,1,2,3
        ppu.write_vram(0x9800, 1); // top left of map is tile 1
        ppu.write_vram(0x8010, 0b1010_0000); // tile 1, row 0, low byte
        ppu.write_vram(0x8011, 0b1100_0000); // high byte
        ppu
    }

    // colour of one pixel already on screen
    fn shade_at(ppu: &Ppu, x: usize, y: usize) -> [u8; 4] {
        let at = (y * SCREEN_WIDTH + x) * 4;
        ppu.framebuffer[at..at + 4].try_into().unwrap()
    }

    // twelve dots of warm up: one fetch thrown away, then real one
    #[test]
    fn nothing_is_drawn_until_first_tile_arrives() {
        let mut ppu = ppu_with_one_tile();
        ppu.tick(OAM_SCAN_DOTS + 12);
        assert_eq!(ppu.drawn, 0, "still warming up");

        ppu.tick(1);
        assert_eq!(ppu.drawn, 1, "first pixel goes out on thirteenth dot");
    }

    #[test]
    fn a_tile_lands_on_screen_leftmost_pixel_first() {
        let mut ppu = ppu_with_one_tile();
        ppu.tick(OAM_SCAN_DOTS + 16);
        assert_eq!(shade_at(&ppu, 0, 0), SHADES[3], "both bits on");
        assert_eq!(shade_at(&ppu, 1, 0), SHADES[2], "only high bit");
        assert_eq!(shade_at(&ppu, 2, 0), SHADES[1], "only low bit");
        assert_eq!(shade_at(&ppu, 3, 0), SHADES[0], "neither");
    }

    // fetcher makes eight pixels in eight dots, so it keeps up once started
    #[test]
    fn pixels_keep_coming_one_per_dot() {
        let mut ppu = ppu_with_one_tile();
        ppu.tick(OAM_SCAN_DOTS + 13);
        for expected in 2..60 {
            ppu.tick(1);
            assert_eq!(ppu.drawn, expected);
        }
    }

    #[test]
    fn a_line_is_160_pixels_wide() {
        let mut ppu = ppu_with_one_tile();
        ppu.tick(OAM_SCAN_DOTS + MIN_DRAWING_DOTS);
        assert_eq!(ppu.drawn as usize, SCREEN_WIDTH);
        assert_eq!(ppu.mode(), Mode::HBlank);
    }

    // how many dots drawing took, counted one at a time
    fn drawing_dots(ppu: &mut Ppu) -> u32 {
        ppu.tick(OAM_SCAN_DOTS); // up to start of drawing
        assert_eq!(ppu.mode(), Mode::Drawing);
        let mut dots = 0;
        while ppu.mode() == Mode::Drawing {
            ppu.tick(1);
            dots += 1;
        }
        dots
    }

    // Pan Docs: 160 pixels plus 12 dots of warm up
    #[test]
    fn drawing_takes_172_dots_when_nothing_stalls_it() {
        let mut ppu = ppu_with_one_tile();
        assert_eq!(drawing_dots(&mut ppu), MIN_DRAWING_DOTS);
    }

    // every discarded pixel costs a dot, so scrolling lengthens line
    #[test]
    fn fine_scrolling_makes_drawing_longer() {
        for scroll in 0..8u32 {
            let mut ppu = ppu_with_one_tile();
            ppu.write_register(0xFF43, scroll as u8);
            assert_eq!(
                drawing_dots(&mut ppu),
                MIN_DRAWING_DOTS + scroll,
                "scrolled by {scroll}"
            );
        }
    }

    // line is always 456 dots, so a longer drawing leaves a shorter blank
    #[test]
    fn a_longer_drawing_eats_into_the_blank() {
        let mut ppu = ppu_with_one_tile();
        ppu.write_register(0xFF43, 5);
        let drawing = drawing_dots(&mut ppu);

        let mut blank = 0;
        while ppu.mode() == Mode::HBlank {
            ppu.tick(1);
            blank += 1;
        }
        assert_eq!(OAM_SCAN_DOTS + drawing + blank, DOTS_PER_LINE);
    }

    // BGP decides which shade each of four colours becomes
    #[test]
    fn the_palette_can_swap_the_shades_round() {
        let mut ppu = ppu_with_one_tile();
        ppu.write_register(0xFF47, 0b00_01_10_11); // 0 becomes 3, 3 becomes 0
        ppu.tick(OAM_SCAN_DOTS + 16);
        assert_eq!(
            shade_at(&ppu, 0, 0),
            SHADES[0],
            "colour 3 now shows shade 0"
        );
        assert_eq!(
            shade_at(&ppu, 3, 0),
            SHADES[3],
            "colour 0 now shows shade 3"
        );
    }

    // SCX inside a tile throws away leftmost pixels, so background slides smoothly
    #[test]
    fn fine_scrolling_throws_away_the_first_few_pixels() {
        let mut ppu = ppu_with_one_tile();
        ppu.write_register(0xFF43, 2); // scrolled two pixels into tile
        ppu.tick(OAM_SCAN_DOTS + 16);
        assert_eq!(
            shade_at(&ppu, 0, 0),
            SHADES[1],
            "third pixel of tile is now first"
        );
    }

    #[test]
    fn a_switched_off_background_leaves_the_line_blank() {
        let mut ppu = ppu_with_one_tile();
        ppu.write_register(0xFF40, 0x90); // background bit off
        ppu.tick(OAM_SCAN_DOTS + 16);
        assert_eq!(shade_at(&ppu, 0, 0), SHADES[0]);
    }

    #[test]
    fn a_pixel_takes_one_bit_from_each_byte() {
        let (low, high) = (0b1010_0000, 0b1100_0000);
        assert_eq!(pixel_colour(low, high, 7), 3); // both bits on
        assert_eq!(pixel_colour(low, high, 6), 2); // only high
        assert_eq!(pixel_colour(low, high, 5), 1); // only low
        assert_eq!(pixel_colour(low, high, 4), 0); // neither
    }

    #[test]
    fn lcdc_bit_3_picks_which_map_is_read() {
        let mut ppu = Ppu::new();
        ppu.write_vram(0x9800, 0x11);
        ppu.write_vram(0x9C00, 0x22);

        ppu.write_register(0xFF40, 0x80); // bit 3 off
        assert_eq!(ppu.map_tile(ppu.bg_map_base(), 0, 0), 0x11);

        ppu.write_register(0xFF40, 0x88); // bit 3 on
        assert_eq!(ppu.map_tile(ppu.bg_map_base(), 0, 0), 0x22);
    }

    #[test]
    fn a_map_wraps_round_after_32_tiles() {
        let mut ppu = Ppu::new();
        ppu.write_vram(0x9800, 0x42);
        assert_eq!(
            ppu.map_tile(0x9800, 32, 32),
            0x42,
            "tile 32 is tile 0 again"
        );
    }

    // 8000 mode counts tiles upwards from 8000
    #[test]
    fn tile_rows_come_from_8000_when_bit_4_is_on() {
        let mut ppu = Ppu::new();
        ppu.write_register(0xFF40, 0x90); // bit 4 on
        ppu.write_vram(0x8010, 0xAB); // tile 1, row 0
        ppu.write_vram(0x8011, 0xCD);
        assert_eq!(ppu.tile_row(1, 0, 0), (0xAB, 0xCD));

        ppu.write_vram(0x801E, 0x12); // tile 1, row 7
        ppu.write_vram(0x801F, 0x34);
        assert_eq!(ppu.tile_row(1, 7, 0), (0x12, 0x34));
    }

    // 8800 mode counts from 9000, and tile numbers above 127 count backwards
    #[test]
    fn tile_rows_count_backwards_when_bit_4_is_off() {
        let mut ppu = Ppu::new();
        ppu.write_register(0xFF40, 0x80); // bit 4 off
        ppu.write_vram(0x9000, 0x0A);
        assert_eq!(ppu.tile_row(0, 0, 0).0, 0x0A, "tile 0 sits at 9000");

        ppu.write_vram(0x8800, 0x0B);
        assert_eq!(ppu.tile_row(128, 0, 0).0, 0x0B, "tile 128 is 128 back");

        ppu.write_vram(0x8FF0, 0x0C);
        assert_eq!(ppu.tile_row(255, 0, 0).0, 0x0C, "tile 255 is one back");
    }

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

        let irq = ppu.tick(OAM_SCAN_DOTS + MIN_DRAWING_DOTS); // into blank at end of line 0
        assert_eq!(irq & IF_STAT, 0, "line was already up from LY match");
    }

    #[test]
    fn line_has_to_drop_before_it_can_rise_again() {
        let mut ppu = Ppu::new();
        ppu.write_register(0xFF41, 0x08); // HBlank only
        ppu.write_register(0xFF45, 0xFF); // never matches

        let first = ppu.tick(OAM_SCAN_DOTS + MIN_DRAWING_DOTS);
        assert_eq!(first & IF_STAT, IF_STAT, "first blank asks for one");

        let same_blank = ppu.tick(DOTS_PER_LINE - OAM_SCAN_DOTS - MIN_DRAWING_DOTS);
        assert_eq!(same_blank & IF_STAT, 0, "still inside same blank");

        // drawing drops line, so next blank raises it again
        let next = ppu.tick(OAM_SCAN_DOTS + MIN_DRAWING_DOTS);
        assert_eq!(next & IF_STAT, IF_STAT);
    }

    // switching a source on while its condition already holds is itself a rise
    #[test]
    fn switching_a_source_on_can_ask_for_an_interrupt() {
        let mut ppu = Ppu::new();
        ppu.tick(OAM_SCAN_DOTS + MIN_DRAWING_DOTS); // sit in blank at end of line
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
        ppu.tick(OAM_SCAN_DOTS + MIN_DRAWING_DOTS);
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
        ppu.tick(MIN_DRAWING_DOTS); // on to blank at end of line
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

        ppu.tick(MIN_DRAWING_DOTS);
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
    // a letter as an 8 by 8 tile, where "#" is darkest shade
    fn glyph(letter: char) -> [&'static str; 8] {
        match letter {
            'd' => [
                "    #   ", "    #   ", " ####   ", "#   #   ", "#   #   ", "#   #   ", " ####   ",
                "        ",
            ],
            'u' => [
                "        ", "        ", "#   #   ", "#   #   ", "#   #   ", "#   #   ", " ####   ",
                "        ",
            ],
            's' => [
                "        ", "        ", " ####   ", "#       ", " ###    ", "    #   ", "####    ",
                "        ",
            ],
            _ => ["        "; 8],
        }
    }

    // put a word into video memory as tiles, then point map at them
    fn write_word(ppu: &mut Ppu, word: &str) {
        for (slot, letter) in word.chars().enumerate() {
            let tile = slot as u16 + 1;
            for (row, line) in glyph(letter).iter().enumerate() {
                let bits = line
                    .bytes()
                    .enumerate()
                    .fold(0u8, |b, (i, c)| if c == b'#' { b | 0x80 >> i } else { b });
                let at = 0x8000 + tile * 16 + row as u16 * 2;
                ppu.write_vram(at, bits); // both bytes set means darkest shade
                ppu.write_vram(at + 1, bits);
            }
            ppu.write_vram(0x9800 + slot as u16, tile as u8);
        }
    }

    // turn top left corner of picture back into text, so a human can read it
    fn picture(ppu: &Ppu, width: usize, height: usize) -> String {
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| {
                        if shade_at(ppu, x, y)[0] < 128 {
                            '#'
                        } else {
                            ' '
                        }
                    })
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    // whole path at once: tiles, map, fetcher, queue, palette, picture
    #[test]
    fn it_draws_a_word_on_screen() {
        let mut ppu = Ppu::new();
        ppu.write_register(0xFF40, 0x91); // screen on, background on, tiles from 8000
        ppu.write_register(0xFF47, 0xE4); // shades straight through: 0,1,2,3
        write_word(&mut ppu, "duds");
        ppu.tick(DOTS_PER_LINE * LINES_PER_FRAME as u32);

        assert_eq!(
            picture(&ppu, 64, 8),
            [
                "    #               #",
                "    #               #",
                " ####   #   #    ####    ####",
                "#   #   #   #   #   #   #",
                "#   #   #   #   #   #    ###",
                "#   #   #   #   #   #       #",
                " ####    ####    ####   ####",
                "",
            ]
            .join("\n")
        );
    }

    // a screen whose background is shade 1 everywhere and window is shade 3
    fn ppu_with_window() -> Ppu {
        let mut ppu = Ppu::new();
        ppu.write_register(0xFF40, 0xB1); // screen, background and window all on
        ppu.write_register(0xFF47, 0xE4); // shades straight through: 0,1,2,3
        for row in 0..8 {
            ppu.write_vram(0x8010 + row * 2, 0xFF); // tile 1 is shade 1 everywhere
            ppu.write_vram(0x8020 + row * 2, 0xFF); // tile 2 is shade 3 everywhere
            ppu.write_vram(0x8021 + row * 2, 0xFF);
        }
        for square in 0..32 * 32 {
            ppu.write_vram(0x9800 + square, 1); // whole map is background tile
            ppu.write_vram(0x9C00 + square, 2); // other map is window tile
        }
        ppu.write_register(0xFF40, 0xF1); // window reads map at 9C00
        ppu
    }

    #[test]
    fn window_covers_background_from_its_corner_onwards() {
        let mut ppu = ppu_with_window();
        ppu.write_register(0xFF4A, 0); // window starts on first line
        ppu.write_register(0xFF4B, 7 + 40); // and 40 pixels in
        ppu.tick(DOTS_PER_LINE);

        assert_eq!(shade_at(&ppu, 39, 0), SHADES[1], "background up to corner");
        assert_eq!(shade_at(&ppu, 40, 0), SHADES[3], "window from corner on");
        assert_eq!(shade_at(&ppu, 159, 0), SHADES[3], "window to end of line");
    }

    #[test]
    fn window_does_not_show_above_its_top_edge() {
        let mut ppu = ppu_with_window();
        ppu.write_register(0xFF4A, 1); // window only from second line down
        ppu.write_register(0xFF4B, 7);
        ppu.tick(DOTS_PER_LINE * 2);

        assert_eq!(shade_at(&ppu, 80, 0), SHADES[1], "first line is background");
        assert_eq!(shade_at(&ppu, 80, 1), SHADES[3], "second line is window");
    }

    // window keeps its own row count, so lines where it is off do not move it on
    #[test]
    fn window_counts_only_the_lines_it_drew() {
        let mut ppu = ppu_with_window();
        ppu.write_register(0xFF4A, 0);
        ppu.write_register(0xFF4B, 7);
        ppu.tick(DOTS_PER_LINE * 3);
        assert_eq!(
            ppu.fetcher.window_line, 3,
            "three lines drawn, three counted"
        );

        ppu.write_register(0xFF40, 0xD1); // window off
        ppu.tick(DOTS_PER_LINE * 2);
        assert_eq!(
            ppu.fetcher.window_line, 3,
            "two lines skipped, none counted"
        );
    }

    #[test]
    fn a_new_picture_starts_window_from_its_first_row() {
        let mut ppu = ppu_with_window();
        ppu.write_register(0xFF4A, 0);
        ppu.write_register(0xFF4B, 7);
        ppu.tick(DOTS_PER_LINE * LINES_PER_FRAME as u32);
        assert_eq!(ppu.fetcher.window_line, 0);
    }
    // switching to window throws away queue, so fetcher has to fill it again
    #[test]
    fn a_window_part_way_along_makes_drawing_longer() {
        let mut ppu = ppu_with_window();
        ppu.write_register(0xFF4A, 0);
        ppu.write_register(0xFF4B, 7 + 80);
        assert_eq!(drawing_dots(&mut ppu), MIN_DRAWING_DOTS + 6);
    }

    #[test]
    fn a_window_over_the_whole_line_still_takes_at_least_the_minimum() {
        for wx in 0..=7 {
            let mut ppu = ppu_with_window();
            ppu.write_register(0xFF4A, 0);
            ppu.write_register(0xFF4B, wx);
            assert!(drawing_dots(&mut ppu) >= MIN_DRAWING_DOTS, "wx {wx}");
        }
    }

    // WX of 7 puts window's left edge at very first pixel
    #[test]
    fn a_window_at_seven_covers_the_line_end_to_end() {
        let mut ppu = ppu_with_window();
        ppu.write_register(0xFF4A, 0);
        ppu.write_register(0xFF4B, 7);
        ppu.tick(DOTS_PER_LINE);
        assert_eq!(shade_at(&ppu, 0, 0), SHADES[3]);
        assert_eq!(shade_at(&ppu, 159, 0), SHADES[3]);
    }

    #[test]
    fn a_window_pushed_off_the_right_edge_never_shows() {
        let mut ppu = ppu_with_window();
        ppu.write_register(0xFF4A, 0);
        ppu.write_register(0xFF4B, 167);
        ppu.tick(DOTS_PER_LINE);
        assert_eq!(shade_at(&ppu, 159, 0), SHADES[1], "background to very end");
        assert_eq!(ppu.fetcher.window_line, 0, "and window counted no line");
    }

    // a game may switch window on half way down a picture
    #[test]
    fn a_window_switched_on_mid_picture_starts_drawing_there() {
        let mut ppu = ppu_with_window();
        ppu.write_register(0xFF40, 0xD1); // window off to begin with
        ppu.write_register(0xFF4A, 0);
        ppu.write_register(0xFF4B, 7);
        ppu.tick(DOTS_PER_LINE * 2);
        assert_eq!(shade_at(&ppu, 80, 1), SHADES[1]);

        ppu.write_register(0xFF40, 0xF1); // window on from third line
        ppu.tick(DOTS_PER_LINE);
        assert_eq!(shade_at(&ppu, 80, 2), SHADES[3]);
    }

    // window sitting over a word, so overlay can be seen rather than measured
    #[test]
    fn it_draws_a_window_over_the_background() {
        let mut ppu = Ppu::new();
        ppu.write_register(0xFF40, 0xF1); // screen, background and window on, window map at 9C00
        ppu.write_register(0xFF47, 0xE4); // shades straight through: 0,1,2,3
        write_word(&mut ppu, "duds");
        for row in 0..8 {
            ppu.write_vram(0x8090 + row * 2, 0xFF); // tile 9 is darkest everywhere
            ppu.write_vram(0x8091 + row * 2, 0xFF);
        }
        for square in 0..32 * 32 {
            ppu.write_vram(0x9C00 + square, 9);
        }
        ppu.write_register(0xFF4A, 4); // window starts half way down word
        ppu.write_register(0xFF4B, 7 + 32); // and after fourth tile
        ppu.tick(DOTS_PER_LINE * 8);

        assert_eq!(
            picture(&ppu, 48, 8),
            [
                "    #               #",
                "    #               #",
                " ####   #   #    ####    ####",
                "#   #   #   #   #   #   #",
                "#   #   #   #   #   #    ###    ################",
                "#   #   #   #   #   #       #   ################",
                " ####    ####    ####   ####    ################",
                "                                ################",
            ]
            .join("\n")
        );
    }

    // background of shade 1, plus two sprite pictures to put over it
    fn ppu_with_sprites() -> Ppu {
        let mut ppu = Ppu::new();
        ppu.write_register(0xFF40, 0x93); // screen, background and sprites on
        ppu.write_register(0xFF47, 0xE4); // background shades straight through
        ppu.write_register(0xFF48, 0xE4); // sprite palette 0 straight through
        ppu.write_register(0xFF49, 0x1B); // sprite palette 1 back to front
        for row in 0..8 {
            ppu.write_vram(0x8010 + row * 2, 0xFF); // tile 1 is shade 1 everywhere
            ppu.write_vram(0x8020 + row * 2, 0xFF); // tile 2 is shade 3 everywhere
            ppu.write_vram(0x8021 + row * 2, 0xFF);
            ppu.write_vram(0x8030 + row * 2, 0x0F); // tile 3 is clear then shade 3
            ppu.write_vram(0x8031 + row * 2, 0x0F);
        }
        for square in 0..32 * 32 {
            ppu.write_vram(0x9800 + square, 1);
        }
        ppu
    }

    // one sprite entry: y, x, tile, attributes. written straight in, since
    // a real game would only get at the table while screen is resting.
    fn put_sprite(ppu: &mut Ppu, slot: usize, sprite: (u8, u8, u8, u8)) {
        let at = slot * 4;
        ppu.oam[at] = sprite.0;
        ppu.oam[at + 1] = sprite.1;
        ppu.oam[at + 2] = sprite.2;
        ppu.oam[at + 3] = sprite.3;
    }

    #[test]
    fn a_sprite_is_drawn_over_the_background() {
        let mut ppu = ppu_with_sprites();
        put_sprite(&mut ppu, 0, (16, 8 + 40, 2, 0));
        ppu.tick(DOTS_PER_LINE);

        assert_eq!(shade_at(&ppu, 39, 0), SHADES[1], "background up to sprite");
        assert_eq!(shade_at(&ppu, 40, 0), SHADES[3], "sprite starts here");
        assert_eq!(shade_at(&ppu, 47, 0), SHADES[3], "and is eight wide");
        assert_eq!(shade_at(&ppu, 48, 0), SHADES[1], "background again after");
    }

    // colour 0 of a sprite is see through, unlike background where it is a shade
    #[test]
    fn a_sprites_clear_pixels_let_the_background_show() {
        let mut ppu = ppu_with_sprites();
        put_sprite(&mut ppu, 0, (16, 8 + 40, 3, 0));
        ppu.tick(DOTS_PER_LINE);

        assert_eq!(
            shade_at(&ppu, 41, 0),
            SHADES[1],
            "clear half shows background"
        );
        assert_eq!(shade_at(&ppu, 45, 0), SHADES[3], "solid half shows sprite");
    }

    #[test]
    fn a_sprite_can_use_either_of_its_two_palettes() {
        let mut ppu = ppu_with_sprites();
        put_sprite(&mut ppu, 0, (16, 8 + 40, 2, 0x10)); // attribute bit 4 picks palette 1
        ppu.tick(DOTS_PER_LINE);
        assert_eq!(
            shade_at(&ppu, 40, 0),
            SHADES[0],
            "palette 1 turns shade 3 into 0"
        );
    }

    #[test]
    fn sprites_vanish_when_their_switch_is_off() {
        let mut ppu = ppu_with_sprites();
        put_sprite(&mut ppu, 0, (16, 8 + 40, 2, 0));
        ppu.write_register(0xFF40, 0x91); // sprite bit off
        ppu.tick(DOTS_PER_LINE);
        assert_eq!(shade_at(&ppu, 40, 0), SHADES[1]);
    }

    // where two sprites land on same spot, one earlier in table wins
    #[test]
    fn the_earlier_sprite_in_the_table_wins_an_overlap() {
        let mut ppu = ppu_with_sprites();
        put_sprite(&mut ppu, 0, (16, 8 + 40, 3, 0x10)); // clear left, palette 1
        put_sprite(&mut ppu, 1, (16, 8 + 40, 2, 0)); // solid, palette 0
        ppu.tick(DOTS_PER_LINE);

        assert_eq!(
            shade_at(&ppu, 45, 0),
            SHADES[0],
            "first sprite keeps its pixels"
        );
        assert_eq!(
            shade_at(&ppu, 41, 0),
            SHADES[3],
            "second shows through first's clear half"
        );
    }

    #[test]
    fn a_tall_sprite_is_two_tiles_stacked() {
        let mut ppu = ppu_with_sprites();
        put_sprite(&mut ppu, 0, (16, 8 + 40, 2, 0));
        ppu.write_register(0xFF40, 0x97); // sprites are 8 by 16 now
        ppu.tick(DOTS_PER_LINE * 9);
        assert_eq!(
            shade_at(&ppu, 40, 0),
            SHADES[3],
            "first line is tile 2, solid"
        );
        assert_eq!(
            shade_at(&ppu, 41, 8),
            SHADES[1],
            "ninth line is tile 3, clear half"
        );
        assert_eq!(shade_at(&ppu, 45, 8), SHADES[3], "and tile 3's solid half");
    }

    #[test]
    fn only_ten_sprites_land_on_one_line() {
        let mut ppu = ppu_with_sprites();
        for slot in 0..12usize {
            put_sprite(&mut ppu, slot, (16, 8 + slot as u8 * 8, 2, 0));
        }
        ppu.tick(DOTS_PER_LINE);

        assert_eq!(shade_at(&ppu, 72, 0), SHADES[3], "tenth sprite still draws");
        assert_eq!(shade_at(&ppu, 80, 0), SHADES[1], "eleventh is dropped");
    }

    // tile 3 is clear on its left half and solid on its right
    #[test]
    fn a_sprite_can_be_mirrored_left_to_right() {
        let mut ppu = ppu_with_sprites();
        put_sprite(&mut ppu, 0, (16, 8 + 40, 3, 0x20));
        ppu.tick(DOTS_PER_LINE);

        assert_eq!(
            shade_at(&ppu, 41, 0),
            SHADES[3],
            "solid half is on left now"
        );
        assert_eq!(shade_at(&ppu, 45, 0), SHADES[1], "and clear half on right");
    }

    #[test]
    fn a_sprite_can_be_turned_upside_down() {
        let mut ppu = ppu_with_sprites();
        for row in 0..8 {
            ppu.write_vram(0x8040 + row * 2, if row == 0 { 0xFF } else { 0 }); // tile 4 solid on top row only
            ppu.write_vram(0x8041 + row * 2, if row == 0 { 0xFF } else { 0 });
        }
        put_sprite(&mut ppu, 0, (16, 8 + 40, 4, 0x40));
        ppu.tick(DOTS_PER_LINE * 8);

        assert_eq!(shade_at(&ppu, 40, 0), SHADES[1], "its top row is empty now");
        assert_eq!(
            shade_at(&ppu, 40, 7),
            SHADES[3],
            "solid row moved to bottom"
        );
    }

    // attribute bit 7 puts a sprite under any background that is not blank
    #[test]
    fn a_low_sprite_hides_behind_the_background() {
        let mut ppu = ppu_with_sprites();
        put_sprite(&mut ppu, 0, (16, 8 + 40, 2, 0x80));
        ppu.tick(DOTS_PER_LINE);
        assert_eq!(shade_at(&ppu, 40, 0), SHADES[1], "background wins");
    }

    #[test]
    fn a_low_sprite_still_shows_over_blank_background() {
        let mut ppu = ppu_with_sprites();
        for square in 0..32 * 32 {
            ppu.write_vram(0x9800 + square, 0); // map now points at an empty tile
        }
        put_sprite(&mut ppu, 0, (16, 8 + 40, 2, 0x80));
        ppu.tick(DOTS_PER_LINE);
        assert_eq!(shade_at(&ppu, 40, 0), SHADES[3], "nothing to hide behind");
    }

    #[test]
    fn switching_the_background_off_lets_every_sprite_through() {
        let mut ppu = ppu_with_sprites();
        put_sprite(&mut ppu, 0, (16, 8 + 40, 2, 0x80));
        ppu.write_register(0xFF40, 0x92); // background bit off, sprites on
        ppu.tick(DOTS_PER_LINE);
        assert_eq!(shade_at(&ppu, 40, 0), SHADES[3]);
    }

    #[test]
    fn a_tall_sprite_swaps_its_halves_when_turned_over() {
        let mut ppu = ppu_with_sprites();
        put_sprite(&mut ppu, 0, (16, 8 + 40, 2, 0x40));
        ppu.write_register(0xFF40, 0x97); // sprites are 8 by 16 now
        ppu.tick(DOTS_PER_LINE * 9);

        assert_eq!(shade_at(&ppu, 41, 0), SHADES[1], "lower tile is on top now");
        assert_eq!(shade_at(&ppu, 40, 8), SHADES[3], "and upper tile below");
    }
}
