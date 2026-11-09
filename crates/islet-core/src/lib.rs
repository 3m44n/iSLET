//! Core geometry, configuration baseline, and state primitives for Islet.
//!
//! Milestone 1 provides the foundational pill dimensions, easing curves,
//! and shadow padding constants used by the GTK4 layer-shell skeleton.
//! Full state machine and event bus arrive in Milestone 2.

use serde::{Deserialize, Serialize};

/// Logical dimensions (in application pixels before HiDPI scale) for the pill.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PillDimensions {
    pub width: f64,
    pub height: f64,
}

impl PillDimensions {
    pub const fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }

    /// Compact idle pill size (`120x32` logical px).
    pub const IDLE: Self = Self::new(120.0, 32.0);

    /// Expanded media HUD size (`340x76` logical px).
    pub const MEDIA_EXPANDED: Self = Self::new(340.0, 76.0);

    /// Linearly interpolate between two dimensions by factor `t` (`0.0..=1.0`, or with spring overshoot).
    #[must_use]
    pub fn lerp(self, target: Self, t: f64) -> Self {
        Self {
            width: (target.width - self.width).mul_add(t, self.width).max(24.0),
            height: (target.height - self.height).mul_add(t, self.height).max(16.0),
        }
    }

    /// Corner radius for a true capsule/pill (half of current height).
    #[must_use]
    pub fn corner_radius(self) -> f64 {
        self.height * 0.5
    }
}

/// Shadow and glow padding around the pill inside the fixed-size layer surface.
///
/// Reserving a static outer surface size with internal padding prevents calling
/// `wl_surface.commit` size resizes on every animation frame, avoiding
/// compositor-side layout jitter on Hyprland, Sway, River, and Niri.
pub const SURFACE_SHADOW_PADDING: i32 = 24;
pub const MAX_SURFACE_WIDTH: i32 = 420 + (SURFACE_SHADOW_PADDING * 2);
pub const MAX_SURFACE_HEIGHT: i32 = 120 + (SURFACE_SHADOW_PADDING * 2);
pub const DEFAULT_MARGIN_TOP: i32 = 8;

/// Cubic ease-out curve `1 - (1 - t)^3` for fallback or reduced-spring morphs.
#[must_use]
pub fn ease_out_cubic(t: f64) -> f64 {
    let clamped = t.clamp(0.0, 1.0);
    1.0 - (1.0 - clamped).powi(3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pill_dimensions_lerp_and_radius() {
        let start = PillDimensions::IDLE;
        let end = PillDimensions::MEDIA_EXPANDED;

        let mid = start.lerp(end, 0.5);
        assert!((mid.width - 230.0).abs() < f64::EPSILON);
        assert!((mid.height - 54.0).abs() < f64::EPSILON);
        assert!((mid.corner_radius() - 27.0).abs() < f64::EPSILON);
    }

    #[test]
    fn ease_out_cubic_bounds() {
        assert!((ease_out_cubic(0.0) - 0.0).abs() < f64::EPSILON);
        assert!((ease_out_cubic(1.0) - 1.0).abs() < f64::EPSILON);
        assert!(ease_out_cubic(0.5) > 0.8);
    }
}
