//! The browser host: draws the console onto a canvas.
//!
//! Only says how to draw and how to tell the time; the frontend does the rest.

use std::cell::RefCell;
use std::rc::Rc;

use rustboy_core::{Button, FRAMEBUFFER_LEN, SCREEN_HEIGHT, SCREEN_WIDTH};
use rustboy_frontend::{Frontend, Host};
use wasm_bindgen::prelude::*;
use wasm_bindgen::{Clamped, JsCast};
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, ImageData, KeyboardEvent};

// The animation callback hands itself back to the browser, so it holds its own handle.
type FrameLoop = Rc<RefCell<Option<Closure<dyn FnMut()>>>>;
// The frame loop and the key handlers all reach the same frontend.
type Shared = Rc<RefCell<Frontend>>;

/// Everything the shared frontend needs from a browser tab.
struct Browser {
    context: CanvasRenderingContext2d,
    pixels: Vec<u8>, // painted here, then handed to the canvas in one go
    started: f64,    // milliseconds, from the browser's own clock
}

impl Browser {
    fn new(context: CanvasRenderingContext2d) -> Self {
        Self {
            context,
            pixels: vec![0; FRAMEBUFFER_LEN],
            started: now_ms(),
        }
    }
}

impl Host for Browser {
    fn elapsed(&self) -> f32 {
        ((now_ms() - self.started) / 1000.0) as f32
    }

    fn frame(&mut self) -> &mut [u8] {
        &mut self.pixels
    }

    fn present(&mut self) {
        let slice = Clamped(self.pixels.as_slice());
        let width = SCREEN_WIDTH as u32;
        let image =
            match ImageData::new_with_u8_clamped_array_and_sh(slice, width, SCREEN_HEIGHT as u32) {
                Ok(image) => image,
                Err(_) => return, // nothing sensible to do in a tab
            };
        let _ = self.context.put_image_data(&image, 0.0, 0.0);
    }
}

/// Milliseconds since the page loaded.
fn now_ms() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map_or(0.0, |p| p.now())
}

/// Find the canvas the page set aside and get a 2D context for it.
fn canvas_context(canvas_id: &str) -> Result<CanvasRenderingContext2d, JsValue> {
    let document = web_sys::window()
        .and_then(|w| w.document())
        .ok_or_else(|| JsValue::from_str("no document"))?;
    let canvas: HtmlCanvasElement = document
        .get_element_by_id(canvas_id)
        .ok_or_else(|| JsValue::from_str("no canvas with that id"))?
        .dyn_into()?;

    // The canvas holds one console pixel each; CSS stretches it up.
    canvas.set_width(SCREEN_WIDTH as u32);
    canvas.set_height(SCREEN_HEIGHT as u32);

    canvas
        .get_context("2d")?
        .ok_or_else(|| JsValue::from_str("no 2d context"))?
        .dyn_into()
        .map_err(Into::into)
}

// Pass key presses and releases on to the frontend.
fn listen(name: &str, frontend: &Shared, pressed: bool) -> Result<(), JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let frontend = Rc::clone(frontend);

    let handler = Closure::<dyn FnMut(KeyboardEvent)>::new(move |event: KeyboardEvent| {
        let key = event.key();
        if let Some(button) = button_for(&key) {
            event.prevent_default(); // arrows would otherwise scroll the page
            let mut frontend = frontend.borrow_mut();
            frontend.set_button(button, pressed);
            if pressed {
                frontend.skip_splash();
            }
        }
    });

    window.add_event_listener_with_callback(name, handler.as_ref().unchecked_ref())?;
    handler.forget(); // the page owns it now, for as long as the tab is open
    Ok(())
}

// Which key works which button. Anything else is ignored.
fn button_for(key: &str) -> Option<Button> {
    match key {
        "ArrowRight" => Some(Button::Right),
        "ArrowLeft" => Some(Button::Left),
        "ArrowUp" => Some(Button::Up),
        "ArrowDown" => Some(Button::Down),
        "Enter" => Some(Button::Start),
        "Shift" => Some(Button::Select),
        "a" | "A" => Some(Button::A),
        "x" | "X" => Some(Button::B),
        _ => None,
    }
}

/// Ask the browser to call us back before the next repaint.
fn request_frame(callback: &Closure<dyn FnMut()>) {
    if let Some(window) = web_sys::window() {
        let _ = window.request_animation_frame(callback.as_ref().unchecked_ref());
    }
}

/// Start the console on the canvas with the given id. Called from JavaScript.
#[wasm_bindgen]
pub fn start(canvas_id: &str) -> Result<(), JsValue> {
    console_error_panic_hook::set_once();

    let mut browser = Browser::new(canvas_context(canvas_id)?);
    let frontend: Shared = Rc::new(RefCell::new(Frontend::new()));

    listen("keydown", &frontend, true)?;
    listen("keyup", &frontend, false)?;

    let next: FrameLoop = Rc::new(RefCell::new(None));
    let me = Rc::clone(&next);
    *next.borrow_mut() = Some(Closure::new(move || {
        frontend.borrow_mut().tick(&mut browser);
        if let Some(callback) = me.borrow().as_ref() {
            request_frame(callback);
        }
    }));

    if let Some(callback) = next.borrow().as_ref() {
        request_frame(callback);
    }
    Ok(())
}
