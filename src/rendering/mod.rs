use crate::sim::*;
use crate::util::Color;

const BURN_COLOR: Color = Color::rgb(255, 0, 0);
const TREE_COLOR: Color = Color::rgb(0, 255, 0);
const UNDERBRUSH_COLOR: Color = Color::rgb(252, 186, 3);
const BACKGROUND_COLOR: Color = Color::rgb(50, 50, 50);

pub fn render<S: RenderSurface>(state: &RenderState, surface: &mut S) {
    let RenderSurfaceSize {
        w_px: width,
        h_px: height,
    } = surface.query_size();
    let mut buf = vec![0u32; width * height];
    match state {
        RenderState::Singular(section) => {
            display_simframe(
                section.sim_frame,
                &mut buf,
                width,
                height,
                0,
                0,
                &section.mode,
            );
        }
        RenderState::Multiple(sections) => {
            for (sec, (x, y)) in sections.into_iter().zip([
                (0, 0),
                (width / 2, 0),
                (0, height / 2),
                (width / 2, height / 2),
            ]) {
                let Some(sec) = sec else {
                    continue;
                };
                display_simframe(
                    sec.sim_frame,
                    &mut buf,
                    width / 2,
                    height / 2,
                    x,
                    y,
                    &sec.mode,
                );
            }
        }
    }
    surface.present_frame(buf);
}

fn display_simframe(
    frame: &SimulationFrame,
    buf: &mut [u32],
    width: usize,
    height: usize,
    offset_x: usize,
    offset_y: usize,
    mode: &RenderMode,
) {
    let ratio_x = frame.width as f32 / width as f32;
    let ratio_y = frame.height as f32 / height as f32;
    for x in 0..width {
        for y in 0..height {
            let cell_x = (x as f32 * ratio_x).round() as usize;
            let cell_x = cell_x.min(frame.width - 1);
            let cell_y = (y as f32 * ratio_y).round() as usize;
            let cell_y = cell_y.min(frame.height - 1);
            let state = &frame.grid[cell_x + cell_y * frame.width];
            buf[x + offset_x + (y + offset_y) * width] = match mode {
                RenderMode::Standard => match state {
                    CellState {
                        burning: BurnState::Burning { .. },
                        ..
                    } => BURN_COLOR,
                    CellState { tree: true, .. } => TREE_COLOR,
                    CellState { underbrush, .. } => {
                        BACKGROUND_COLOR.lerp(&UNDERBRUSH_COLOR, *underbrush)
                    }
                }
                .as_u32(),
            };
        }
    }
}

pub struct RenderSurfaceSize {
    pub w_px: usize,
    pub h_px: usize,
}
pub trait RenderSurface {
    fn query_size(&self) -> RenderSurfaceSize;
    fn present_frame(&mut self, frame: Vec<u32>);
}

pub enum RenderState<'a> {
    Singular(RenderSection<'a>),
    Multiple([Option<RenderSection<'a>>; 4]),
}

pub struct RenderSection<'a> {
    pub sim_frame: &'a SimulationFrame,
    pub mode: RenderMode,
}

pub enum RenderMode {
    Standard,
}
