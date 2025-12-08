use super::context::UiContext;

// pub trait BoundedDrawable {
//     fn width(&self) -> usize;
//     fn height(&self) -> usize;
//     fn pixel_buf(&self, context: &mut UiContext) -> Vec<u32>;
//     fn transform(&self) -> Transform;
// }

pub trait Drawable {
    fn draw(&self, buf: &mut [u32], width: usize, height: usize, context: &mut UiContext);
}

#[derive(Copy, Clone)]
pub struct Position {
    pub x: i32,
    pub y: i32,
}

impl Position {
    pub fn apply(&self, (x, y): (usize, usize)) -> Option<(usize, usize)> {
        let x = x as i32 + self.x;
        let y = y as i32 + self.y;
        if x < 0 || y < 0 {
            return None;
        }
        Some((x as usize, y as usize))
    }
}

pub trait CompositeDrawable {
    fn components(&self) -> Vec<&dyn Drawable>;
}
