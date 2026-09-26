//! two pixel queues mixer takes from. See `docs/architecture.md` 5.2.

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Pixel {
    pub color: u8,      // which of 4 palette slots to use; colour comes later
    pub palette: u8,    // which palette to look it up in
    pub priority: bool, // who wins when a background pixel and a sprite land on same spot
}

const CAPACITY: usize = 16;

#[derive(Debug)]
pub struct PixelFifo {
    queue: [Pixel; CAPACITY], // ring of waiting pixels
    head: usize,              // where next pixel comes out
    len: usize,               // how many are waiting
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

    /// add one pixel at back. a full queue keeps what it has and drops this one.
    pub fn push(&mut self, pixel: Pixel) {
        if self.len == CAPACITY {
            return;
        }
        self.queue[(self.head + self.len) % CAPACITY] = pixel;
        self.len += 1;
    }

    /// take next pixel off front, or `None` when nothing is waiting.
    pub fn pop(&mut self) -> Option<Pixel> {
        if self.len == 0 {
            return None;
        }
        let pixel = self.queue[self.head];
        self.head = (self.head + 1) % CAPACITY;
        self.len -= 1;
        Some(pixel)
    }
}

impl Default for PixelFifo {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(color: u8) -> Pixel {
        Pixel {
            color,
            palette: 0,
            priority: false,
        }
    }

    #[test]
    fn pixels_come_out_in_order_they_went_in() {
        let mut fifo = PixelFifo::new();
        for colour in 0..3 {
            fifo.push(pixel(colour));
        }
        assert_eq!(fifo.len(), 3);

        assert_eq!(fifo.pop(), Some(pixel(0)));
        assert_eq!(fifo.pop(), Some(pixel(1)));
        assert_eq!(fifo.pop(), Some(pixel(2)));
        assert!(fifo.is_empty());
    }

    #[test]
    fn an_empty_queue_hands_out_nothing() {
        let mut fifo = PixelFifo::new();
        assert_eq!(fifo.pop(), None);
    }

    // fetcher adds 8 at a time while mixer takes 1 per dot, so it wraps round often
    #[test]
    fn it_keeps_order_while_wrapping_round() {
        let mut fifo = PixelFifo::new();
        for round in 0..4 {
            for step in 0..8 {
                fifo.push(pixel((round * 8 + step) % 4));
            }
            for step in 0..8 {
                assert_eq!(fifo.pop(), Some(pixel((round * 8 + step) % 4)));
            }
        }
        assert!(fifo.is_empty());
    }

    #[test]
    fn a_full_queue_drops_what_will_not_fit() {
        let mut fifo = PixelFifo::new();
        for _ in 0..CAPACITY {
            fifo.push(pixel(1));
        }
        fifo.push(pixel(2));

        assert_eq!(fifo.len(), CAPACITY);
        assert_eq!(fifo.pop(), Some(pixel(1)), "oldest is still first");
    }

    #[test]
    fn clearing_throws_everything_away() {
        let mut fifo = PixelFifo::new();
        fifo.push(pixel(3));
        fifo.clear();
        assert!(fifo.is_empty());
        assert_eq!(fifo.pop(), None);
    }
}
