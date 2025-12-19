// Array stride: 16 bytes
struct Cell {
    tree: f32,
    underbrush: f32,
    fire: u32,
    @size(4) _padding: u32, // pad to 16 bytes
}

struct Parameters {
    /// The base chance (0 - 1) that a tree will grow in a given cell each tick
    tree_growth_rate: f32,
    /// The factor by which the tree growth rate is reduced with underbrush.
    underbrush_tree_growth_hindrance: f32,
    /// The base rate of underbrush accumulation
    tree_underbrush_generation: f32,
    /// The amount of underbrush created when a tree dies naturally
    tree_death_underbrush: f32,
    /// The chance (0 - 1) that a particular tree dies naturally each tick
    tree_death_rate: f32,
    /// The length a single tree can support a fire for in ticks
    tree_fire_duration: u32,
    /// The length that underbrush can support a fire for in ticks. This is
    /// multiplied by the amount of underbrush
    underbrush_fire_duration: u32,
    /// The base chance (0 - 1) that fire spreads from a particular cell to a
    /// particular neighbor cell
    fire_spread_rate: f32,
    /// The multiplier for fire spread rate for trees
    tree_flammability: f32,
    /// The multiplier for fire spread rate for underbrush (multiplied by the
    /// amount of underbrush). This is added with the value from tree_flammability
    /// to calculate the final chance
    underbrush_flammability: f32,
    /// The chance (0 - 1) of a lightning strike each tick, globally
    lightning_frequency: f32,
    /// The tick rate in ticks per second (unused in this shader)
    tick_rate: u32,
}

@group(0) @binding(0)
var<storage, read_write> input: array<Cell>;
// Output of the shader.  
@group(0) @binding(1)
var<storage, read_write> output: array<Cell>;
// Simulation parameters input
@group(1) @binding(0)
var<uniform> params: Parameters;
// Size of the grid
@group(2) @binding(0)
var <uniform> size: vec2<u32>;
// Step count (use only for rng)
@group(3) @binding(0)
var <uniform> steps: u32;

fn random(s: u32, count: u32) -> f32 {
    // 1. Combine all three inputs using bitwise XOR and large primes
    // Each prime helps "spread" the bits of that specific variable
    var state = s;
    state ^= steps * 2654435769u;
    state ^= count * 3405691582u;

    // 2. The PCG "Mixing" Stage
    state = state * 747796405u + 2891336453u;
    var word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    var result = (word >> 22u) ^ word;

    // 3. Normalize to [0.0, 1.0]
    return f32(result) / f32(0xffffffffu);
}

struct NeighboringCellInfo {
    trees: u32,
    fires: u32,
    underbrush: f32,
}

// Ideal workgroup size depends on the hardware, the workload, and other factors. However, it should
// _generally_ be a multiple of 64. Common sizes are 64x1x1, 256x1x1; or 8x8x1, 16x16x1 for 2D workloads.
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    // While compute invocations are 3d, we're only using one dimension.
    let index = global_id.x;

    // Because we're using a workgroup size of 64, if the input size isn't a multiple of 64,
    // we will have some "extra" invocations. This is fine, but we should tell them to stop
    // to avoid out-of-bounds accesses.
    let array_length = arrayLength(&input);
    if (global_id.x >= array_length) {
        return;
    }

    // Do the multiply by two and write to the output.
    output[global_id.x] = input[global_id.x];
    apply_rules(global_id.x);
    // output[global_id.x].underbrush = random(global_id.x + steps);
    if (global_id.x == 0) {
        output[global_id.x].underbrush = 1.0;
    }
}

