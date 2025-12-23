use std::sync::Arc;

use winit::window::Window;

// Keep the old types for compatibility, but mark them as deprecated
/// Legacy render state - kept for compatibility
#[allow(dead_code)]
pub struct RenderState {
    window: Arc<Window>,
    sectioning: RenderSectioning,
}

impl RenderState {
    pub async fn new(window: Arc<Window>) -> Self {
        Self {
            window,
            sectioning: RenderSectioning::Singular(RenderMode::Standard),
        }
    }
    pub fn redraw(&mut self) {}
}

pub enum RenderSectioning {
    Singular(RenderMode),
    Multiple([Option<RenderMode>; 4]),
}

pub enum RenderMode {
    Standard,
}
