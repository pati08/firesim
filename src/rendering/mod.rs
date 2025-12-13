use crate::sim::*;
use crate::util::Color;

const BURN_COLOR: Color = Color::rgb(255, 0, 0);
const TREE_COLOR: Color = Color::rgb(0, 255, 0);
const UNDERBRUSH_COLOR: Color = Color::rgb(252, 186, 3);
const BACKGROUND_COLOR: Color = Color::rgb(50, 50, 50);

pub fn display_simframe(frame: &SimulationFrame, buf: &mut [u32], width: usize, height: usize) {
    let ratio_x = frame.width as f32 / width as f32;
    let ratio_y = frame.height as f32 / height as f32;
    for x in 0..width {
        for y in 0..height {
            let cell_x = (x as f32 * ratio_x).round() as usize;
            let cell_x = cell_x.min(frame.width - 1);
            let cell_y = (y as f32 * ratio_y).round() as usize;
            let cell_y = cell_y.min(frame.height - 1);
            buf[x + y * width] = match frame.grid[cell_x + cell_y * frame.width] {
                CellState {
                    burning: BurnState::Burning { .. },
                    ..
                } => BURN_COLOR,
                CellState { tree: true, .. } => TREE_COLOR,
                CellState { underbrush, .. } => {
                    BACKGROUND_COLOR.lerp(&UNDERBRUSH_COLOR, underbrush)
                } // _ => BACKGROUND_COLOR,
            }
            .as_u32();
        }
    }
}

pub struct RenderSurfaceSize {
    pub w_px: usize,
    pub h_px: usize,
}
pub trait RenderSurface {
    fn query_size(&self) -> RenderSurfaceSize;
    fn present_frame(&mut self, frame: Vec<usize>);
}
