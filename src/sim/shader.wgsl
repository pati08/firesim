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
var<storage, read> input: array<Cell>;
// Output of the shader.  
@group(0) @binding(1)
var<storage, read_write> output: array<Cell>;
// Simulation parameters input
@group(1) @binding(0)
var<uniform, read> params: Parameters;
// Size of the grid
@group(2) @binding(0)
var <uniform, read> size: vec2<u32>;

fn random(seed: vec2f) -> f32 {
    let dot_product = dot(seed, vec2f(12.9898, 78.233));
    return fract(sin(dot_product) * 43758.5453123);
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
    output[global_id.x].tree = f32(global_id.x % 2);
}
