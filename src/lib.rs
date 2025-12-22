#![feature(if_let_guard)]
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, atomic::AtomicBool},
    time::SystemTime,
};

use crate::{
    rendering::{RenderMode, RenderSection, RenderState, RenderSurface, RenderSurfaceSize, render},
    sim::{
        ConfigurableParameters, SimulationFrame, SimulationHandle, SimulationStatistics,
        spawn_simulation,
    },
};
use futures_intrusive::channel::shared::OneshotSender;
use wasm_bindgen::prelude::*;
use watch::{WatchReceiver, WatchSender};
use web_sys::{
    CanvasRenderingContext2d, DedicatedWorkerGlobalScope, HtmlCanvasElement, ImageBitmap,
    ImageData, Window, Worker, WorkerOptions,
};
use winit::{
    event::WindowEvent,
    event_loop::{EventLoop, EventLoopProxy},
    platform::web::WindowAttributesExtWebSys,
    window::WindowAttributes,
};

pub mod rendering;
pub mod sim;
pub mod util;

struct Application {
    simulation: SimulationHandle,
    proxy: Option<EventLoopProxy<RenderState>>,
    render_state: Option<RenderState>,
}

impl Application {
    fn new(event_loop: &EventLoop<RenderState>) -> Self {
        const SIM_WIDTH: usize = 500;
        const SIM_HEIGHT: usize = 500;

        let start_frame = SimulationFrame::new(SIM_WIDTH, SIM_HEIGHT);
        let sim_params = ConfigurableParameters::realistic(SIM_WIDTH, SIM_HEIGHT, 2.0, 36.0);
        let sim = spawn_simulation(start_frame, sim_params);
        Self {
            simulation: sim,
            proxy: Some(event_loop.create_proxy()),
            render_state: None,
        }
    }
}

impl winit::application::ApplicationHandler<RenderState> for Application {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.render_state.is_some() {
            return;
        }

        let dom_window = web_sys::window().expect("could not get window");
        let canvas: HtmlCanvasElement = dom_window
            .document()
            .expect("could not get document")
            .get_element_by_id("sim-surface")
            .expect("could not get element with id `sim-surface` as required")
            .dyn_into()
            .expect("`sim-surface` is not a canvas");
        match event_loop.create_window(WindowAttributes::default().with_canvas(Some(canvas))) {
            Ok(window) => {
                if let Some(proxy) = self.proxy.take() {
                    wasm_bindgen_futures::spawn_local(async move {
                        let state = RenderState::new(Arc::new(window)).await;
                        assert!(proxy.send_event(state).is_ok());
                    });
                    // Some(RenderState::new(Arc::new(window)))
                }
            }
            Err(e) => log::error!("failed to create window: {e}"),
        };
    }
    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested | WindowEvent::Destroyed => self.render_state = None,
            WindowEvent::RedrawRequested if let Some(ref mut state) = self.render_state => {
                state.redraw()
            }
            _ => (),
        };
    }
    fn user_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        mut event: RenderState,
    ) {
    }
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

#[wasm_bindgen]
pub fn worker_entry(ptr: u32) -> Result<(), JsValue> {
    let ptr = unsafe { Box::from_raw(ptr as *mut SimWorkerArgs) };
    let global = js_sys::global().unchecked_into::<DedicatedWorkerGlobalScope>();
    wasm_bindgen_futures::spawn_local(async move {
        sim::sim_thread(
            ptr.parameters_rx,
            ptr.stop,
            ptr.latest_frame_tx,
            ptr.latest_frame_rx,
            ptr.stats_tx,
            ptr.wants_new_frame,
        )
        .await;
    });
    global.post_message(&JsValue::undefined())?;
    Ok(())
}

