//! The two pixel queues the mixer takes from. See `docs/architecture.md` 5.2.

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Pixel {
    /// Which of the 4 palette slots this pixel uses; the colour is looked up later.
    pub color: u8,
    pub palette: u8,
    /// Who wins when a background pixel and a sprite pixel land on the same spot.
    pub priority: bool,
}

const CAPACITY: usize = 16;

#[derive(Debug)]
pub struct PixelFifo {
    /// Nothing reads this until the mixer is written.
    #[allow(dead_code, reason = "TODO(PR-14): read by push/pop")]
    queue: [Pixel; CAPACITY],
    head: usize,
    len: usize,
}

impl PixelFifo {
    pub fn new() -> Self {
        Self {
            queue: [Pixel::default(); CAPACITY],
            head: 0,
            len: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn clear(&mut self) {
        self.head = 0;
        self.len = 0;
    }

    // TODO(PR-14): the fetcher adds 8 pixels at a time, the mixer takes 1 per dot.
    pub fn push(&mut self, _pixel: Pixel) {
        todo!("PR-14: background FIFO")
    }

    pub fn pop(&mut self) -> Option<Pixel> {
        todo!("PR-14: background FIFO")
    }
}

impl Default for PixelFifo {
    fn default() -> Self {
        Self::new()
    }
}
