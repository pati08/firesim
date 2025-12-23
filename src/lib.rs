#![feature(if_let_guard)]
use std::sync::{Arc, atomic::AtomicBool};

use crate::{
    gpu::GpuSimRenderer,
    sim::{
        ConfigurableParameters, SimulationFrame, SimulationHandle, SimulationParameters,
        SimulationStatistics,
    },
};
use futures_intrusive::channel::shared::OneshotSender;
use wasm_bindgen::prelude::*;
use watch::{WatchReceiver, WatchSender};
#[cfg(target_arch = "wasm32")]
use web_sys::HtmlCanvasElement;
use web_sys::{DedicatedWorkerGlobalScope, Worker, WorkerOptions};
use winit::{
    event::WindowEvent,
    event_loop::{EventLoop, EventLoopProxy},
    window::WindowAttributes,
};

#[cfg(target_arch = "wasm32")]
use winit::platform::web::WindowAttributesExtWebSys;

pub mod gpu;
pub mod rendering;
pub mod sim;
pub mod util;

/// Message type for GPU renderer events
pub enum GpuMessage {
    Initialized(GpuSimRenderer),
    Error(String),
    TogglePause,
    Stop,
    Resume,
    SetParameters(ConfigurableParameters),
}

#[allow(dead_code)]
struct Application {
    simulation: Option<SimulationHandle>, // Reserved for future use
    proxy: Option<EventLoopProxy<GpuMessage>>,
    gpu_renderer: Option<GpuSimRenderer>,
    config_params: ConfigurableParameters,
    paused: bool,
    stopped: bool,
}

impl Application {
    fn new(event_loop: &EventLoop<GpuMessage>) -> Self {
        const SIM_WIDTH: usize = 500;
        const SIM_HEIGHT: usize = 500;

        let sim_params = ConfigurableParameters::realistic(SIM_WIDTH, SIM_HEIGHT, 2.0, 36.0);
        Self {
            simulation: None,
            proxy: Some(event_loop.create_proxy()),
            gpu_renderer: None,
            config_params: sim_params,
            paused: false,
            stopped: false,
        }
    }

    /// Process any pending control messages from JavaScript
    #[cfg(target_arch = "wasm32")]
    fn process_control_messages(&mut self) {
        CONTROL_QUEUE.with(|queue| {
            let messages: Vec<_> = queue.borrow_mut().drain(..).collect();
            for msg in messages {
                match msg {
                    ControlMessage::TogglePause => {
                        self.paused = !self.paused;
                        log::info!(
                            "Simulation {}",
                            if self.paused { "paused" } else { "resumed" }
                        );
                    }
                    ControlMessage::Stop => {
                        self.stopped = true;
                        self.paused = false;
                        log::info!("Simulation stopped");
                    }
                    ControlMessage::Resume => {
                        if self.stopped {
                            self.stopped = false;
                            log::info!("Simulation resumed from stop");
                        }
                    }
                    ControlMessage::SetParameters(params) => {
                        self.config_params = params;
                        log::debug!("Parameters updated");
                    }
                }
            }
        });
    }
}

impl winit::application::ApplicationHandler<GpuMessage> for Application {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.gpu_renderer.is_some() {
            return;
        }

