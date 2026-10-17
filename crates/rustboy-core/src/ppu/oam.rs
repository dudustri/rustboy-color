//! before drawing a line, screen picks at most ten sprites that touch it.

pub const MAX_SPRITES_PER_LINE: usize = 10;

#[derive(Debug, Clone, Copy, Default)]
pub struct Sprite {
    pub y: u8,          // stored 16 lower than it looks, so sprites can slide in from top
    pub x: u8,          // stored 8 further right, so sprites can slide in from left
    pub tile: u8,       // which picture to draw
    pub attributes: u8, // palette, flips, and whether background covers it
}

#[derive(Debug)]
pub struct SpriteScan {
    sprites: [Sprite; MAX_SPRITES_PER_LINE], // ten slots
    len: usize,                              // how many of them this line filled
}

impl SpriteScan {
    pub fn new() -> Self {
        Self {
            sprites: [Sprite::default(); MAX_SPRITES_PER_LINE],
            len: 0,
        }
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    pub fn visible(&self) -> &[Sprite] {
        &self.sprites[..self.len]
    }

    /// walk all 40 sprites in order and keep first ten that touch this line.
    pub fn scan(&mut self, oam: &[u8], ly: u8, tall_sprites: bool) {
        self.len = 0;
        let height = if tall_sprites { 16 } else { 8 };
        // y is stored 16 too high, so compare in that same shifted space
        let row = ly as u16 + 16;
        for entry in oam.chunks_exact(4) {
            if self.len == MAX_SPRITES_PER_LINE {
                break; // hardware gives up after ten, rest of line simply misses out
            }
            let y = entry[0] as u16;
            if row < y || row >= y + height {
                continue;
            }
            self.sprites[self.len] = Sprite {
                y: entry[0],
                x: entry[1],
                tile: entry[2],
                attributes: entry[3],
            };
            self.len += 1;
        }
    }
}

impl Default for SpriteScan {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SCREEN_HEIGHT;

    // one sprite table, each entry y, x, tile, attributes
    fn oam(entries: &[(u8, u8, u8, u8)]) -> Vec<u8> {
        let mut bytes = vec![0; 40 * 4];
        for (slot, &(y, x, tile, attributes)) in entries.iter().enumerate() {
            bytes[slot * 4..slot * 4 + 4].copy_from_slice(&[y, x, tile, attributes]);
        }
        bytes
    }

    // y of 16 puts a sprite's top on first line of screen
    #[test]
    fn a_short_sprite_covers_eight_lines() {
        let bytes = oam(&[(16, 8, 1, 0)]);
        let mut scan = SpriteScan::new();
        for line in 0..8 {
            scan.scan(&bytes, line, false);
            assert_eq!(scan.visible().len(), 1, "line {line}");
        }
        scan.scan(&bytes, 8, false);
        assert!(scan.visible().is_empty(), "ninth line is past its bottom");
    }

    #[test]
    fn a_tall_sprite_covers_sixteen() {
        let bytes = oam(&[(16, 8, 1, 0)]);
        let mut scan = SpriteScan::new();
        scan.scan(&bytes, 15, true);
        assert_eq!(scan.visible().len(), 1);
        scan.scan(&bytes, 16, true);
        assert!(scan.visible().is_empty());
    }

    // y under 16 means a sprite is partly above screen, showing only its bottom
    #[test]
    fn a_sprite_can_slide_in_from_the_top() {
        let bytes = oam(&[(13, 8, 1, 0)]);
        let mut scan = SpriteScan::new();
        scan.scan(&bytes, 0, false);
        assert_eq!(scan.visible().len(), 1, "bottom five rows show");
        scan.scan(&bytes, 5, false);
        assert!(scan.visible().is_empty());
    }

    #[test]
    fn a_sprite_hidden_at_zero_never_turns_up() {
        let bytes = oam(&[(0, 8, 1, 0)]);
        let mut scan = SpriteScan::new();
        for line in 0..SCREEN_HEIGHT as u8 {
            scan.scan(&bytes, line, false);
            assert!(scan.visible().is_empty(), "line {line}");
        }
    }

    // a line can only hold ten, and it is first ten in table that win
    #[test]
    fn only_the_first_ten_of_a_line_are_kept() {
        let crowd: Vec<_> = (0..20).map(|i| (16, i * 4, i, 0)).collect();
        let mut scan = SpriteScan::new();
        scan.scan(&oam(&crowd), 0, false);

        assert_eq!(scan.visible().len(), MAX_SPRITES_PER_LINE);
        let kept: Vec<u8> = scan.visible().iter().map(|s| s.tile).collect();
        assert_eq!(
            kept,
            (0..10).collect::<Vec<u8>>(),
            "table order, not x order"
        );
    }

    // ten is counted per line, so a crowd on one line leaves next line free
    #[test]
    fn each_line_gets_its_own_ten() {
        let mut entries: Vec<_> = (0..10).map(|i| (16, i * 4, i, 0)).collect();
        entries.push((17, 100, 99, 0)); // only touches lines 1 to 8
        let bytes = oam(&entries);
        let mut scan = SpriteScan::new();

        scan.scan(&bytes, 0, false);
        assert_eq!(scan.visible().len(), 10, "crowd fills first line");
        scan.scan(&bytes, 8, false);
        assert_eq!(scan.visible()[0].tile, 99, "late sprite gets in lower down");
    }

    #[test]
    fn a_scan_forgets_what_the_line_before_found() {
        let bytes = oam(&[(16, 8, 1, 0)]);
        let mut scan = SpriteScan::new();
        scan.scan(&bytes, 0, false);
        scan.scan(&bytes, 100, false);
        assert!(scan.visible().is_empty());
    }
}
