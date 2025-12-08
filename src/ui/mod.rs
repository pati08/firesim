use fontdue::layout::Layout;
use minifb::Window;

mod components;
mod context;
mod engine;
mod primitives;
mod text;
use components::{Button, FloatSlider};
use text::Text;

pub struct Ui {
    window: Window,
    mouse_state: MouseState,
    text_layout: Layout,
    state: UiState,
}

struct UiState {
    sliders: Vec<FloatSlider>,
    buttons: Vec<Button>,
    labels: Vec<Text>,
}

pub enum MouseState {
    Down { start_pos: (f32, f32) },
    Up,
}
