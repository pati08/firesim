#[derive(Clone, Copy, Debug)]
pub struct Color {
    r: u8,
    g: u8,
    b: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
    pub fn as_u32(&self) -> u32 {
        (self.r as u32) << 16 | (self.g as u32) << 8 | (self.b as u32)
    }
    pub fn lerp(&self, other: &Color, factor: f32) -> Color {
        let r = (self.r as f32 + (other.r as f32 - self.r as f32) * factor).round() as u8;
        let g = (self.g as f32 + (other.g as f32 - self.g as f32) * factor).round() as u8;
        let b = (self.b as f32 + (other.b as f32 - self.b as f32) * factor).round() as u8;
        Color { r, g, b }
    }
}