        #[cfg(target_arch = "wasm32")]
        {
            let dom_window = web_sys::window().expect("could not get window");
            let canvas: HtmlCanvasElement = dom_window
                .document()
                .expect("could not get document")
                .get_element_by_id("sim-surface")
                .expect("could not get element with id `sim-surface` as required")
                .dyn_into()
                .expect("`sim-surface` is not a canvas");
            let window_attrs = WindowAttributes::default().with_canvas(Some(canvas));
            match event_loop.create_window(window_attrs) {
                Ok(window) => {
                    if let Some(proxy) = self.proxy.take() {
                        let window = Arc::new(window);
                        let config = self.config_params.clone();
                        let sim_params = SimulationParameters::from(&config);
                        let start_frame =
                            SimulationFrame::new(config.forest_width, config.forest_height);

                        wasm_bindgen_futures::spawn_local(async move {
                            match GpuSimRenderer::new(window, start_frame, sim_params).await {
                                Ok(renderer) => {
                                    let _ = proxy.send_event(GpuMessage::Initialized(renderer));
                                }
                                Err(e) => {
                                    // Error will be logged in user_event handler
                                    let _ = proxy.send_event(GpuMessage::Error(e.to_string()));
                                }
                            }
                        });
                    }
                }
                Err(e) => log::error!("failed to create window: {e}"),
            };
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            match event_loop.create_window(WindowAttributes::default()) {
                Ok(window) => {
                    if let Some(proxy) = self.proxy.take() {
                        let window = Arc::new(window);
                        let config = self.config_params.clone();
                        let sim_params = SimulationParameters::from(&config);
                        let start_frame =
                            SimulationFrame::new(config.forest_width, config.forest_height);

                        // On native, use pollster to block on the future
                        let renderer_result = pollster::block_on(GpuSimRenderer::new(
                            window,
                            start_frame,
                            sim_params,
                        ));
                        match renderer_result {
                            Ok(renderer) => {
                                let _ = proxy.send_event(GpuMessage::Initialized(renderer));
                            }
                            Err(e) => {
                                log::error!("Failed to create GPU renderer: {e}");
                                let _ = proxy.send_event(GpuMessage::Error(e.to_string()));
                            }
                        }
                    }
                }
                Err(e) => log::error!("failed to create window: {e}"),
            };
        }
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested | WindowEvent::Destroyed => {
                self.gpu_renderer = None;
            }
            WindowEvent::Resized(size) => {
                if let Some(ref mut renderer) = self.gpu_renderer {
                    renderer.resize(size.width, size.height);
                }
            }
            WindowEvent::RedrawRequested => {
                // Process any pending control messages from JavaScript
                #[cfg(target_arch = "wasm32")]
                self.process_control_messages();

                if let Some(ref mut renderer) = self.gpu_renderer {
                    let result = if self.stopped {
                        // Stopped: just render current state, don't request more redraws
                        renderer.render()
                    } else if self.paused {
                        // Paused: render current state but keep the animation loop going
                        let r = renderer.render();
                        if r.is_ok() {
                            renderer.request_redraw();
                        }
                        r
                    } else {
                        // Running: step and render
                        let sim_params = SimulationParameters::from(&self.config_params);
                        let r = renderer.step_and_render(sim_params);
                        if r.is_ok() {
                            renderer.request_redraw();
                        }
                        r
                    };

                    match result {
                        Ok(()) => {}
                        Err(wgpu::SurfaceError::Lost) => {
                            // Reconfigure the surface
                            let (w, h) = renderer.dimensions();
                            renderer.resize(w as u32, h as u32);
                            if !self.stopped {
                                renderer.request_redraw();
                            }
                        }
                        Err(wgpu::SurfaceError::OutOfMemory) => {
                            log::error!("Out of memory!");
                            event_loop.exit();
                        }
                        Err(e) => {
                            log::warn!("Surface error: {e:?}");
                            if !self.stopped {
                                renderer.request_redraw();
                            }
                        }
                    }
                }
            }
            _ => (),
        };
    }

    fn user_event(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop, event: GpuMessage) {
        match event {
            GpuMessage::Initialized(renderer) => {
                log::info!("GPU renderer initialized successfully");
                // Request first redraw to kick off the animation loop
                renderer.request_redraw();
                self.gpu_renderer = Some(renderer);
            }
            GpuMessage::Error(e) => {
                log::error!("GPU initialization error: {e}");
            }
            GpuMessage::TogglePause => {
                self.paused = !self.paused;
                log::info!(
                    "Simulation {}",
                    if self.paused { "paused" } else { "resumed" }
                );
            }
            GpuMessage::Stop => {
                self.stopped = true;
                self.paused = false;
                log::info!("Simulation stopped");
            }
            GpuMessage::Resume => {
                if self.stopped {
                    self.stopped = false;
                    // Request a redraw to restart the animation loop
                    if let Some(ref renderer) = self.gpu_renderer {
                        renderer.request_redraw();
                    }
                    log::info!("Simulation resumed from stop");
                }
            }
            GpuMessage::SetParameters(params) => {
                self.config_params = params;
                log::debug!("Parameters updated");
            }
        }
    }
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

