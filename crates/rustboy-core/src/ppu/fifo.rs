//! The two pixel queues the mixer takes from. See `docs/architecture.md` 5.2.

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Pixel {
    pub color: u8,      // which of the 4 palette slots to use; the colour comes later
    pub palette: u8,    // which palette to look it up in
    pub priority: bool, // who wins when a background pixel and a sprite land on the same spot
}

const CAPACITY: usize = 16;

#[derive(Debug)]
pub struct PixelFifo {
    #[allow(dead_code, reason = "TODO(PR-14): read by push/pop")]
    queue: [Pixel; CAPACITY], // the ring of waiting pixels
    head: usize, // where the next pixel comes out
    len: usize,  // how many are waiting
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
