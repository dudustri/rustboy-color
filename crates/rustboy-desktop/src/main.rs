//! The desktop host: opens a window and shows what the core draws.

use std::sync::Arc;

use pixels::{Pixels, SurfaceTexture};
use rustboy_core::{Emulator, SCREEN_HEIGHT, SCREEN_WIDTH};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

const SCALE: u32 = 4; // every console pixel becomes a 4 by 4 block

struct App {
    emulator: Emulator,
    // Both are None until the event loop hands us a window.
    window: Option<Arc<Window>>,
    pixels: Option<Pixels<'static>>,
}

impl App {
    fn new() -> Self {
        Self {
            emulator: Emulator::new(),
            window: None,
            pixels: None,
        }
    }

    // Run the console for one frame, then copy its picture into the window.
    fn draw(&mut self) {
        let Some(pixels) = self.pixels.as_mut() else {
            return;
        };
        self.emulator.run_frame();
        pixels
            .frame_mut()
            .copy_from_slice(self.emulator.framebuffer());
        if let Err(error) = pixels.render() {
            eprintln!("could not draw the frame: {error}");
        }
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
            .with_min_inner_size(LogicalSize::new(width, height));

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
            Ok(pixels) => self.pixels = Some(pixels),
            Err(error) => {
                eprintln!("could not set up drawing: {error}");
                event_loop.exit();
                return;
            }
        }
        self.window = Some(window);
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
            WindowEvent::Resized(size) => {
                if let Some(pixels) = self.pixels.as_mut()
                    && let Err(error) = pixels.resize_surface(size.width, size.height)
                {
                    eprintln!("could not resize: {error}");
                }
            }
            WindowEvent::RedrawRequested => self.draw(),
            _ => {}
        }
    }

    // Ask for another frame as soon as this one is done.
    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
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

    // Poll rather than wait, because there is a frame to run every time round.
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App::new();
    if let Err(error) = event_loop.run_app(&mut app) {
        eprintln!("the event loop stopped: {error}");
    }
}