#[wasm_bindgen]
pub fn worker_entry(ptr: u32) -> Result<(), JsValue> {
    // Initialize logging for the worker thread
    let _ = fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[Worker {} {}] {}",
                record.level(),
                record.target(),
                message
            ))
        })
        .level(log::LevelFilter::Debug)
        .chain(fern::Output::call(console_log::log))
        .apply();

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
    let _ = fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{} {}] {}",
                record.level(),
                record.target(),
                message
            ))
        })
        .level(log::LevelFilter::Debug)
        .chain(fern::Output::call(console_log::log))
        .apply();
}

/// Control message for the simulation (simple enum without heavy types)
#[cfg(target_arch = "wasm32")]
#[derive(Clone)]
enum ControlMessage {
    TogglePause,
    Stop,
    Resume,
    SetParameters(ConfigurableParameters),
}

// Thread-local storage for control messages (WASM is single-threaded)
#[cfg(target_arch = "wasm32")]
thread_local! {
    static CONTROL_QUEUE: std::cell::RefCell<Vec<ControlMessage>> = std::cell::RefCell::new(Vec::new());
    static PARAMS_STORE: std::cell::RefCell<Option<ConfigurableParameters>> = const { std::cell::RefCell::new(None) };
}

/// Controller for the running simulation
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub struct SimulationController;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl SimulationController {
    /// Toggle pause state
    #[wasm_bindgen]
    pub fn toggle_pause() {
        CONTROL_QUEUE.with(|q| q.borrow_mut().push(ControlMessage::TogglePause));
    }

    /// Stop the simulation
    #[wasm_bindgen]
    pub fn stop() {
        CONTROL_QUEUE.with(|q| q.borrow_mut().push(ControlMessage::Stop));
    }

    /// Resume the simulation (after stop)
    #[wasm_bindgen]
    pub fn resume() {
        CONTROL_QUEUE.with(|q| q.borrow_mut().push(ControlMessage::Resume));
    }

    /// Set lightning frequency (strikes per year per acre)
    #[wasm_bindgen]
    pub fn set_lightning_frequency(value: f32) {
        Self::update_param(|p| p.lightning_strikes_per_year_per_acre = value);
    }

    /// Set fire spread rate (0-1)
    #[wasm_bindgen]
    pub fn set_fire_spread_rate(value: f32) {
        Self::update_param(|p| p.fire_spread_rate = value);
    }

    /// Set tree growth years
    #[wasm_bindgen]
    pub fn set_tree_growth_years(value: f32) {
        Self::update_param(|p| p.tree_growth_years = value);
    }

    /// Set tree death years
    #[wasm_bindgen]
    pub fn set_tree_death_years(value: f32) {
        Self::update_param(|p| p.tree_death_years = value);
    }

    /// Set months per second (simulation speed)
    #[wasm_bindgen]
    pub fn set_months_per_second(value: f32) {
        Self::update_param(|p| p.months_per_second = value);
    }

    /// Set tree flammability
    #[wasm_bindgen]
    pub fn set_tree_flammability(value: f32) {
        Self::update_param(|p| p.tree_flammability = value);
    }

    /// Set underbrush flammability
    #[wasm_bindgen]
    pub fn set_underbrush_flammability(value: f32) {
        Self::update_param(|p| p.underbrush_flammability = value);
    }

    /// Set underbrush tree growth hindrance
    #[wasm_bindgen]
    pub fn set_underbrush_tree_growth_hindrance(value: f32) {
        Self::update_param(|p| p.underbrush_tree_growth_hindrance = value);
    }

    /// Set tree underbrush generation
    #[wasm_bindgen]
    pub fn set_tree_underbrush_generation(value: f32) {
        Self::update_param(|p| p.tree_underbrush_generation = value);
    }

    /// Set tree death underbrush
    #[wasm_bindgen]
    pub fn set_tree_death_underbrush(value: f32) {
        Self::update_param(|p| p.tree_death_underbrush = value);
    }

    /// Set tree fire duration
    #[wasm_bindgen]
    pub fn set_tree_fire_duration(value: u32) {
        Self::update_param(|p| p.tree_fire_duration = value);
    }

    /// Set underbrush fire duration
    #[wasm_bindgen]
    pub fn set_underbrush_fire_duration(value: u32) {
        Self::update_param(|p| p.underbrush_fire_duration = value);
    }

    /// Set ticks per month (simulation resolution)
    #[wasm_bindgen]
    pub fn set_ticks_per_month(value: f32) {
        Self::update_param(|p| p.ticks_per_month = value);
    }

    fn update_param<F: FnOnce(&mut ConfigurableParameters)>(f: F) {
        PARAMS_STORE.with(|store| {
            if let Some(ref mut params) = *store.borrow_mut() {
                f(params);
                let params_clone = params.clone();
                CONTROL_QUEUE.with(|q| {
                    q.borrow_mut()
                        .push(ControlMessage::SetParameters(params_clone));
                });
            }
        });
    }
}

