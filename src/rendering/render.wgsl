// Render shader for fire simulation visualization
// This shader reads from the compute shader's cell buffer and renders a fullscreen quad

// Cell structure must match the compute shader exactly
struct Cell {
    tree: f32,
    underbrush: f32,
    fire: u32,
    _padding: u32,
}

// Grid size uniform
struct GridSize {
    width: u32,
    height: u32,
}

// Bind group 0: Cell data (read-only for rendering)
@group(0) @binding(0)
var<storage, read> cells: array<Cell>;

// Bind group 1: Grid size
@group(1) @binding(0)
var<uniform> grid_size: GridSize;

// Vertex output / Fragment input
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

// Fullscreen triangle vertex shader
// Uses vertex index to generate a fullscreen triangle (3 vertices cover the entire screen)
@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var out: VertexOutput;
    
    // Generate fullscreen triangle vertices
    // Triangle covers clip space from (-1,-1) to (3,3), which is clipped to (-1,-1) to (1,1)
    let x = f32(i32(vertex_index & 1u) * 4 - 1);
    let y = f32(i32(vertex_index >> 1u) * 4 - 1);
    
    out.position = vec4<f32>(x, y, 0.0, 1.0);
    
    // UV coordinates: map from clip space to [0,1] range
    // Note: flip Y so that (0,0) is top-left
    out.uv = vec2<f32>(
        (x + 1.0) * 0.5,
        (1.0 - y) * 0.5
    );
    
    return out;
}

// Color constants
const BURN_COLOR: vec3<f32> = vec3<f32>(1.0, 0.2, 0.0);      // Bright orange-red for fire
const TREE_COLOR: vec3<f32> = vec3<f32>(0.133, 0.545, 0.133); // Forest green
const UNDERBRUSH_COLOR: vec3<f32> = vec3<f32>(0.545, 0.353, 0.169); // Saddle brown
const BACKGROUND_COLOR: vec3<f32> = vec3<f32>(0.196, 0.196, 0.196);  // Dark gray

// Fragment shader - samples the cell buffer and outputs color
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Calculate which cell this pixel corresponds to
    let cell_x = u32(in.uv.x * f32(grid_size.width));
    let cell_y = u32(in.uv.y * f32(grid_size.height));
    
    // Clamp to valid range
    let x = min(cell_x, grid_size.width - 1u);
    let y = min(cell_y, grid_size.height - 1u);
    
    // Calculate buffer index
    let index = x + y * grid_size.width;
    
    // Get cell state
    let cell = cells[index];
    
    // Determine color based on cell state
    var color: vec3<f32>;
    
    if (cell.fire > 0u) {
        // Cell is burning - interpolate between yellow and red based on intensity
        let intensity = min(f32(cell.fire) / 10.0, 1.0);
        let yellow = vec3<f32>(1.0, 0.9, 0.0);
        color = mix(BURN_COLOR, yellow, intensity * 0.5);
    } else if (cell.tree > 0.5) {
        // Cell has a tree - show tree color, slightly modulated by underbrush
        let underbrush_factor = clamp(cell.underbrush, 0.0, 1.0);
        color = mix(TREE_COLOR, TREE_COLOR * 0.7 + UNDERBRUSH_COLOR * 0.3, underbrush_factor * 0.3);
    } else {
        // No tree - interpolate between background and underbrush color
        let underbrush_factor = clamp(cell.underbrush, 0.0, 1.0);
        color = mix(BACKGROUND_COLOR, UNDERBRUSH_COLOR, underbrush_factor);
    }
    
    return vec4<f32>(color, 1.0);
}
