use fontdue::{Font, layout::Layout};

pub struct UiContext {
    layout_engine: Layout,
    fonts: Vec<Font>,
}

impl UiContext {
    pub fn new() -> Self {
        let font_bytes = include_bytes!("../../FreeSans.ttf");
        let font = Font::from_bytes(&font_bytes[..], fontdue::FontSettings::default()).unwrap();
        Self {
            layout_engine: Layout::new(fontdue::layout::CoordinateSystem::PositiveYDown),
            fonts: vec![font],
        }
    }
    pub fn layout_engine(&mut self) -> &mut Layout {
        &mut self.layout_engine
    }
}
