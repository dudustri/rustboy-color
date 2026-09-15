use super::*;

// A pretend platform that records what it was asked to do.
struct Fake {
    seconds: f32,
    frame: Vec<u8>,
    presented: usize,
}

impl Fake {
    fn new() -> Self {
        Self {
            seconds: 0.0,
            frame: vec![0; FRAMEBUFFER_LEN],
            presented: 0,
        }
    }
}

impl Host for Fake {
    fn elapsed(&self) -> f32 {
        self.seconds
    }
    fn frame(&mut self) -> &mut [u8] {
        &mut self.frame
    }
    fn present(&mut self) {
        self.presented += 1;
    }
}

#[test]
fn the_first_frames_are_the_title_screen() {
    let mut frontend = Frontend::new();
    let mut host = Fake::new();
    host.seconds = rustboy_splash::SECONDS / 2.0;
    frontend.tick(&mut host);
    assert!(!frontend.splash_over);
    assert_eq!(host.presented, 1);
}

#[test]
fn the_console_takes_over_when_the_title_ends() {
    let mut frontend = Frontend::new();
    let mut host = Fake::new();
    host.seconds = rustboy_splash::SECONDS;
    frontend.tick(&mut host);
    assert!(frontend.splash_over);
}

// Once the title is over it must not come back, even if the clock says so.
#[test]
fn the_title_screen_never_returns() {
    let mut frontend = Frontend::new();
    let mut host = Fake::new();
    frontend.skip_splash();
    host.seconds = 0.0;
    frontend.tick(&mut host);
    assert!(frontend.splash_over);
}
