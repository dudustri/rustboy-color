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
    pub step: FetchStep,
    pub tile_x: u8,
    pub tile_number: u8,
    pub data_low: u8,
    pub data_high: u8,
    /// The window counts its own lines, separately from the line being drawn.
    pub window_line: u8,
    /// Every step lasts two dots; this marks the second one.
    pub second_dot: bool,
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
