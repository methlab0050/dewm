//! Smooth 2D workspace pan animation driven by swipe gestures and keyboard navigation.
//!
//! [`PanAnimation`] tracks the camera location in `Space` logical coordinates.
//! During pan transitions, it smoothly interpolates the viewport camera between
//! workspace origins using spring-damped exponential easing.

use std::time::Duration;
use smithay::utils::{Logical, Point};
use crate::workspace::grid::Direction;

/// Animation speed (spring frequency coefficient).
const ANIMATION_SPEED: f64 = 14.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PanPhase {
    /// No animation in progress, camera is stationary at active workspace origin.
    Idle,
    /// User is actively dragging fingers in real-time (3-finger gesture).
    Dragging,
    /// Animating toward target camera position.
    Animating,
}

pub struct PanAnimation {
    /// Current camera location in Space logical coordinates.
    pub current_camera: Point<f64, Logical>,
    /// Target camera location in Space logical coordinates.
    pub target_camera: Point<f64, Logical>,
    /// Camera location when current animation / drag started.
    pub start_camera: Point<f64, Logical>,
    /// Current phase of animation.
    pub phase: PanPhase,
    /// Velocity of drag for momentum commit.
    pub velocity: Point<f64, Logical>,
}

impl PanAnimation {
    pub fn new() -> Self {
        Self {
            current_camera: Point::from((0.0, 0.0)),
            target_camera:  Point::from((0.0, 0.0)),
            start_camera:   Point::from((0.0, 0.0)),
            phase:          PanPhase::Idle,
            velocity:       Point::from((0.0, 0.0)),
        }
    }

    /// Check if the pan animation is currently running or dragging.
    pub fn is_active(&self) -> bool {
        self.phase != PanPhase::Idle
    }

    /// Check if stationary.
    pub fn is_idle(&self) -> bool {
        self.phase == PanPhase::Idle
    }

    /// Start an animated transition from current camera to target camera position.
    pub fn start_transition(&mut self, target: Point<f64, Logical>) {
        self.start_camera = self.current_camera;
        self.target_camera = target;
        self.phase = PanPhase::Animating;
    }

    /// Begin a live gesture drag from current camera position.
    pub fn begin_gesture(&mut self) {
        self.start_camera = self.current_camera;
        self.velocity = Point::from((0.0, 0.0));
        self.phase = PanPhase::Dragging;
    }

    /// Update gesture drag displacement (in pixels).
    pub fn update_gesture_drag(&mut self, delta: Point<f64, Logical>, dt: Duration) {
        let dt_secs = dt.as_secs_f64().max(0.001);
        let new_cam = self.start_camera + delta;
        let diff = new_cam - self.current_camera;
        self.velocity = Point::from((diff.x / dt_secs, diff.y / dt_secs));
        self.current_camera = new_cam;
    }

    /// Nudge slightly in direction `dir` and spring back to `origin` (when blocked).
    pub fn nudge_and_snap_back(&mut self, origin: Point<f64, Logical>, dir: Direction) {
        let nudge_offset = match dir {
            Direction::Left  => Point::from((-40.0, 0.0)),
            Direction::Right => Point::from((40.0, 0.0)),
            Direction::Up    => Point::from((0.0, -40.0)),
            Direction::Down  => Point::from((0.0, 40.0)),
        };
        self.current_camera = origin + nudge_offset;
        self.start_camera = self.current_camera;
        self.target_camera = origin;
        self.phase = PanPhase::Animating;
    }

    /// Update animation state by delta time `dt`. Returns `true` if camera moved.
    pub fn update(&mut self, dt: Duration) -> bool {
        match self.phase {
            PanPhase::Idle => false,
            PanPhase::Dragging => true,
            PanPhase::Animating => {
                let dt_secs = dt.as_secs_f64();
                let factor = 1.0 - (-ANIMATION_SPEED * dt_secs).exp();

                self.current_camera.x += (self.target_camera.x - self.current_camera.x) * factor;
                self.current_camera.y += (self.target_camera.y - self.current_camera.y) * factor;

                let dist_x = (self.target_camera.x - self.current_camera.x).abs();
                let dist_y = (self.target_camera.y - self.current_camera.y).abs();

                if dist_x < 0.5 && dist_y < 0.5 {
                    self.current_camera = self.target_camera;
                    self.phase = PanPhase::Idle;
                    true
                } else {
                    true
                }
            }
        }
    }
}

impl Default for PanAnimation {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state_is_idle() {
        let anim = PanAnimation::new();
        assert!(!anim.is_active());
        assert_eq!(anim.current_camera, Point::from((0.0, 0.0)));
    }

    #[test]
    fn transition_interpolates_to_target() {
        let mut anim = PanAnimation::new();
        let target = Point::from((1920.0, 0.0));
        anim.start_transition(target);
        assert!(anim.is_active());

        // Step a few frames
        let dt = Duration::from_millis(16);
        let mut steps = 0;
        while anim.is_active() && steps < 120 {
            anim.update(dt);
            steps += 1;
        }

        assert_eq!(anim.current_camera, target);
        assert!(!anim.is_active());
        assert!(steps > 5, "Animation should take multiple frames");
    }

    #[test]
    fn nudge_and_snap_back_returns_to_origin() {
        let mut anim = PanAnimation::new();
        let origin = Point::from((0.0, 0.0));
        anim.nudge_and_snap_back(origin, Direction::Right);
        assert!(anim.is_active());
        assert_eq!(anim.current_camera, Point::from((40.0, 0.0)));

        let dt = Duration::from_millis(16);
        let mut steps = 0;
        while anim.is_active() && steps < 120 {
            anim.update(dt);
            steps += 1;
        }

        assert_eq!(anim.current_camera, origin);
        assert!(!anim.is_active());
    }
}