fn apply_rules(global_x: u32) {
    let neighboring_cell_info = get_neighboring_cell_info(global_x);

    output[global_x] = input[global_x];
    
    // Extinguish burnt-out fires
    if (input[global_x].fire > 0u) {
        output[global_x].fire = input[global_x].fire - 1;
        if (output[global_x].fire == 0u) {
            output[global_x].tree = 0.0;
            output[global_x].underbrush = 0.0;
        }
    }
    // Handle fire spreading
    var total_flammability: f32 = input[global_x].underbrush * params.underbrush_flammability + input[global_x].tree * params.tree_flammability;
    let already_burning = input[global_x].fire > 0u;
    let catches_fire = random(global_x, 0) < (f32(neighboring_cell_info.fires) / 8.0) * params.fire_spread_rate * total_flammability
        || random(global_x, 3) < params.lightning_frequency / f32(size.x * size.y);
    if (catches_fire && !already_burning) {
        output[global_x].fire = burn_duration(global_x);
    }
    var tree_dies = false;
    // Handle natural tree death
    if (input[global_x].tree > 0.0f && random(global_x, 1) < params.tree_death_rate) {
        output[global_x].tree = 0.0;
        tree_dies = true;
    }

    if (!already_burning && !catches_fire) {
        // Handle tree growth
        if (input[global_x].tree == 0.0 && random(global_x, 2) < params.tree_growth_rate * (1.0 - params.underbrush_tree_growth_hindrance * input[global_x].underbrush)) {
            output[global_x].tree = 1.0;
        }

        // Underbrush generation
        output[global_x].underbrush = input[global_x].underbrush + (output[global_x].tree + f32(neighboring_cell_info.trees)) * params.tree_underbrush_generation;
        if (tree_dies) {
            output[global_x].underbrush += params.tree_death_underbrush;
        }
    }
}

fn burn_duration(global_x: u32) -> u32 {
    return u32(round(input[global_x].underbrush)) * params.underbrush_fire_duration + u32(round(input[global_x].tree)) * params.tree_fire_duration;
}

fn get_neighboring_cell_info(global_x: u32) -> NeighboringCellInfo {
    var total_trees: u32 = 0;
    var total_fires: u32 = 0;
    var total_underbrush: f32 = 0;
    let row = global_x / size.x;
    let col = global_x % size.x;
    let width = size.x;
    let height = size.y;
    if (col > 0) {
        if (row > 0) {
            total_trees += u32(ceil(input[global_x - width - 1].tree));
            total_fires += min(1, input[global_x - width - 1].fire);
            total_underbrush += input[global_x - width - 1].underbrush;
        }
        if (row < size.y - 1) {
            total_trees += u32(ceil(input[global_x + width - 1].tree));
            total_fires += min(1, input[global_x + width - 1].fire);
            total_underbrush += input[global_x + width - 1].underbrush;
        }
        total_trees += u32(ceil(input[global_x - 1].tree));
        total_fires += min(1, input[global_x - 1].fire);
        total_underbrush += input[global_x - 1].underbrush;
    }
    if (col < size.x - 1) {
        if (row > 0) {
            total_trees += u32(ceil(input[global_x - width + 1].tree));
            total_fires += min(1, input[global_x - width + 1].fire);
            total_underbrush += input[global_x - width + 1].underbrush;
        }
        if (row < size.y) {
            total_trees += u32(ceil(input[global_x + width + 1].tree));
            total_fires += min(1, input[global_x + width + 1].fire);
            total_underbrush += input[global_x + width + 1].underbrush;
        }
        total_trees += u32(ceil(input[global_x + 1].tree));
        total_fires += min(1, input[global_x + 1].fire);
        total_underbrush += input[global_x + 1].underbrush;
    }
    if (row > 0) {
        total_trees += u32(ceil(input[global_x - width].tree));
        total_fires += min(1, input[global_x - width].fire);
        total_underbrush += input[global_x - width].underbrush;
    }
    if (row < size.y - 1) {
        total_trees += u32(ceil(input[global_x + width].tree));
        total_fires += min(1, input[global_x + width].fire);
        total_underbrush += input[global_x + width].underbrush;
    }
    return NeighboringCellInfo(total_trees, total_fires, total_underbrush);
}
