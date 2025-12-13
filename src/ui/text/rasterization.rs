use fontdue::{
    Font, Metrics,
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
    // let total_width = glyphs
    //     .last()
    //     .map(|v| v.width as i32 + v.x as i32)
    //     .unwrap_or(0) as usize;
    let mut buf: Vec<Vec<u8>> = Vec::new();
    let mut bottom_offset = 0usize;
    for glyph in glyphs {
        let (
            Metrics {
                xmin,
                ymin,
                width,
                height,
                advance_width,
                ..
            },
            bitmap,
        ) = fonts[glyph.font_index].rasterize_config(glyph.key);

        // Extend the buffer upwards (-y) to make room for any negative y offset
        if ymin < 0 && bottom_offset < ymin.unsigned_abs() as usize {
            let diff = ymin.unsigned_abs() as usize - bottom_offset;
            bottom_offset = ymin.unsigned_abs() as usize;
            let current_width = buf.first().map(|i| i.len()).unwrap_or_default();
            buf.extend(std::iter::repeat_n(vec![0; current_width], diff));
            buf.rotate_right(diff);
        }

        let horizontal_baseline = buf.get(0)
        let new_width = 
    }
}

fn layout_text(
    fonts: &[Font],
    engine: &mut Layout,
    text: &str,
    style: TextStyle,
) -> Vec<GlyphPosition> {
    let text_style = fontdue::layout::TextStyle::new(text, style.px, 0);
    engine.append(fonts, &text_style);
    let glyphs = engine.glyphs().clone();
    engine.clear();
    glyphs
}
