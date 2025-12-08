use fontdue::{
    Font,
    layout::{GlyphPosition, Layout},
};

use crate::util::Color;

pub struct TextStyle {
    pub px: f32,
    pub color: Color,
}

pub struct TextBitmap {
    buf: Vec<u8>,
    width: usize,
    height: usize,
}

pub struct TextMetrics {
    offset: (f32, f32),
    width: f32,
    height: f32,
}

fn rasterize_string(fonts: &[Font], engine: &mut Layout, s: &str, style: TextStyle) {
    let glyphs = layout_text(fonts, engine, s, style);
    let total_width = glyphs
        .last()
        .map(|v| v.width as i32 + v.x as i32)
        .unwrap_or(0) as usize;
}

fn layout_text(
    fonts: &[Font],
    engine: &mut Layout,
    text: &str,
    style: TextStyle,
) -> Vec<GlyphPosition> {
    let text_style = fontdue::layout::TextStyle::new(text, style.px, 0);
    engine.append(fonts, &text_style);
    let glyphs = engine.glyphs();
    engine.clear();
    glyphs
}