fn spawn_sim_worker(args: SimWorkerArgs) -> Result<(), JsValue> {
    let opts = WorkerOptions::new();
    opts.set_type(web_sys::WorkerType::Module);
    let worker = Worker::new_with_options("worker.js", &opts)?;
    let array = js_sys::Array::new();
    array.push(&wasm_bindgen::module());
    array.push(&wasm_bindgen::memory());
    worker.post_message(&array)?;
    let work = Box::new(args);
    let ptr = Box::into_raw(work);
    worker.post_message(&JsValue::from(ptr as u32))?;
    Ok(())
}
struct SimWorkerArgs {
    parameters_rx: WatchReceiver<ConfigurableParameters>,
    stop: Arc<AtomicBool>,
    latest_frame_tx: WatchSender<SimulationFrame>,
    latest_frame_rx: WatchReceiver<SimulationFrame>,
    stats_tx: OneshotSender<SimulationStatistics>,
    wants_new_frame: Arc<AtomicBool>,
}

#[wasm_bindgen(start)]
pub fn initialize() {
    console_error_panic_hook::set_once();
}

// ... (Your structs and imports remain the same)
// Assuming SimulationFrame, ConfigurableParameters, RenderState, RenderSection,
// RenderMode, CanvasRenderSurface, render, and spawn_simulation are defined.

// #[wasm_bindgen]
// pub fn start() -> SimulationHandle {
//     let _ = fern::Dispatch::new()
//         .format(|out, message, record| {
//             out.finish(format_args!(
//                 "[{} {} {} {}",
//                 humantime::format_rfc3339_seconds(
//                     SystemTime::UNIX_EPOCH + std::time::Duration::from_millis(Date::now() as u64)
//                 ),
//                 record.level(),
//                 record.target(),
//                 message
//             ))
//         })
//         .level(log::LevelFilter::Debug)
//         .chain(fern::Output::call(console_log::log))
//         .apply()
//         .inspect_err(|e| crate::log(&format!("failed to initialize logger: {e}")));
//     let window = web_sys::window().expect("could not get window");
//     let canvas: HtmlCanvasElement = window
//         .document()
//         .expect("could not get document")
//         .get_element_by_id("sim-surface")
//         .expect("could not get element with id `sim-surface` as required")
//         .dyn_into()
//         .expect("`sim-surface` is not a canvas");
//     let context: CanvasRenderingContext2d = canvas
//         .get_context("2d")
//         .expect("failed to get context")
//         .expect("no 2d context")
//         .dyn_into()
//         .expect(r#"could not cast the result of calling `canvas.getContext("2d")` into a CanvasRenderingContext2d object"#);
//     let mut render_surface = CanvasRenderSurface {
//         canvas_rendering_context: context,
//         canvas,
//         window: window.clone(),
//     };
//
//     const SIM_WIDTH: usize = 500;
//     const SIM_HEIGHT: usize = 500;
//
//     let start_frame = SimulationFrame::new(SIM_WIDTH, SIM_HEIGHT);
//     let sim_params = ConfigurableParameters::realistic(SIM_WIDTH, SIM_HEIGHT, 2.0, 36.0);
//     let sim = spawn_simulation(start_frame, sim_params);
//
//     type RAFClosure = Closure<dyn FnMut()>;
//
//     let f: Rc<RefCell<Option<RAFClosure>>> = Rc::new(RefCell::new(None));
//     let g = f.clone();
//
//     let mut s2 = sim.clone();
//
//     // 2. Store the Closure object directly.
//     *g.borrow_mut() = Some(Closure::new(move || {
//         let start = Date::now();
//         let frame = s2.get_latest_frame();
//         let state = RenderState::Singular(RenderSection {
//             sim_frame: &frame,
//             mode: RenderMode::Standard,
//         });
//         render(&state, &mut render_surface);
//         log::info!("rendering took ~{}", Date::now() - start);
//
//         let window = web_sys::window().expect("failed to get window");
//
//         // 3. To schedule the next frame, borrow the closure from 'f',
//         //    get its JsValue reference, and pass it to request_animation_frame.
//         window
//             .request_animation_frame(f.borrow().as_ref().unwrap().as_ref().unchecked_ref())
//             .expect("failed to request animation frame");
//     }));
//
//     // 4. Start the loop. Borrow the closure from 'g' and pass its JsValue reference.
//     window
//         .request_animation_frame(g.borrow().as_ref().unwrap().as_ref().unchecked_ref())
//         .expect("failed to request animation frame");
//
//     // --- FIX END ---
//
//     sim
// }
