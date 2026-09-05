//! Before drawing a line, the screen picks at most ten sprites that touch it.

pub const MAX_SPRITES_PER_LINE: usize = 10;

#[derive(Debug, Clone, Copy, Default)]
pub struct Sprite {
    pub y: u8,          // stored 16 lower than it looks, so sprites can slide in from the top
    pub x: u8,          // stored 8 further right, so sprites can slide in from the left
    pub tile: u8,       // which picture to draw
    pub attributes: u8, // palette, flips, and whether the background covers it
}

#[derive(Debug)]
pub struct SpriteScan {
    sprites: [Sprite; MAX_SPRITES_PER_LINE], // the ten slots
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

    // TODO(PR-16): look at all 40 sprites and keep the first ten that touch this line.
    pub fn scan(&mut self, _oam: &[u8], _ly: u8, _tall_sprites: bool) {
        todo!("PR-16: sprites")
    }
}

impl Default for SpriteScan {
    fn default() -> Self {
        Self::new()
    }
}
