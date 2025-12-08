use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use convolve2d::DynamicMatrix;
use rayon::iter::{IntoParallelIterator, ParallelIterator};

#[derive(Clone)]
pub struct SimulationFrame {
    pub width: usize,
    pub height: usize,
    pub grid: Vec<CellState>,
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
            ],
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

impl BurnState {
    fn burning(&self) -> bool {
        matches!(self, &BurnState::Burning { .. })
    }
}

/// The parameters controlling the simulation
pub struct SimulationParameters {
    /// The base chance (0 - 1) that a tree will grow in a given cell each tick
    pub tree_growth_rate: f32,
    /// The factor by which the tree growth rate is reduced with underbrush.
    /// The final growth rate is calculated as
    /// ```
    /// let final_growth_rate = (1.0 - underbrush_tree_growth_hindrance * underbrush) *
    /// tree_growth_rate;
    /// ```
    pub underbrush_tree_growth_hindrance: f32,
    /// The base rate of underbrush accumulation
    pub underbrush_base_accumulation: f32,
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

#[non_exhaustive]
#[derive(Default)]
pub struct SimulationStatistics {
    pub average_step_exec_time: Duration,
    pub segments: Vec<(&'static str, Duration)>,
}

pub struct Simulation {
    join_handle: JoinHandle<SimulationStatistics>,
    parameters: Arc<Mutex<SimulationParameters>>,
    stop: Arc<AtomicBool>,
    latest_frame: Arc<Mutex<SimulationFrame>>,
}

impl Simulation {
    pub fn spawn(start_frame: SimulationFrame, parameters: SimulationParameters) -> Self {
        let parameters = Arc::new(Mutex::new(parameters));
        let stop = Arc::new(AtomicBool::new(false));
        let latest_frame = Arc::new(Mutex::new(start_frame));
        let p = Arc::clone(&parameters);
        let s = Arc::clone(&stop);
        let l = Arc::clone(&latest_frame);
        Self {
            join_handle: std::thread::spawn(|| sim_thread(p, s, l)),
            parameters,
            stop,
            latest_frame,
        }
    }
    /// Get the latest completed simulation frame. This function blocks if a
    /// simulation step is in progress
    pub fn get_latest_frame(&self) -> SimulationFrame {
        self.latest_frame
            .lock()
            .expect("a thread accessing the latest simulation frame panicked")
            .clone()
    }
    pub fn stop(self) -> SimulationStatistics {
        self.stop.store(true, Ordering::Relaxed);
        self.join_handle.join().unwrap()
    }
}

macro_rules! segment_bench_while {
    (while ($cond:expr) { $({$name:literal : $($contents:stmt)*}),+ $(,)? }) => {{
        let mut segments = vec![$(($name, std::time::Duration::new(0, 0))),+];
        let mut cur_segments = Vec::with_capacity(segments.len());
        let mut iter_count = 0;

        while $cond {
            cur_segments.clear();

            $(
                let segment_start = std::time::Instant::now();
                $($contents)*     // all statements, same scope
                cur_segments.push(segment_start.elapsed());
            )+

            for (idx, item) in cur_segments.iter().enumerate() {
                segments[idx].1 += *item;
            }
            iter_count += 1;
        }

        for s in segments.iter_mut() {
            s.1 /= iter_count;
        }
        segments
    }}
}

fn sim_thread(
    parameters: Arc<Mutex<SimulationParameters>>,
    stop: Arc<AtomicBool>,
    latest_frame: Arc<Mutex<SimulationFrame>>,
) -> SimulationStatistics {
    let (width, height) = {
        let f = latest_frame
            .lock()
            .expect("a thread accessing the simulation parameters panicked");
        (f.width, f.height)
    };
    #[rustfmt::skip]
    let neighbors_kernel = DynamicMatrix::new(
        3,
        3,
        vec![
            0.125, 0.125, 0.125,
            0.125, 0.0,   0.125,
            0.125, 0.125, 0.125
        ],
    )
    .unwrap();
    let mut end_of_last_step: Instant = Instant::now();
    let mut total_iterations = 0;
    let mut total_time = Duration::new(0, 0);
    #[allow(redundant_semicolons)]
    let segments = segment_bench_while!(
    while (!stop.load(Ordering::Relaxed)) {
        {
            "lock values":
            let parameters = parameters
                .lock()
                .expect("a thread accessing the simulation parameters panicked");
            let mut latest_frame = latest_frame
                .lock()
                .expect("a thread accessing the latest simulation frame data panicked");
        },
        {
            "build burning_neighbors":
            let burning_neighbors: Vec<_> = (0..width * height)
                .into_par_iter()
                .map(|i| get_burning_neighbors_of_cell(&latest_frame.grid, i, width, height))
                .collect();
        },
        {
            "build cell_data":
            let cell_data: Vec<_> = latest_frame
                .grid
                .iter_mut()
                .zip(burning_neighbors.into_iter())
                .collect();
        },
        {
            "apply simulation rules":
            cell_data
                .into_par_iter()
                .for_each(|(state, burning_neighbors)| {
                    let s1 = fastrand::f32();
                    let s2 = fastrand::f32();
                    let s3 = fastrand::f32();
                    if s3 < parameters.lightning_frequency / (width * height) as f32
                        && !state.burning.burning()
                    {
                        state.burning = BurnState::Burning {
                            ticks_remaining: calculate_burn_duration(state, &parameters),
                        }
                    }
                    apply_simulation_rules(state, &parameters, burning_neighbors, s1, s2)
                });
        },
        {
            "cleanup":
            total_time += end_of_last_step.elapsed();
            total_iterations += 1;
            let to_wait =
                (parameters.tick_rate as f32).recip() - end_of_last_step.elapsed().as_secs_f32();
            drop(latest_frame);
            drop(parameters);
            if to_wait > 0.0 {
                std::thread::sleep(Duration::from_secs_f32(to_wait));
            }
            end_of_last_step = Instant::now();
        }
    }
    );
    SimulationStatistics {
        average_step_exec_time: total_time / total_iterations,
        segments,
    }
}

fn apply_simulation_rules(
    state: &mut CellState,
    parameters: &SimulationParameters,
    neighboring_fires: f32,
    fire_random_seed: f32,
    tree_random_seed: f32,
) {
    handle_burn(state);
    let total_flammability = if state.tree {
        parameters.tree_flammability
    } else {
        0.0
    } + state.underbrush * parameters.underbrush_flammability;
    let already_burning = state.burning.burning();
    let burning = already_burning
        || fire_random_seed < neighboring_fires * parameters.fire_spread_rate * total_flammability;
    let new_burn_state = if already_burning || !burning {
        state.burning.clone()
    } else {
        BurnState::Burning {
            ticks_remaining: calculate_burn_duration(state, parameters),
        }
    };

    let tree = state.tree
        || !burning
            && tree_random_seed
                < parameters.tree_growth_rate
                    * (1.0 - parameters.underbrush_tree_growth_hindrance * state.underbrush);

    let underbrush =
        state.underbrush + (!burning) as u32 as f32 * parameters.underbrush_base_accumulation;

    state.underbrush = underbrush;
    state.tree = tree;
    state.burning = new_burn_state;
}

fn handle_burn(cell: &mut CellState) {
    let CellState {
        burning: BurnState::Burning { ticks_remaining },
        ..
    } = cell
    else {
        return;
    };
    if *ticks_remaining > 0 {
        *ticks_remaining -= 1;
    }
    if *ticks_remaining > 1 {
        return;
    }

    cell.burning = BurnState::NotBurning;
    cell.tree = false;
    cell.underbrush = 0.0;
}

fn calculate_burn_duration(state: &CellState, parameters: &SimulationParameters) -> u32 {
    (state.underbrush * parameters.underbrush_fire_duration as f32).round() as u32
        + state.tree as u32 * parameters.tree_fire_duration
}

#[inline(always)]
fn get_burning_neighbors_of_cell(
    grid: &[CellState],
    idx: usize,
    width: usize,
    height: usize,
) -> f32 {
    // Precompute row/col
    let row = idx / width;
    let col = idx % width;

    // Small lookup table; will be fully inlined and optimized away.
    const N: [(isize, isize); 8] = [
        (-1, -1),
        (-1, 0),
        (-1, 1),
        (0, -1),
        (0, 1),
        (1, -1),
        (1, 0),
        (1, 1),
    ];

    let mut count: u8 = 0;

    // Manual bounds checks avoid modulo and wrapping logic.
    for (dr, dc) in N {
        let nr = row as isize + dr;
        let nc = col as isize + dc;

        // Bounds check becomes two predictable comparisons.
        if nr >= 0 && nr < height as isize && nc >= 0 && nc < width as isize {
            let nidx = nr as usize * width + nc as usize;

            // Burning check—typically branch-predictable.
            if grid[nidx].burning.burning() {
                count += 1;
            }
        }
    }

    // Multiply instead of adding 0.125 repeatedly.
    (count as f32) * 0.125
}
