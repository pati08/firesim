use bytemuck::{Pod, Zeroable};
use futures_intrusive::channel::shared::{OneshotReceiver, OneshotSender};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use wasm_bindgen::prelude::*;
use watch::{WatchReceiver, WatchSender};

pub mod gpucompute;

use js_sys::Date;

use crate::spawn_sim_worker;

#[derive(Clone)]
pub struct SimulationFrame {
    pub width: usize,
    pub height: usize,
    pub grid: Arc<[CellState]>,
}

impl SimulationFrame {
    pub fn new(width: usize, height: usize) -> SimulationFrame {
        SimulationFrame {
            width,
            height,
            grid: vec![
                CellState {
                    burning: BurnState::NotBurning,
                    tree: false,
                    underbrush: 0.0
                };
                width * height
            ]
            .into(),
        }
    }
}

impl Default for SimulationFrame {
    fn default() -> Self {
        Self::new(100, 100)
    }
}

#[derive(Clone)]
pub struct CellState {
    pub burning: BurnState,
    pub underbrush: f32,
    pub tree: bool,
}

#[derive(Clone)]
pub enum BurnState {
    NotBurning,
    Burning { ticks_remaining: u32 },
}

/// Configuration parameters with realistic units and static forest properties.
/// These parameters are grounded in reality and are used to compute the internal
/// SimulationParameters struct.
#[derive(Clone)]
#[wasm_bindgen]
pub struct ConfigurableParameters {
    // Static parameters - forest size
    /// Width of the forest in cells
    pub forest_width: usize,
    /// Height of the forest in cells
    pub forest_height: usize,
    /// Size of the forest in acres (computed from width and height)
    pub forest_acres: f32,

    // Time scale parameters
    /// Number of simulation ticks per month
    pub ticks_per_month: f32,
    /// Number of months that pass per second of real time
    pub months_per_second: f32,

    // Realistic configurable parameters
    /// Lightning strike frequency in strikes per year per acre
    pub lightning_strikes_per_year_per_acre: f32,
    /// Tree growth rate: average years for a tree to grow (e.g., 150.0 means 1/150 per year)
    pub tree_growth_years: f32,
    /// Tree death rate: average years for a tree to die naturally (e.g., 200.0 means 1/200 per year)
    pub tree_death_years: f32,
    /// The factor by which the tree growth rate is reduced with underbrush.
    /// The final growth rate is calculated as
    /// ```
    /// let final_growth_rate = (1.0 - underbrush_tree_growth_hindrance * underbrush) *
    /// tree_growth_rate;
    /// ```
    pub underbrush_tree_growth_hindrance: f32,
    /// The base rate of underbrush accumulation per tick
    pub tree_underbrush_generation: f32,
    /// The amount of underbrush created when a tree dies naturally
    pub tree_death_underbrush: f32,
    /// The length a single tree can support a fire for in ticks
    pub tree_fire_duration: u32,
    /// The length that underbrush can support a fire for in ticks. This is
    /// multiplied by the amount of underbrush
    pub underbrush_fire_duration: u32,
    /// The base chance (0 - 1) that fire spreads from a particular cell to a
    /// particular neighbor cell
    pub fire_spread_rate: f32,
    /// The multiplier for fire spread rate for trees
    pub tree_flammability: f32,
    /// The multiplier for fire spread rate for underbrush (multiplied by the
    /// amount of underbrush). This is added with the value from tree_flammability
    /// to calculate the final chance
    pub underbrush_flammability: f32,
}

impl ConfigurableParameters {
    /// Create realistic default parameters for a forest of the given size
    pub fn realistic(
        width: usize,
        height: usize,
        ticks_per_month: f32,
        months_per_second: f32,
    ) -> ConfigurableParameters {
        let forest_acres = (width as f32 * height as f32) / 4047.0;
        Self {
            forest_width: width,
            forest_height: height,
            forest_acres,
            ticks_per_month,
            months_per_second,
            lightning_strikes_per_year_per_acre: 1.0 / 45.0, // ~1 strike per 45 acres per year
            tree_growth_years: 150.0,
            tree_death_years: 200.0,
            underbrush_tree_growth_hindrance: 0.0,
            tree_underbrush_generation: 0.0001,
            tree_death_underbrush: 0.01,
            tree_fire_duration: 1,
            underbrush_fire_duration: 1,
            fire_spread_rate: 1.0,
            tree_flammability: 0.5,
            underbrush_flammability: 1.0,
        }
    }
}

