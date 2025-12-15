use std::time::{Duration, Instant};
use wasm_bindgen_rayon::init_thread_pool;

use crate::{
    rendering::{RenderMode, RenderSection, RenderState, RenderSurface, RenderSurfaceSize, render},
    sim::{SimulationFrame, SimulationParameters, spawn_simulation},
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
    width: usize,
    height: usize,
}

impl RenderSurface for CanvasRenderSurface {
    fn query_size(&self) -> crate::rendering::RenderSurfaceSize {
        RenderSurfaceSize {
            w_px: self.width,
            h_px: self.height,
        }
    }
    fn present_frame(&mut self, frame: Vec<u32>) {
        let bitmap: Vec<u8> = frame
            .into_iter()
            .flat_map(|i| [(i >> 16) as u8, (i >> 8) as u8, i as u8].into_iter())
            .collect();
        let bitmap = Uint8ClampedArray::new_from_slice(&bitmap[..]);
        let image_data = ImageData::new_with_js_u8_clamped_array(&bitmap, self.width as u32)
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

#[wasm_bindgen]
pub async fn start() {
    let window = web_sys::window().expect("could not get window");
    let canvas: HtmlCanvasElement = window
        .document()
        .expect("could not get document")
        .get_element_by_id("sim-surface")
        .expect("could not get element with id `sim-surface` as required")
        .dyn_into()
        .expect("`sim-surface` is not a canvas");
    let width = canvas.width() as usize;
    let height = canvas.height() as usize;
    let context: CanvasRenderingContext2d = canvas
        .get_context("2d")
        .expect("failed to get context")
        .expect("no 2d context")
        .dyn_into()
        .expect(r#"could not cast the result of calling `canvas.getContext("2d")` into a CanvasRenderingContext2d object"#);
    let mut render_surface = CanvasRenderSurface {
        canvas_rendering_context: context,
        width,
        height,
        window,
    };

    const SIM_WIDTH: usize = 10;
    const SIM_HEIGHT: usize = 10;

    let start_frame = SimulationFrame::new(SIM_WIDTH, SIM_HEIGHT);
    let sim_params = SimulationParameters::realistic(SIM_WIDTH, SIM_HEIGHT, 10.0, 36.0);
    let sim = spawn_simulation(start_frame, sim_params);

    let mut last_frame_time: Option<Instant> = None;
    loop {
        if let Some(v) = last_frame_time
            && v.elapsed() < Duration::from_millis(16)
        {
            gloo_timers::future::sleep(Duration::from_millis(16) - v.elapsed()).await;
        }
        let latest_frame = sim.get_latest_frame();
        let render_state = RenderState::Singular(RenderSection {
            sim_frame: &latest_frame,
            mode: RenderMode::Standard,
        });
        render(&render_state, &mut render_surface);
        last_frame_time = Some(Instant::now());
    }
}
