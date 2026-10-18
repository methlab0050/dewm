//! Overview mode animation state.
//!
//! The overview is a zoomed-out thumbnail grid showing all workspaces.
//! It is triggered by a 4-finger swipe up and dismissed by a 4-finger swipe
//! down or the Escape key.
//!
//! `animation_progress` drives the lerp between the normal desktop view
//! and the fully-zoomed overview.  The renderer reads this value each frame.

use std::time::Duration;

/// Animation speed (higher = faster transition).
const ANIM_SPEED: f32 = 6.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverviewPhase {
    /// Overview fully closed — normal compositor view.
    Closed,
    /// Animating toward fully open.
    Opening,
    /// Overview fully open and interactive.
    Open,
    /// Animating back toward fully closed.
    Closing,
}

pub struct OverviewState {
    /// `0.0` = normal desktop, `1.0` = overview fully visible.
    pub animation_progress: f32,
    pub phase: OverviewPhase,

    /// Which workspace cell the pointer is currently hovering over (if any).
    pub hovered_cell: Option<(usize, usize)>,
}

impl OverviewState {
    pub fn new() -> Self {
        Self {
            animation_progress: 0.0,
            phase:              OverviewPhase::Closed,
            hovered_cell:       None,
        }
    }

    /// Trigger the overview to open.
    pub fn open(&mut self) {
        if !matches!(self.phase, OverviewPhase::Open | OverviewPhase::Opening) {
            self.phase = OverviewPhase::Opening;
        }
    }

    /// Trigger the overview to close.
    pub fn close(&mut self) {
        if !matches!(self.phase, OverviewPhase::Closed | OverviewPhase::Closing) {
            self.phase = OverviewPhase::Closing;
        }
    }

    /// Toggle open/closed.
    pub fn toggle(&mut self) {
        match self.phase {
            OverviewPhase::Closed | OverviewPhase::Closing => self.open(),
            OverviewPhase::Open   | OverviewPhase::Opening => self.close(),
        }
    }

    pub fn is_visible(&self) -> bool {
        self.animation_progress > 0.001
    }

    pub fn is_interactive(&self) -> bool {
        matches!(self.phase, OverviewPhase::Open)
    }

    /// Advance animation by `dt`.  Call this every compositor frame.
    /// Returns `true` while an animation is in progress (caller should schedule a redraw).
    pub fn update(&mut self, dt: Duration) -> bool {
        let dt = dt.as_secs_f32();
        match self.phase {
            OverviewPhase::Opening => {
                self.animation_progress += ANIM_SPEED * dt;
                if self.animation_progress >= 1.0 {
                    self.animation_progress = 1.0;
                    self.phase = OverviewPhase::Open;
                    return false;
                }
                true
            }
            OverviewPhase::Closing => {
                self.animation_progress -= ANIM_SPEED * dt;
                if self.animation_progress <= 0.0 {
                    self.animation_progress = 0.0;
                    self.phase = OverviewPhase::Closed;
                    return false;
                }
                true
            }
            _ => false,
        }
    }

    /// Compute the scale factor for each workspace thumbnail given the current
    /// grid dimensions.  Scale shrinks so all cells fit on screen.
    pub fn thumbnail_scale(rows: usize, cols: usize) -> f32 {
        let max_dim = rows.max(cols).max(1) as f32;
        1.0 / max_dim
    }

    /// Compute the pixel position (top-left) of a thumbnail cell given the
    /// output dimensions and grid layout.
    pub fn cell_position(
        r: usize,
        c: usize,
        rows: usize,
        cols: usize,
        output_w: u32,
        output_h: u32,
    ) -> (f32, f32) {
        let cell_w = output_w as f32 / cols as f32;
        let cell_h = output_h as f32 / rows as f32;
        (c as f32 * cell_w, r as f32 * cell_h)
    }
}

impl Default for OverviewState {
    fn default() -> Self { Self::new() }
}