/// Internal computed parameters derived from ConfigurableParameters.
/// This struct contains per-tick values computed from the realistic units
/// in ConfigurableParameters.
#[derive(Clone, Copy, Pod, Zeroable, PartialEq)]
#[repr(C)]
pub struct SimulationParameters {
    /// The base chance (0 - 1) that a tree will grow in a given cell each tick
    pub tree_growth_rate: f32,
    /// The factor by which the tree growth rate is reduced with underbrush.
    pub underbrush_tree_growth_hindrance: f32,
    /// The base rate of underbrush accumulation
    pub tree_underbrush_generation: f32,
    /// The amount of underbrush created when a tree dies naturally
    pub tree_death_underbrush: f32,
    /// The chance (0 - 1) that a particular tree dies naturally each tick
    pub tree_death_rate: f32,
    /// The length a single tree can support a fire for in ticks
    pub tree_fire_duration: u32,
    /// The length that underbrush can support a fire for in ticks. This is
    /// multiplied by the amount of underbrush
    pub underbrush_fire_duration: u32,
    /// The base chance (0 - 1) that fire spreads from a particular cell to a
    /// particular neighbor cell
    pub fire_spread_rate: f32,
    /// The multiplier for fire spread rate for trees
    pub tree_flammability: f32,
    /// The multiplier for fire spread rate for underbrush (multiplied by the
    /// amount of underbrush). This is added with the value from tree_flammability
    /// to calculate the final chance
    pub underbrush_flammability: f32,
    /// The chance (0 - 1) of a lightning strike each tick, globally
    pub lightning_frequency: f32,
    /// The tick rate in ticks per second
    pub tick_rate: u32,
}

impl From<&ConfigurableParameters> for SimulationParameters {
    fn from(config: &ConfigurableParameters) -> Self {
        let tick_rate = (config.ticks_per_month * config.months_per_second).round() as u32;
        let ticks_per_year = config.ticks_per_month * 12.0;

        // Convert lightning strikes per year per acre to per-tick probability
        let lightning_frequency =
            config.lightning_strikes_per_year_per_acre * config.forest_acres / ticks_per_year;

        // Convert tree growth/death rates from years to per-tick probabilities
        let tree_growth_rate = 1.0 / (ticks_per_year * config.tree_growth_years);
        let tree_death_rate = 1.0 / (ticks_per_year * config.tree_death_years);

        Self {
            tick_rate,
            lightning_frequency,
            tree_growth_rate,
            tree_death_rate,
            underbrush_tree_growth_hindrance: config.underbrush_tree_growth_hindrance,
            tree_underbrush_generation: config.tree_underbrush_generation,
            tree_death_underbrush: config.tree_death_underbrush,
            tree_fire_duration: config.tree_fire_duration,
            underbrush_fire_duration: config.underbrush_fire_duration,
            fire_spread_rate: config.fire_spread_rate,
            tree_flammability: config.tree_flammability,
            underbrush_flammability: config.underbrush_flammability,
        }
    }
}

#[non_exhaustive]
#[derive(Default, Debug)]
#[wasm_bindgen(getter_with_clone)]
pub struct SimulationStatistics {
    pub average_step_exec_time: f64,
}

#[wasm_bindgen]
#[derive(Clone)]
pub struct SimulationHandle {
    parameters_tx: WatchSender<ConfigurableParameters>,
    parameters_rx: WatchReceiver<ConfigurableParameters>,
    stop: Arc<AtomicBool>,
    latest_frame_rx: WatchReceiver<SimulationFrame>,
    stats_rx: Arc<Mutex<OneshotReceiver<SimulationStatistics>>>,
    wants_new_frame: Arc<AtomicBool>,
}

