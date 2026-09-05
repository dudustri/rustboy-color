//! The desktop host: opens a window and will show what the core draws.

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

const SCREEN_WIDTH: u32 = 160; // the real console's screen, in pixels
const SCREEN_HEIGHT: u32 = 144;
const SCALE: u32 = 4; // every console pixel becomes a 4 by 4 block

struct App {
    window: Option<Window>, // None until the event loop hands us one
}

impl ApplicationHandler for App {
    // Called once when the event loop is ready to give us a window.
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let size = LogicalSize::new(SCREEN_WIDTH * SCALE, SCREEN_HEIGHT * SCALE);
        let attributes = Window::default_attributes()
            .with_title("rustboy-color")
            .with_inner_size(size)
            .with_min_inner_size(LogicalSize::new(SCREEN_WIDTH, SCREEN_HEIGHT));

        match event_loop.create_window(attributes) {
            Ok(window) => self.window = Some(window),
            Err(error) => {
                eprintln!("could not open a window: {error}");
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::KeyboardInput { event, .. }
                if event.state.is_pressed()
                    && event.logical_key == Key::Named(NamedKey::Escape) =>
            {
                event_loop.exit();
            }
            _ => {}
        }
    }
}

fn main() {
    let event_loop = match EventLoop::new() {
        Ok(event_loop) => event_loop,
        Err(error) => {
            eprintln!("could not start the event loop: {error}");
            return;
        }
    };

    // Poll rather than wait, because the emulator will need to run every frame.
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App { window: None };
    if let Err(error) = event_loop.run_app(&mut app) {
        eprintln!("the event loop stopped: {error}");
    }
}
