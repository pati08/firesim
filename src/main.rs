use minifb::{Key, Window, WindowOptions};

use firesim::{
    rendering,
    sim::{Simulation, SimulationFrame, SimulationParameters},
};

const WINDOW_WIDTH: usize = 1000;
const WINDOW_HEIGHT: usize = 1000;

const SIM_WIDTH: usize = 500;
const SIM_HEIGHT: usize = 500;

fn main() {
    run_graphical_simulation();
    // run_benchmark();
}

fn run_graphical_simulation() {
    let mut buffer: Vec<u32> = vec![0; WINDOW_WIDTH * WINDOW_HEIGHT];

    let mut window = Window::new(
        "Test - ESC to exit",
        WINDOW_WIDTH,
        WINDOW_HEIGHT,
        WindowOptions::default(),
    )
    .expect("failed to open window");

    let start_frame = SimulationFrame::new(SIM_WIDTH, SIM_HEIGHT);
    let sim_params = SimulationParameters::realistic(SIM_WIDTH, SIM_HEIGHT, 10.0, 36.0);
    let sim = Simulation::spawn(start_frame, sim_params);

    // Limit to max ~60 fps update rate
    window.set_target_fps(60);

    while window.is_open() && !window.is_key_down(Key::Escape) {
        let latest_frame = sim.get_latest_frame();
        rendering::display_simframe(&latest_frame, &mut buffer, WINDOW_WIDTH, WINDOW_HEIGHT);

        // We unwrap here as we want this code to exit if it fails. Real applications may want to handle this in a different way
        window
            .update_with_buffer(&buffer, WINDOW_WIDTH, WINDOW_HEIGHT)
            .unwrap();
    }
    let _stats = sim.stop();
}

pub fn run_benchmark() {
    let start_frame = SimulationFrame::new(100, 100);
    let sim_params = SimulationParameters {
        // General properties
        lightning_frequency: 0.01,
        tick_rate: 999999999,
        // Tree lifecycle properties
        tree_growth_rate: 0.00025,
        tree_death_rate: 0.0,

        // Fire properties
        tree_fire_duration: 10,
        tree_flammability: 1.0,
        fire_spread_rate: 1.0,

        // Disable underbrush to start
        underbrush_tree_growth_hindrance: 0.0,
        tree_underbrush_generation: 0.001,
        underbrush_fire_duration: 0,
        underbrush_flammability: 0.0,
        tree_death_underbrush: 0.0,
    };

    let sim = Simulation::spawn(start_frame, sim_params);

    for i in (1..=10).rev() {
        println!("{i}...");
        std::thread::sleep(std::time::Duration::from_secs(1));
    }

    let stats = sim.stop();
    println!(
        "simulation took an average execution time of {}μs",
        stats.average_step_exec_time.as_micros()
    );
    println!("\n\nSegments:\n");
    let longest_name = stats.segments.iter().map(|i| i.0.len()).max().unwrap();
    for (name, time) in stats.segments {
        println!("{} {}μs", pad(name, longest_name + 2), time.as_micros());
    }
}

fn pad(s: &str, len: usize) -> String {
    if s.len() >= len {
        return s.to_string();
    }
    format!("{s}{}", " ".repeat(len - s.len()))
}
