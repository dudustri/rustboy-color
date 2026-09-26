//! builds background pixels 8 at a time, in five steps of two dots each.

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
    pub step: FetchStep,  // which of five steps we are on
    pub tile_x: u8,       // how many tiles along line we have fetched
    pub tile_number: u8,  // which picture map told us to draw
    pub data_low: u8,     // first half of 8 pixels
    pub data_high: u8,    // second half; two together give each pixel its colour
    pub window_line: u8,  // window counts its own lines, apart from one being drawn
    pub second_dot: bool, // every step lasts two dots; this marks second
}

impl Fetcher {
    pub fn new() -> Self {
        Self::default()
    }

    /// start over, at beginning of a line or when window switches on.
    pub fn restart(&mut self) {
        self.step = FetchStep::TileNumber;
        self.tile_x = 0;
        self.second_dot = false;
    }

    /// first four steps last two dots each. true on second dot, when step happens.
    pub fn ready(&mut self) -> bool {
        self.second_dot = !self.second_dot;
        !self.second_dot
    }

    /// move on to next of five steps.
    pub fn next(&mut self) {
        self.step = match self.step {
            FetchStep::TileNumber => FetchStep::TileDataLow,
            FetchStep::TileDataLow => FetchStep::TileDataHigh,
            FetchStep::TileDataHigh => FetchStep::Sleep,
            FetchStep::Sleep => FetchStep::Push,
            FetchStep::Push => FetchStep::TileNumber,
        };
    }
}