/// Start the fire simulation with GPU rendering
///
/// This function creates a winit event loop and runs the simulation
/// with integrated GPU compute and rendering.
#[wasm_bindgen]
pub fn start() {
    use winit::event_loop::EventLoop;

    log::info!("Starting fire simulation with GPU rendering");

    let event_loop = EventLoop::<GpuMessage>::with_user_event()
        .build()
        .expect("Failed to create event loop");

    #[allow(unused_mut)]
    let mut app = Application::new(&event_loop);

    // Store initial parameters for controller access
    #[cfg(target_arch = "wasm32")]
    {
        PARAMS_STORE.with(|store| {
            *store.borrow_mut() = Some(app.config_params.clone());
        });
    }

    // On web, we need to spawn the event loop
    #[cfg(target_arch = "wasm32")]
    {
        use winit::platform::web::EventLoopExtWebSys;
        event_loop.spawn_app(app);
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        event_loop.run_app(&mut app).expect("Event loop error");
    }
}

/// Standalone GPU simulation and renderer for more control
///
/// Use this when you want to manage the render loop yourself
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub struct GpuSimulation {
    renderer: GpuSimRenderer,
    config_params: ConfigurableParameters,
    paused: bool,
    stopped: bool,
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl GpuSimulation {
    /// Create a new GPU simulation from a canvas element ID
    #[wasm_bindgen]
    pub async fn create(canvas_id: &str) -> Result<GpuSimulation, JsValue> {
        use winit::platform::web::WindowAttributesExtWebSys;

        const SIM_WIDTH: usize = 500;
        const SIM_HEIGHT: usize = 500;

        let config_params = ConfigurableParameters::realistic(SIM_WIDTH, SIM_HEIGHT, 2.0, 36.0);
        let sim_params = SimulationParameters::from(&config_params);
        let start_frame = SimulationFrame::new(SIM_WIDTH, SIM_HEIGHT);

        let dom_window = web_sys::window().ok_or("No window")?;
        let canvas: HtmlCanvasElement = dom_window
            .document()
            .ok_or("No document")?
            .get_element_by_id(canvas_id)
            .ok_or("Canvas not found")?
            .dyn_into()
            .map_err(|_| "Element is not a canvas")?;

        // Create a minimal event loop just to get a window
        let event_loop = winit::event_loop::EventLoop::<()>::new()
            .map_err(|e| format!("Failed to create event loop: {e}"))?;

        #[allow(deprecated)]
        let window = event_loop
            .create_window(WindowAttributes::default().with_canvas(Some(canvas)))
            .map_err(|e| format!("Failed to create window: {e}"))?;

        let window = Arc::new(window);

        let renderer = GpuSimRenderer::new(window, start_frame, sim_params)
            .await
            .map_err(|e| format!("Failed to create renderer: {e}"))?;

        Ok(Self {
            renderer,
            config_params,
            paused: false,
            stopped: false,
        })
    }

    /// Run one simulation step and render the result
    #[wasm_bindgen]
    pub fn step_and_render(&mut self) -> Result<(), JsValue> {
        if self.stopped {
            return self.render();
        }
        if self.paused {
            return self.render();
        }
        let sim_params = SimulationParameters::from(&self.config_params);
        self.renderer
            .step_and_render(sim_params)
            .map_err(|e| JsValue::from_str(&format!("Render error: {e:?}")))
    }

    /// Run one simulation step without rendering
    #[wasm_bindgen]
    pub fn step(&mut self) {
        if self.stopped || self.paused {
            return;
        }
        let sim_params = SimulationParameters::from(&self.config_params);
        self.renderer.compute_step(sim_params);
    }

    /// Render the current state without advancing simulation
    #[wasm_bindgen]
    pub fn render(&self) -> Result<(), JsValue> {
        self.renderer
            .render()
            .map_err(|e| JsValue::from_str(&format!("Render error: {e:?}")))
    }

    /// Resize the render surface
    #[wasm_bindgen]
    pub fn resize(&mut self, width: u32, height: u32) {
        self.renderer.resize(width, height);
    }

    /// Get current simulation step count
    #[wasm_bindgen]
    pub fn steps(&self) -> u32 {
        self.renderer.steps()
    }

    /// Check if simulation is paused
    #[wasm_bindgen]
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Check if simulation is stopped
    #[wasm_bindgen]
    pub fn is_stopped(&self) -> bool {
        self.stopped
    }

    /// Toggle pause state
    #[wasm_bindgen]
    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
    }

    /// Pause the simulation
    #[wasm_bindgen]
    pub fn pause(&mut self) {
        self.paused = true;
    }

    /// Resume the simulation (unpause)
    #[wasm_bindgen]
    pub fn unpause(&mut self) {
        self.paused = false;
    }

    /// Stop the simulation permanently
    #[wasm_bindgen]
    pub fn stop(&mut self) {
        self.stopped = true;
        self.paused = false;
    }

    /// Set lightning frequency
    #[wasm_bindgen]
    pub fn set_lightning_frequency(&mut self, strikes_per_year_per_acre: f32) {
        self.config_params.lightning_strikes_per_year_per_acre = strikes_per_year_per_acre;
    }

    /// Get lightning frequency
    #[wasm_bindgen]
    pub fn get_lightning_frequency(&self) -> f32 {
        self.config_params.lightning_strikes_per_year_per_acre
    }

    /// Set fire spread rate
    #[wasm_bindgen]
    pub fn set_fire_spread_rate(&mut self, rate: f32) {
        self.config_params.fire_spread_rate = rate;
    }

    /// Get fire spread rate
    #[wasm_bindgen]
    pub fn get_fire_spread_rate(&self) -> f32 {
        self.config_params.fire_spread_rate
    }

    /// Set tree growth rate in years
    #[wasm_bindgen]
    pub fn set_tree_growth_years(&mut self, years: f32) {
        self.config_params.tree_growth_years = years;
    }

    /// Get tree growth years
    #[wasm_bindgen]
    pub fn get_tree_growth_years(&self) -> f32 {
        self.config_params.tree_growth_years
    }

    /// Set tree death years
    #[wasm_bindgen]
    pub fn set_tree_death_years(&mut self, years: f32) {
        self.config_params.tree_death_years = years;
    }

    /// Get tree death years
    #[wasm_bindgen]
    pub fn get_tree_death_years(&self) -> f32 {
        self.config_params.tree_death_years
    }

    /// Set simulation speed (months per second)
    #[wasm_bindgen]
    pub fn set_months_per_second(&mut self, months: f32) {
        self.config_params.months_per_second = months;
    }

    /// Get months per second
    #[wasm_bindgen]
    pub fn get_months_per_second(&self) -> f32 {
        self.config_params.months_per_second
    }

    /// Set ticks per month (simulation resolution)
    #[wasm_bindgen]
    pub fn set_ticks_per_month(&mut self, value: f32) {
        self.config_params.ticks_per_month = value;
    }

    /// Get ticks per month
    #[wasm_bindgen]
    pub fn get_ticks_per_month(&self) -> f32 {
        self.config_params.ticks_per_month
    }

    /// Set tree flammability
    #[wasm_bindgen]
    pub fn set_tree_flammability(&mut self, value: f32) {
        self.config_params.tree_flammability = value;
    }

    /// Get tree flammability
    #[wasm_bindgen]
    pub fn get_tree_flammability(&self) -> f32 {
        self.config_params.tree_flammability
    }

    /// Set underbrush flammability
    #[wasm_bindgen]
    pub fn set_underbrush_flammability(&mut self, value: f32) {
        self.config_params.underbrush_flammability = value;
    }

    /// Get underbrush flammability
    #[wasm_bindgen]
    pub fn get_underbrush_flammability(&self) -> f32 {
        self.config_params.underbrush_flammability
    }

    /// Set underbrush tree growth hindrance
    #[wasm_bindgen]
    pub fn set_underbrush_tree_growth_hindrance(&mut self, value: f32) {
        self.config_params.underbrush_tree_growth_hindrance = value;
    }

    /// Get underbrush tree growth hindrance
    #[wasm_bindgen]
    pub fn get_underbrush_tree_growth_hindrance(&self) -> f32 {
        self.config_params.underbrush_tree_growth_hindrance
    }

    /// Set tree underbrush generation
    #[wasm_bindgen]
    pub fn set_tree_underbrush_generation(&mut self, value: f32) {
        self.config_params.tree_underbrush_generation = value;
    }

    /// Get tree underbrush generation
    #[wasm_bindgen]
    pub fn get_tree_underbrush_generation(&self) -> f32 {
        self.config_params.tree_underbrush_generation
    }

    /// Set tree death underbrush
    #[wasm_bindgen]
    pub fn set_tree_death_underbrush(&mut self, value: f32) {
        self.config_params.tree_death_underbrush = value;
    }

    /// Get tree death underbrush
    #[wasm_bindgen]
    pub fn get_tree_death_underbrush(&self) -> f32 {
        self.config_params.tree_death_underbrush
    }

    /// Set tree fire duration in ticks
    #[wasm_bindgen]
    pub fn set_tree_fire_duration(&mut self, value: u32) {
        self.config_params.tree_fire_duration = value;
    }

    /// Get tree fire duration
    #[wasm_bindgen]
    pub fn get_tree_fire_duration(&self) -> u32 {
        self.config_params.tree_fire_duration
    }

    /// Set underbrush fire duration in ticks
    #[wasm_bindgen]
    pub fn set_underbrush_fire_duration(&mut self, value: u32) {
        self.config_params.underbrush_fire_duration = value;
    }

    /// Get underbrush fire duration
    #[wasm_bindgen]
    pub fn get_underbrush_fire_duration(&self) -> u32 {
        self.config_params.underbrush_fire_duration
    }

    /// Get forest width
    #[wasm_bindgen]
    pub fn get_forest_width(&self) -> usize {
        self.config_params.forest_width
    }

    /// Get forest height
    #[wasm_bindgen]
    pub fn get_forest_height(&self) -> usize {
        self.config_params.forest_height
    }

    /// Get forest acres
    #[wasm_bindgen]
    pub fn get_forest_acres(&self) -> f32 {
        self.config_params.forest_acres
    }
}
