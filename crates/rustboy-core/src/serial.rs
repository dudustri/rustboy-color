//! The link cable port. Nothing is plugged in, but test ROMs print their results through it.

pub struct Serial {
    sb: u8,          // FF01 the byte being sent or received
    sc: u8,          // FF02 starts a transfer and picks the clock
    output: Vec<u8>, // everything sent out; test ROMs print their result here
}

impl Serial {
    pub fn new() -> Self {
        Self {
            sb: 0,
            sc: 0,
            output: Vec::new(),
        }
    }

    pub fn tick(&mut self, _t_cycles: u32) -> u8 {
        0 // TODO(PR-12): time the transfer and raise the interrupt
    }

    pub fn output(&self) -> &[u8] {
        &self.output
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0xFF01 => self.sb,
            0xFF02 => self.sc | 0x7E,
            _ => 0xFF,
        }
    }

    pub fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0xFF01 => self.sb = value,
            0xFF02 => {
                self.sc = value;
                if value & 0x81 == 0x81 {
                    self.output.push(self.sb);
                }
            }
            _ => {}
        }
    }
}

impl Default for Serial {
    fn default() -> Self {
        Self::new()
    }
}
