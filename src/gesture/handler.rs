//! Gesture handler — bridges libinput swipe events to compositor actions.
//!
//! This module implements Smithay's `InputHandler` swipe-gesture callbacks and
//! translates completed [`CompletedGesture`] events into workspace grid
//! navigation or overview mode toggles.

use smithay::backend::input::{
    GestureBeginEvent, GestureEndEvent,
    GestureSwipeBeginEvent, GestureSwipeEndEvent, GestureSwipeUpdateEvent,
};

use crate::{
    gesture::tracker::{CompletedGesture, SwipeAxis},
    state::DewmState,
    workspace::grid::Direction,
};

impl DewmState {
    /// Process a libinput swipe-begin event.
    pub fn handle_gesture_swipe_begin<B: smithay::backend::input::InputBackend>(
        &mut self,
        event: B::GestureSwipeBeginEvent,
    ) where
        B::GestureSwipeBeginEvent: GestureSwipeBeginEvent<B>,
    {
        let fingers = event.fingers() as u32;
        self.gesture_tracker.on_begin(fingers);
        tracing::debug!(fingers, "Swipe begin");
    }

    /// Process a libinput swipe-update event.
    pub fn handle_gesture_swipe_update<B: smithay::backend::input::InputBackend>(
        &mut self,
        event: B::GestureSwipeUpdateEvent,
        screen_size: (u32, u32),
    ) where
        B::GestureSwipeUpdateEvent: GestureSwipeUpdateEvent<B>,
    {
        let (sw, sh) = screen_size;
        self.gesture_tracker.on_update(event.delta().x, event.delta().y, sw, sh);
    }

    /// Process a libinput swipe-end event and dispatch the resulting action.
    pub fn handle_gesture_swipe_end<B: smithay::backend::input::InputBackend>(
        &mut self,
        event: B::GestureSwipeEndEvent,
    ) where
        B::GestureSwipeEndEvent: GestureSwipeEndEvent<B>,
    {
        let Some(gesture) = self.gesture_tracker.on_end(event.cancelled()) else {
            return;
        };

        match gesture {
            CompletedGesture::Cancelled => {
                tracing::debug!("Swipe cancelled");
            }

            CompletedGesture::Navigate3 { delta_x, delta_y, axis } => {
                let dir = direction_from_delta(delta_x, delta_y, axis);
                if let Some(dir) = dir {
                    tracing::debug!(?dir, "3-finger workspace navigate");
                    self.switch_workspace(dir);
                }
            }

            CompletedGesture::Overview4 { delta_y, axis, .. } => {
                if axis == SwipeAxis::Vertical {
                    if delta_y < 0.0 {
                        // Swipe up → open overview
                        tracing::info!("4-finger swipe up → opening overview");
                        self.enter_overview();
                    } else {
                        // Swipe down → close overview
                        tracing::info!("4-finger swipe down → closing overview");
                        self.exit_overview();
                    }
                }
            }
        }
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Map (delta_x, delta_y, axis) → workspace navigation [`Direction`].
///
/// The axis lock tells us which dimension to read; a negative delta means
/// we're swiping *toward* lower index workspaces.
///
/// libinput convention: positive dx = finger moving right, positive dy = finger
/// moving down.  Swiping right with three fingers means "show left workspace"
/// (the same natural-scroll direction used by macOS / GNOME).
fn direction_from_delta(dx: f32, dy: f32, axis: SwipeAxis) -> Option<Direction> {
    match axis {
        SwipeAxis::Horizontal => {
            if dx > 0.0 { Some(Direction::Left)  } // fingers move right → scroll left
            else        { Some(Direction::Right) }  // fingers move left  → scroll right
        }
        SwipeAxis::Vertical => {
            if dy > 0.0 { Some(Direction::Up)   }  // fingers move down → show workspace above
            else        { Some(Direction::Down) }   // fingers move up   → show workspace below
        }
    }
}
