//! The desktop host: a window, a keyboard, and nothing else.
//!
//! Keys: arrows, A and X for A and B, Enter, Shift, F11 fullscreen, Escape quits.
//!
//! Pass a game on the command line: `cargo run -p rustboy-desktop -- game.gbc`

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use pixels::{Pixels, SurfaceTexture};
use rustboy_core::{Button, SCREEN_HEIGHT, SCREEN_WIDTH, T_CYCLES_PER_FRAME, T_CYCLES_PER_SECOND};
use rustboy_frontend::{Frontend, Host};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Fullscreen, Window, WindowButtons, WindowId};

const SCALE: u32 = 4; // every console pixel becomes a 4 by 4 block

// One frame on the real console: 70,224 ticks at 4,194,304 a second, just under 59.73 a second.
const FRAME_TIME: Duration =
    Duration::from_nanos((T_CYCLES_PER_FRAME as u64 * 1_000_000_000) / T_CYCLES_PER_SECOND as u64);

// Everything the shared frontend needs from this platform.
struct Desktop {
    window: Arc<Window>,
    pixels: Pixels<'static>,
    started: Instant,
    next_frame: Instant, // when the next frame is due
}

impl Host for Desktop {
    fn elapsed(&self) -> f32 {
        self.started.elapsed().as_secs_f32()
    }

    fn frame(&mut self) -> &mut [u8] {
        self.pixels.frame_mut()
    }

    fn present(&mut self) {
        if let Err(error) = self.pixels.render() {
            eprintln!("could not draw the frame: {error}");
        }
    }
}

struct App {
    frontend: Frontend,
    desktop: Option<Desktop>, // None until the event loop hands us a window
}

impl App {
    fn new() -> Self {
        Self {
            frontend: Frontend::new(),
            desktop: None,
        }
    }

    // Fill the screen or go back to a window, keeping the monitor's current resolution.
    fn toggle_fullscreen(&self) {
        let Some(desktop) = self.desktop.as_ref() else {
            return;
        };
        let next = match desktop.window.fullscreen() {
            Some(_) => None,
            None => Some(Fullscreen::Borderless(None)), // None means the current monitor
        };
        desktop.window.set_fullscreen(next);
    }
}

// Which key works which button. Anything else is ignored.
fn button_for(key: &Key) -> Option<Button> {
    match key {
        Key::Named(NamedKey::ArrowRight) => Some(Button::Right),
        Key::Named(NamedKey::ArrowLeft) => Some(Button::Left),
        Key::Named(NamedKey::ArrowUp) => Some(Button::Up),
        Key::Named(NamedKey::ArrowDown) => Some(Button::Down),
        Key::Named(NamedKey::Enter) => Some(Button::Start),
        Key::Named(NamedKey::Shift) => Some(Button::Select),
        Key::Character(c) if c.eq_ignore_ascii_case("a") => Some(Button::A),
        Key::Character(c) if c.eq_ignore_ascii_case("x") => Some(Button::B),
        _ => None,
    }
}

impl ApplicationHandler for App {
    // Called once when the event loop is ready to give us a window.
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let width = SCREEN_WIDTH as u32;
        let height = SCREEN_HEIGHT as u32;
        let attributes = Window::default_attributes()
            .with_title("rustboy-color")
            .with_inner_size(LogicalSize::new(width * SCALE, height * SCALE))
            .with_min_inner_size(LogicalSize::new(width, height))
            // Ask for minimise and maximise; whether they are drawn is up to the desktop.
            .with_enabled_buttons(WindowButtons::all());

        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                eprintln!("could not open a window: {error}");
                event_loop.exit();
                return;
            }
        };

        // The window is bigger than the console; pixels stretches one onto the other.
        let size = window.inner_size();
        let surface = SurfaceTexture::new(size.width, size.height, Arc::clone(&window));
        match Pixels::new(width, height, surface) {
            Ok(pixels) => {
                let now = Instant::now();
                self.desktop = Some(Desktop {
                    window,
                    pixels,
                    started: now,
                    next_frame: now,
                })
            }
            Err(error) => {
                eprintln!("could not set up drawing: {error}");
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::KeyboardInput { event, .. } => {
                let pressed = event.state.is_pressed();
                match &event.logical_key {
                    Key::Named(NamedKey::Escape) if pressed => event_loop.exit(),
                    Key::Named(NamedKey::F11) if pressed => self.toggle_fullscreen(),
                    key => {
                        if pressed {
                            self.frontend.skip_splash(); // any game key cuts the title short
                        }
                        if let Some(button) = button_for(key) {
                            self.frontend.set_button(button, pressed);
                        }
                    }
                }
            }
            WindowEvent::Resized(size) => {
                if let Some(desktop) = self.desktop.as_mut()
                    && let Err(error) = desktop.pixels.resize_surface(size.width, size.height)
                {
                    eprintln!("could not resize: {error}");
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(desktop) = self.desktop.as_mut() {
                    self.frontend.tick(desktop);
                }
            }
            _ => {}
        }
    }

    // Draw only when a frame is actually due, then sleep until the next one.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(desktop) = self.desktop.as_mut() else {
            return;
        };

        let now = Instant::now();
        if now >= desktop.next_frame {
            desktop.next_frame += FRAME_TIME;
            // After a stall, start again from now rather than racing to catch up.
            if desktop.next_frame < now {
                desktop.next_frame = now + FRAME_TIME;
            }
            desktop.window.request_redraw();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(desktop.next_frame));
    }
}

// Read the game named on the command line, if there is one.
fn load_rom(frontend: &mut Frontend) -> Result<(), String> {
    let Some(path) = std::env::args_os().nth(1).map(PathBuf::from) else {
        return Ok(()); // no game is fine; the console shows a blank screen
    };

    let rom =
        std::fs::read(&path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    frontend
        .load_rom(rom)
        .map_err(|e| format!("{} is not a game we can run: {e}", path.display()))
}

fn main() {
    let mut app = App::new();
    if let Err(problem) = load_rom(&mut app.frontend) {
        eprintln!("{problem}");
        return;
    }
    match app.frontend.title() {
        Some(title) => println!("loaded {title}"),
        None => println!("no game given, showing a blank screen"),
    }

    let event_loop = match EventLoop::new() {
        Ok(event_loop) => event_loop,
        Err(error) => {
            eprintln!("could not start the event loop: {error}");
            return;
        }
    };

    // Replaced each round by a deadline for the next frame.
    event_loop.set_control_flow(ControlFlow::Poll);

    if let Err(error) = event_loop.run_app(&mut app) {
        eprintln!("the event loop stopped: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_button_has_a_key() {
        for (key, button) in [
            (Key::Named(NamedKey::ArrowUp), Button::Up),
            (Key::Named(NamedKey::Enter), Button::Start),
            (Key::Named(NamedKey::Shift), Button::Select),
            (Key::Character("a".into()), Button::A),
            (Key::Character("x".into()), Button::B),
        ] {
            assert_eq!(button_for(&key), Some(button));
        }
    }

    #[test]
    fn holding_shift_still_works() {
        assert_eq!(button_for(&Key::Character("A".into())), Some(Button::A));
    }
}
