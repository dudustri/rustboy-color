//! The desktop host: a window, a keyboard, and nothing else.
//!
//! Keys: arrows move, X and Z are A and B, Enter starts, Shift selects,
//! F11 fills the screen, Escape quits.
//!
//! Pass a game on the command line: `cargo run -p rustboy-desktop -- game.gbc`

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use pixels::{Pixels, SurfaceTexture};
use rustboy_core::{Button, SCREEN_HEIGHT, SCREEN_WIDTH};
use rustboy_frontend::{Frontend, Host};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Fullscreen, Window, WindowButtons, WindowId};

const SCALE: u32 = 4; // every console pixel becomes a 4 by 4 block

// Everything the shared frontend needs from this platform.
struct Desktop {
    window: Arc<Window>,
    pixels: Pixels<'static>,
    started: Instant,
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

    // Fill the screen, or go back to a window. Borderless keeps whatever
    // resolution the monitor is already using, so nothing has to change mode.
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
        Key::Character(c) if c == "x" => Some(Button::A),
        Key::Character(c) if c == "z" => Some(Button::B),
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
            // Ask for minimise and maximise beside the close button. Whether they
            // are actually drawn is up to the desktop.
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
                self.desktop = Some(Desktop {
                    window,
                    pixels,
                    started: Instant::now(),
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

    // Ask for another frame as soon as this one is done.
    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(desktop) = self.desktop.as_ref() {
            desktop.window.request_redraw();
        }
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

    // Poll rather than wait, because there is a frame to run every time round.
    event_loop.set_control_flow(ControlFlow::Poll);

    if let Err(error) = event_loop.run_app(&mut app) {
        eprintln!("the event loop stopped: {error}");
    }
}