pub fn spawn_simulation(
    start_frame: SimulationFrame,
    parameters: ConfigurableParameters,
) -> SimulationHandle {
    let (parameters_tx, parameters_rx) = watch::channel(parameters);
    let stop = Arc::new(AtomicBool::new(false));
    let wants_new_frame = Arc::new(AtomicBool::new(false));
    let (latest_frame_tx, latest_frame_rx) = watch::channel(start_frame.clone());
    let s = Arc::clone(&stop);
    let wnf = Arc::clone(&wants_new_frame);
    let p = parameters_rx.clone();
    let (stats_tx, stats_rx) = futures_intrusive::channel::shared::oneshot_channel();
    let lf_rx = latest_frame_rx.clone();
    spawn_sim_worker(crate::SimWorkerArgs {
        parameters_rx: p,
        stop: s,
        latest_frame_tx,
        latest_frame_rx: lf_rx,
        stats_tx,
        wants_new_frame: wnf,
    })
    .unwrap();
    let stats_rx = Arc::new(Mutex::new(stats_rx));
    SimulationHandle {
        parameters_tx,
        parameters_rx,
        stop,
        latest_frame_rx,
        stats_rx,
        wants_new_frame,
    }
}

impl SimulationHandle {
    /// Get the latest completed simulation frame.
    pub fn get_latest_frame(&mut self) -> SimulationFrame {
        self.wants_new_frame.store(true, Ordering::Relaxed);
        self.latest_frame_rx.get()
    }
}

#[wasm_bindgen]
impl SimulationHandle {
    #[wasm_bindgen]
    pub async fn stop(self) -> Option<SimulationStatistics> {
        self.stop.store(true, Ordering::Relaxed);
        log::info!("stopping");
        self.stats_rx
            .lock()
            .expect("failed to get stats rx lock")
            .receive()
            .await
    }
    #[wasm_bindgen]
    pub fn set_parameters(&mut self, new_params: ConfigurableParameters) {
        self.parameters_tx.send(new_params);
    }
    #[wasm_bindgen]
    pub fn get_parameters(&mut self) -> ConfigurableParameters {
        self.parameters_rx.get()
    }
}

pub async fn sim_thread(
    mut parameters_rx: WatchReceiver<ConfigurableParameters>,
    stop: Arc<AtomicBool>,
    latest_frame_tx: WatchSender<SimulationFrame>,
    mut latest_frame_rx: WatchReceiver<SimulationFrame>,
    stats_tx: OneshotSender<SimulationStatistics>,
    wants_new_frame: Arc<AtomicBool>,
) {
    let (device, queue) = gpucompute::create_device().await.unwrap();
    let mut end_of_last_step = Date::now();
    let mut total_iterations = 0;
    let mut total_time = 0.0;
    let mut context = gpucompute::ComputeContext::create(
        device,
        queue,
        latest_frame_rx.get(),
        SimulationParameters::from(&parameters_rx.get()),
        latest_frame_tx,
    )
    .unwrap();
    while !stop.load(Ordering::Relaxed) {
        let config_params = parameters_rx.get();
        let parameters = SimulationParameters::from(&config_params);
        context.compute_step(parameters);
        total_time += Date::now() - end_of_last_step;
        if wants_new_frame.load(Ordering::Relaxed) {
            context.send_latest();
        }
        total_iterations += 1;
        if parameters.tick_rate == 0 {
            parameters_rx.wait();
            end_of_last_step = Date::now();
            continue;
        }
        let to_wait =
            (parameters.tick_rate as f64).recip() * 1000.0 - (Date::now() - end_of_last_step);
        if to_wait > 0.0 {
            gloo_timers::future::TimeoutFuture::new(to_wait as u32).await;
        }
        end_of_last_step = Date::now();
    }
    let stats = SimulationStatistics {
        average_step_exec_time: total_time / total_iterations as f64,
    };
    stats_tx.send(stats).unwrap();
}
