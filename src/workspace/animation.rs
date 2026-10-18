//! Smooth workspace pan animation driven by swipe gesture progress.
//!
//! [`PanAnimation`] maintains a 1-D progress value in `[-1.0, 1.0]` that
//! represents how far the viewport has slid toward the next workspace.
//! The renderer reads this value each frame to translate the scene.

use std::time::Duration;

/// Threshold of displacement (as a fraction of screen width/height) above
/// which a swipe *commits* to switching workspaces.
const COMMIT_THRESHOLD: f32 = 0.30;

/// Minimum velocity (normalised units/second) that also commits a swipe.
const COMMIT_VELOCITY: f32 = 0.8;

/// How quickly the progress snaps back to 0.0 when cancelled.
const SNAP_SPEED: f32 = 8.0;

/// How quickly the progress animates to ±1.0 when a commit is triggered.
const COMMIT_SPEED: f32 = 12.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PanPhase {
    /// No animation in progress.
    Idle,
    /// User is actively dragging — progress is 1-to-1 with finger position.
    Dragging,
    /// Swipe committed: animating progress toward ±1.0.
    Committing { target: f32 },
    /// Swipe cancelled: animating progress back toward 0.0.
    SnappingBack,
}

pub struct PanAnimation {
    /// Current pan offset in `[-1.0, 1.0]`.
    /// Positive = sliding toward the next workspace in the swipe direction.
    pub progress: f32,
    /// Instantaneous velocity, tracked for commit detection.
    velocity: f32,
    pub phase: PanPhase,
}

impl PanAnimation {
    pub fn new() -> Self {
        Self {
            progress: 0.0,
            velocity: 0.0,
            phase:    PanPhase::Idle,
        }
    }

    /// Called each frame with the time since the last frame.
    /// Returns `true` while the animation is still in progress.
    pub fn update(&mut self, dt: Duration) -> bool {
        let dt = dt.as_secs_f32();
        match self.phase {
            PanPhase::Idle | PanPhase::Dragging => false,

            PanPhase::Committing { target } => {
                let diff = target - self.progress;
                self.progress += diff * (COMMIT_SPEED * dt).min(1.0);
                if (self.progress - target).abs() < 0.001 {
                    self.progress = target;
                    self.phase    = PanPhase::Idle;
                    return false;
                }
                true
            }

            PanPhase::SnappingBack => {
                self.progress *= 1.0 - (SNAP_SPEED * dt).min(1.0);
                if self.progress.abs() < 0.001 {
                    self.progress = 0.0;
                    self.phase    = PanPhase::Idle;
                    return false;
                }
                true
            }
        }
    }

    // ── Gesture input ─────────────────────────────────────────────────────────

    /// Begin a new gesture.
    pub fn begin(&mut self) {
        self.progress = 0.0;
        self.velocity = 0.0;
        self.phase    = PanPhase::Dragging;
    }

    /// Update with a normalised delta (`dx / screen_width` or `dy / screen_height`).
    pub fn update_drag(&mut self, delta: f32, dt: Duration) {
        let dt = dt.as_secs_f32().max(0.001);
        self.velocity = delta / dt;
        self.progress = (self.progress + delta).clamp(-1.0, 1.0);
    }

    /// End gesture.  Returns `true` if the swipe should **commit** (switch workspace).
    pub fn end(&mut self) -> bool {
        let should_commit = self.progress.abs() > COMMIT_THRESHOLD
            || self.velocity.abs() > COMMIT_VELOCITY;

        if should_commit {
            let target = self.progress.signum(); // +1.0 or -1.0
            self.phase = PanPhase::Committing { target };
        } else {
            self.phase = PanPhase::SnappingBack;
        }
        should_commit
    }

    /// Cancel gesture immediately (e.g. libinput reports cancelled=true).
    pub fn cancel(&mut self) {
        self.phase = PanPhase::SnappingBack;
    }

    pub fn is_idle(&self) -> bool {
        self.phase == PanPhase::Idle
    }
}

impl Default for PanAnimation {
    fn default() -> Self { Self::new() }
}
