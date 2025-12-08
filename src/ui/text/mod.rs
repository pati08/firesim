use super::engine::Position;
use crate::{ui::engine::Drawable, util::Color};

mod rasterization;
use rasterization::TextBitmap;
pub use rasterization::TextStyle;

pub struct Text {
    position: Position,
    contents: String,
    style: TextStyle,
    bitmap: Option<TextBitmap>,
}

impl Text {
    pub fn new(position: Position, text: String, style: TextStyle) -> Self {
        Self {
            position,
            contents: text,
            style,
            bitmap: None,
        }
    }
    // fn rasterize(&self) -> (textBitMap)
}

impl Drawable for Text {
    fn draw(
        &self,
        buf: &mut [u32],
        width: usize,
        height: usize,
        context: &mut super::context::UiContext,
    ) {
        //
    }
}
