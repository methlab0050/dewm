//! Multi-touch swipe gesture state machine.
//!
//! [`GestureTracker`] processes libinput swipe events (begin / update / end /
//! cancel) and maintains the current gesture phase.  The handler in
//! `gesture::handler` consumes events from this tracker and dispatches high-
//! level actions to the workspace grid and overview subsystems.
//!
//! # Finger-count routing
//!
//! | Fingers | Action                                                 |
//! |---------|--------------------------------------------------------|
//! | 3       | Workspace navigation (4-way pan)                       |
//! | 4       | Swipe Up → Overview open; Swipe Down → Overview close  |

use std::time::Instant;

/// Which axis dominates the current swipe (decided on the first meaningful
/// delta update once `AXIS_LOCK_THRESHOLD` is exceeded).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwipeAxis {
    Horizontal,
    Vertical,
}

/// High-level gesture state.
#[derive(Debug, Clone)]
pub enum GestureState {
    /// No active gesture.
    Idle,

    /// 3-finger workspace-navigation swipe in progress.
    Swiping3 {
        /// Cumulative displacement along the dominant axis (normalised 0..1).
        delta_x: f32,
        delta_y: f32,
        /// Axis lock, determined once `|delta| > AXIS_LOCK_THRESHOLD`.
        axis: Option<SwipeAxis>,
        started_at: Instant,
    },

    /// 4-finger swipe that will trigger overview open/close.
    Swiping4 {
        delta_x: f32,
        delta_y: f32,
        axis: Option<SwipeAxis>,
        started_at: Instant,
    },
}

/// Minimum displacement (pixels) before we lock to an axis.
const AXIS_LOCK_THRESHOLD: f32 = 10.0;

pub struct GestureTracker {
    pub state: GestureState,
}

impl GestureTracker {
    pub fn new() -> Self {
        Self { state: GestureState::Idle }
    }

    // ── libinput event handlers ───────────────────────────────────────────────

    /// Called when libinput fires `GestureSwipeBegin`.
    pub fn on_begin(&mut self, finger_count: u32) {
        self.state = match finger_count {
            3 => GestureState::Swiping3 {
                delta_x: 0.0,
                delta_y: 0.0,
                axis: None,
                started_at: Instant::now(),
            },
            4 => GestureState::Swiping4 {
                delta_x: 0.0,
                delta_y: 0.0,
                axis: None,
                started_at: Instant::now(),
            },
            _ => GestureState::Idle,
        };
    }

    /// Called when libinput fires `GestureSwipeUpdate`.
    /// `dx` / `dy` are raw pixel deltas from libinput.
    /// `screen_w` / `screen_h` are used to normalise deltas into [0..1].
    pub fn on_update(&mut self, dx: f64, dy: f64, screen_w: u32, screen_h: u32) {
        let ndx = dx as f32 / screen_w as f32;
        let ndy = dy as f32 / screen_h as f32;

        match &mut self.state {
            GestureState::Swiping3 { delta_x, delta_y, axis, .. }
            | GestureState::Swiping4 { delta_x, delta_y, axis, .. } => {
                *delta_x += ndx;
                *delta_y += ndy;

                // Axis lock: once either dimension exceeds threshold, lock.
                if axis.is_none() {
                    let adx = delta_x.abs();
                    let ady = delta_y.abs();
                    if adx > AXIS_LOCK_THRESHOLD / screen_w as f32 {
                        *axis = Some(SwipeAxis::Horizontal);
                    } else if ady > AXIS_LOCK_THRESHOLD / screen_h as f32 {
                        *axis = Some(SwipeAxis::Vertical);
                    }
                }
            }
            GestureState::Idle => {}
        }
    }

    /// Called when libinput fires `GestureSwipeEnd`.
    /// Returns the completed gesture (takes ownership) so the handler can act.
    pub fn on_end(&mut self, cancelled: bool) -> Option<CompletedGesture> {
        let state = std::mem::replace(&mut self.state, GestureState::Idle);
        if cancelled {
            return Some(CompletedGesture::Cancelled);
        }
        completed_from_state(state)
    }

    pub fn is_active(&self) -> bool {
        !matches!(self.state, GestureState::Idle)
    }
}

impl Default for GestureTracker {
    fn default() -> Self { Self::new() }
}

// ─── Completed gesture ────────────────────────────────────────────────────────

/// The resolved outcome of a completed gesture, passed to the handler.
#[derive(Debug)]
pub enum CompletedGesture {
    /// 3-finger swipe along an axis.
    Navigate3 { delta_x: f32, delta_y: f32, axis: SwipeAxis },
    /// 4-finger swipe along an axis.
    Overview4 { delta_x: f32, delta_y: f32, axis: SwipeAxis },
    /// Gesture was cancelled by libinput (e.g. too many fingers lifted).
    Cancelled,
}

fn completed_from_state(state: GestureState) -> Option<CompletedGesture> {
    match state {
        GestureState::Swiping3 { delta_x, delta_y, axis, .. } => {
            let axis = axis.unwrap_or_else(|| dominant_axis(delta_x, delta_y));
            Some(CompletedGesture::Navigate3 { delta_x, delta_y, axis })
        }
        GestureState::Swiping4 { delta_x, delta_y, axis, .. } => {
            let axis = axis.unwrap_or_else(|| dominant_axis(delta_x, delta_y));
            Some(CompletedGesture::Overview4 { delta_x, delta_y, axis })
        }
        GestureState::Idle => None,
    }
}

fn dominant_axis(dx: f32, dy: f32) -> SwipeAxis {
    if dx.abs() >= dy.abs() {
        SwipeAxis::Horizontal
    } else {
        SwipeAxis::Vertical
    }
}
