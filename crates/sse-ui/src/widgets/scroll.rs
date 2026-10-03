//! Scroll viewport state for a retained widget tree.
//! Wheel movement is clamped; repaint damage is limited to the viewport.

use crate::raster::Rect;
use crate::widget::Tree;

/// C# thin scrollbar colour.
pub const THUMB: u32 = 0x3D4837;
/// Lighter hover thumb.
pub const THUMB_HOVER: u32 = 0x56644D;
/// Scrollbar thickness in logical pixels.
pub const BAR_WIDTH: f32 = 10.0;

/// Retained scroll viewport.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollView {
    offset_y: f32,
    content_height: f32,
    viewport_height: f32,
    wheel_step: f32,
    hover_thumb: bool,
}

impl ScrollView {
    /// Create a viewport at offset zero.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            offset_y: 0.0,
            content_height: 0.0,
            viewport_height: 0.0,
            wheel_step: 48.0,
            hover_thumb: false,
        }
    }

    /// Update virtual content and viewport heights, clamping the current offset.
    pub fn set_extent(&mut self, content_height: f32, viewport_height: f32) {
        self.content_height = content_height.max(0.0);
        self.viewport_height = viewport_height.max(0.0);
        self.offset_y = self.offset_y.min(self.max_offset());
    }

    /// Current vertical content offset.
    #[must_use]
    pub const fn offset_y(self) -> f32 {
        self.offset_y
    }

    /// Largest legal vertical offset.
    #[must_use]
    pub fn max_offset(self) -> f32 {
        (self.content_height - self.viewport_height).max(0.0)
    }

    /// Apply vertical wheel lines; returns whether the visible content changed.
    pub fn wheel(&mut self, lines: f32) -> bool {
        let old = self.offset_y;
        self.offset_y = (self.offset_y - lines * self.wheel_step).clamp(0.0, self.max_offset());
        old != self.offset_y
    }

    /// Scroll to an absolute content offset; true when it changed.
    pub fn scroll_to(&mut self, y: f32) -> bool {
        let old = self.offset_y;
        self.offset_y = y.clamp(0.0, self.max_offset());
        old != self.offset_y
    }

    /// Update scrollbar-thumb hover state; true when it changed.
    pub fn set_thumb_hover(&mut self, hover: bool) -> bool {
        let changed = self.hover_thumb != hover;
        self.hover_thumb = hover;
        changed
    }

    /// Current scrollbar thumb RGB colour.
    #[must_use]
    pub const fn thumb_color(self) -> u32 {
        if self.hover_thumb {
            THUMB_HOVER
        } else {
            THUMB
        }
    }

    /// Thumb y/height inside a viewport, or None when all content fits.
    #[must_use]
    pub fn thumb(self) -> Option<(f32, f32)> {
        if self.content_height <= self.viewport_height || self.viewport_height <= 0.0 {
            return None;
        }
        let h = (self.viewport_height * self.viewport_height / self.content_height)
            .max(24.0)
            .min(self.viewport_height);
        let travel = self.viewport_height - h;
        Some((self.offset_y / self.max_offset() * travel, h))
    }

    /// Damage only the visible viewport in the retained tree.
    pub fn damage_visible(&self, tree: &mut Tree, viewport: Rect) {
        tree.add_damage(viewport);
    }
}

impl Default for ScrollView {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_and_sizes_thumb() {
        let mut s = ScrollView::new();
        s.set_extent(1000.0, 200.0);
        assert!(s.scroll_to(900.0));
        assert_eq!(s.offset_y(), 800.0);
        let (y, h) = s.thumb().unwrap();
        assert_eq!(y + h, 200.0);
        assert!(!s.scroll_to(900.0));
    }

    #[test]
    fn no_thumb_when_fit() {
        let mut s = ScrollView::new();
        s.set_extent(100.0, 200.0);
        assert_eq!(s.thumb(), None);
    }
}
