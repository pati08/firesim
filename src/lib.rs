use std::{cell::RefCell, rc::Rc};
pub use wasm_bindgen_rayon::init_thread_pool;

use crate::{
    rendering::{RenderMode, RenderSection, RenderState, RenderSurface, RenderSurfaceSize, render},
    sim::{Simulation, SimulationFrame, SimulationParameters, spawn_simulation},
};
use js_sys::Uint8ClampedArray;
use wasm_bindgen::prelude::*;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, ImageBitmap, ImageData, Window};

pub mod rendering;
pub mod sim;
pub mod util;

struct CanvasRenderSurface {
    window: Window,
    canvas_rendering_context: CanvasRenderingContext2d,
    canvas: HtmlCanvasElement,
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

impl RenderSurface for CanvasRenderSurface {
    fn query_size(&self) -> crate::rendering::RenderSurfaceSize {
        RenderSurfaceSize {
            w_px: self.canvas.width() as usize,
            h_px: self.canvas.height() as usize,
        }
    }
    fn present_frame(&mut self, frame: Vec<u32>) {
        let bitmap: Vec<u8> = frame
            .into_iter()
            .flat_map(|i| [(i >> 16) as u8, (i >> 8) as u8, i as u8, u8::MAX].into_iter())
            .collect();
        let bitmap = Uint8ClampedArray::new_from_slice(&bitmap[..]);
        let image_data = ImageData::new_with_js_u8_clamped_array(&bitmap, self.canvas.width())
            .expect("could not construct ImageData");
        let rendering_context = self.canvas_rendering_context.clone();
        let closure = Closure::once(move |data: JsValue| {
            let image_bitmap: ImageBitmap = data
                .dyn_into()
                .expect("failed to cast JsValue into ImageBitmap");
            rendering_context
                .draw_image_with_image_bitmap(&image_bitmap, 0.0, 0.0)
                .expect("failed to draw ");
        });
        let _ = self
            .window
            .create_image_bitmap_with_image_data(&image_data)
            .expect("failed to create image bitmap")
            .then(&closure);
        closure.forget();
    }
}

#[wasm_bindgen(start)]
pub fn initialize() {
    console_error_panic_hook::set_once();
}

// ... (Your structs and imports remain the same)
// Assuming SimulationFrame, SimulationParameters, RenderState, RenderSection,
// RenderMode, CanvasRenderSurface, render, and spawn_simulation are defined.

#[wasm_bindgen]
pub async fn start() -> Simulation {
    let window = web_sys::window().expect("could not get window");
    let canvas: HtmlCanvasElement = window
        .document()
        .expect("could not get document")
        .get_element_by_id("sim-surface")
        .expect("could not get element with id `sim-surface` as required")
        .dyn_into()
        .expect("`sim-surface` is not a canvas");
    let context: CanvasRenderingContext2d = canvas
        .get_context("2d")
        .expect("failed to get context")
        .expect("no 2d context")
        .dyn_into()
        .expect(r#"could not cast the result of calling `canvas.getContext("2d")` into a CanvasRenderingContext2d object"#);
    let mut render_surface = CanvasRenderSurface {
        canvas_rendering_context: context,
        canvas,
        window: window.clone(),
    };

    const SIM_WIDTH: usize = 500;
    const SIM_HEIGHT: usize = 500;

    let start_frame = SimulationFrame::new(SIM_WIDTH, SIM_HEIGHT);
    let sim_params = SimulationParameters::realistic(SIM_WIDTH, SIM_HEIGHT, 10.0, 36.0);
    let sim = spawn_simulation(start_frame, sim_params);

    // --- FIX START ---
    // 1. Change the type to hold the concrete Closure object.
    // The type parameter for Closure is `FnMut` because `requestAnimationFrame`
    // can only take a raw JS function, not a safe Rust closure that implements `FnOnce`.
    type RAFClosure = Closure<dyn FnMut()>;

    let f: Rc<RefCell<Option<RAFClosure>>> = Rc::new(RefCell::new(None));
    let g = f.clone();

    let s2 = sim.clone();

    // 2. Store the Closure object directly.
    *g.borrow_mut() = Some(Closure::new(move || {
        let frame = s2.get_latest_frame();
        let state = RenderState::Singular(RenderSection {
            sim_frame: &frame,
            mode: RenderMode::Standard,
        });
        render(&state, &mut render_surface);

        let window = web_sys::window().expect("failed to get window");

        // 3. To schedule the next frame, borrow the closure from 'f',
        //    get its JsValue reference, and pass it to request_animation_frame.
        window
            .request_animation_frame(f.borrow().as_ref().unwrap().as_ref().unchecked_ref())
            .expect("failed to request animation frame");
    }));

    // 4. Start the loop. Borrow the closure from 'g' and pass its JsValue reference.
    window
        .request_animation_frame(g.borrow().as_ref().unwrap().as_ref().unchecked_ref())
        .expect("failed to request animation frame");

    // --- FIX END ---

    sim
}
