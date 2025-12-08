use super::text::Text;
use crate::ui::engine::{Drawable, Position};
use crate::ui::primitives::Rect;
use crate::util::Color;

pub struct Button {
    position: Position,
    label: Text,
    w: usize,
    h: usize,
    state: ButtonState,
    style: ButtonStyle,
}

impl Drawable for Button {
    fn draw(
        &self,
        buf: &mut [u32],
        width: usize,
        height: usize,
        context: &mut super::context::UiContext,
    ) {
        self.current_rect().draw(buf, width, height, context);
        // self.lab
    }
}

impl Button {
    fn current_rect(&self) -> Rect {
        let color = match self.state {
            ButtonState::Pressed => self.style.bg_color_pressed,
            ButtonState::Unpressed => self.style.bg_color,
        };
        Rect {
            color,
            w: self.w,
            h: self.h,
            position: self.position,
        }
    }
}

pub struct ButtonStyle {
    bg_color: Color,
    bg_color_pressed: Color,
}

pub enum ButtonState {
    Pressed,
    Unpressed,
}

pub struct FloatSlider {
    label: Text,
    value: f32,
    min: f32,
    max: f32,
}
