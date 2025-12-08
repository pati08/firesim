use crate::{
    ui::engine::{Drawable, Position},
    util::Color,
};

pub struct Rect {
    pub color: Color,
    pub position: Position,
    pub w: usize,
    pub h: usize,
}

impl Drawable for Rect {
    fn draw(
        &self,
        buf: &mut [u32],
        width: usize,
        height: usize,
        _context: &mut super::context::UiContext,
    ) {
        for x in 0..self.w {
            for y in 0..self.h {
                let Some((x, y)) = self.position.apply((x, y)) else {
                    continue;
                };
                if x >= width || y >= height {
                    continue;
                }
                buf[width * y + x] = self.color.as_u32();
            }
        }
    }
}
