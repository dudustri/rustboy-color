//! Builds background pixels 8 at a time, in five steps of two dots each.

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FetchStep {
    #[default]
    TileNumber,
    TileDataLow,
    TileDataHigh,
    Sleep,
    Push,
}

#[derive(Debug, Default)]
pub struct Fetcher {
    pub step: FetchStep,  // which of the five steps we are on
    pub tile_x: u8,       // how many tiles along the line we have fetched
    pub tile_number: u8,  // which picture the map told us to draw
    pub data_low: u8,     // first half of the 8 pixels
    pub data_high: u8,    // second half; the two together give each pixel its colour
    pub window_line: u8,  // the window counts its own lines, apart from the one being drawn
    pub second_dot: bool, // every step lasts two dots; this marks the second
}

impl Fetcher {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start over, at the beginning of a line or when the window switches on.
    pub fn restart(&mut self) {
        self.step = FetchStep::TileNumber;
        self.tile_x = 0;
        self.second_dot = false;
    }

    // TODO(PR-14): move one dot forward through the five steps.
    pub fn tick(&mut self) {
        todo!("PR-14: background fetcher")
    }
}
