//! Overview mode animation state.
//!
//! The overview is a zoomed-out thumbnail grid showing all workspaces on a 2D map.
//! Workspaces are rendered at a consistent 0.5x scale.
//! It is triggered by a 4-finger swipe up or Super/Alt+Tab, and dismissed by
//! a 4-finger swipe down, Escape, or clicking a workspace tile.
//!
//! Two-finger swipes scroll/pan the overview camera across the map.

use std::time::Duration;
use smithay::utils::{Logical, Point};

/// Consistent scale factor for workspaces in the overview (half size).
pub const OVERVIEW_SCALE: f32 = 0.5;

/// Gap between workspace cards in the overview map (pixels).
pub const OVERVIEW_GAP: f32 = 32.0;

/// Animation speed (higher = faster transition).
const ANIM_SPEED: f32 = 6.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverviewPhase {
    /// Overview fully closed — normal compositor view.
    Closed,
    /// Animating toward fully open (zooming out).
    Opening,
    /// Overview fully open and interactive.
    Open,
    /// Animating back toward fully closed (zooming in).
    Closing,
}

pub struct OverviewState {
    /// `0.0` = normal desktop (1.0x), `1.0` = overview fully visible (0.5x).
    pub animation_progress: f32,
    pub phase: OverviewPhase,

    /// Which workspace cell the pointer is currently hovering over (if any).
    pub hovered_cell: Option<(i32, i32)>,

    /// Which workspace cell the camera is currently snapped/focused on.
    pub focused_cell: (i32, i32),

    /// 2D camera center in overview map coordinates.
    pub camera_pos: Point<f32, Logical>,
    /// Target camera position when panning / gliding.
    pub target_camera: Point<f32, Logical>,

    /// Accumulated scroll delta for snapping threshold.
    pub scroll_accum: Point<f32, Logical>,
    /// Last time a snap occurred (for gesture debounce).
    pub last_snap_time: Option<std::time::Instant>,

    /// Workspace that was active when entering the overview.
    pub entry_workspace: (i32, i32),
}

impl OverviewState {
    pub fn new() -> Self {
        Self {
            animation_progress: 0.0,
            phase:              OverviewPhase::Closed,
            hovered_cell:       None,
            focused_cell:       (0, 0),
            camera_pos:         Point::from((0.0, 0.0)),
            target_camera:      Point::from((0.0, 0.0)),
            scroll_accum:       Point::from((0.0, 0.0)),
            last_snap_time:     None,
            entry_workspace:    (0, 0),
        }
    }

    /// Compute the center position of a workspace cell in overview coordinates.
    pub fn cell_center(coord: (i32, i32), output_size: (i32, i32)) -> Point<f32, Logical> {
        let card_w = (output_size.0 as f32) * OVERVIEW_SCALE;
        let card_h = (output_size.1 as f32) * OVERVIEW_SCALE;
        let stride_x = card_w + OVERVIEW_GAP;
        let stride_y = card_h + OVERVIEW_GAP;
        Point::from((coord.1 as f32 * stride_x, coord.0 as f32 * stride_y))
    }

    /// Trigger the overview to open, centering on `active_cell`.
    pub fn open(&mut self, active_cell: (i32, i32), output_size: (i32, i32)) {
        if !matches!(self.phase, OverviewPhase::Open | OverviewPhase::Opening) {
            let center = Self::cell_center(active_cell, output_size);
            self.camera_pos = center;
            self.target_camera = center;
            self.entry_workspace = active_cell;
            self.focused_cell = active_cell;
            self.scroll_accum = Point::from((0.0, 0.0));
            self.last_snap_time = None;
            self.phase = OverviewPhase::Opening;
        }
    }

    /// Trigger the overview to close (zoom into active workspace).
    pub fn close(&mut self) {
        if !matches!(self.phase, OverviewPhase::Closed | OverviewPhase::Closing) {
            self.phase = OverviewPhase::Closing;
        }
    }

    /// Toggle open/closed.
    #[allow(dead_code)]
    pub fn toggle(&mut self, active_cell: (i32, i32), output_size: (i32, i32)) {
        match self.phase {
            OverviewPhase::Closed | OverviewPhase::Closing => self.open(active_cell, output_size),
            OverviewPhase::Open   | OverviewPhase::Opening => self.close(),
        }
    }

    /// Scroll / snap the overview camera to adjacent workspaces.
    pub fn snap_scroll(
        &mut self,
        delta: Point<f32, Logical>,
        is_discrete: bool,
        grid: &crate::workspace::grid::WorkspaceGrid,
        output_size: (i32, i32),
    ) {
        const SNAP_THRESHOLD: f32 = 30.0;
        const SNAP_COOLDOWN_MS: u128 = 160;

        // Reset accumulation if fingers lift / stop
        if delta.x == 0.0 && delta.y == 0.0 {
            self.scroll_accum = Point::from((0.0, 0.0));
            return;
        }

        self.scroll_accum += delta;

        let now = std::time::Instant::now();
        if let Some(last) = self.last_snap_time {
            if !is_discrete && now.duration_since(last).as_millis() < SNAP_COOLDOWN_MS {
                return;
            }
        }

        let abs_x = self.scroll_accum.x.abs();
        let abs_y = self.scroll_accum.y.abs();

        if is_discrete || abs_x >= SNAP_THRESHOLD || abs_y >= SNAP_THRESHOLD {
            let dir = if abs_x > abs_y {
                if self.scroll_accum.x > 0.0 {
                    SnapDirection::Right
                } else {
                    SnapDirection::Left
                }
            } else {
                if self.scroll_accum.y > 0.0 {
                    SnapDirection::Down
                } else {
                    SnapDirection::Up
                }
            };

            if let Some(target) = find_snap_target(self.focused_cell, dir, grid) {
                self.focused_cell = target;
                self.target_camera = Self::cell_center(target, output_size);
                self.last_snap_time = Some(now);
                self.scroll_accum = Point::from((0.0, 0.0));
                tracing::debug!(?target, ?dir, "Overview: snapped to workspace");
            } else {
                // At the edge: apply subtle bounce nudge in that direction
                let nudge = match dir {
                    SnapDirection::Right => Point::from((20.0, 0.0)),
                    SnapDirection::Left  => Point::from((-20.0, 0.0)),
                    SnapDirection::Down  => Point::from((0.0, 20.0)),
                    SnapDirection::Up    => Point::from((0.0, -20.0)),
                };
                self.camera_pos += nudge;
                self.scroll_accum = Point::from((0.0, 0.0));
            }
        }
    }

    /// Scroll / pan the overview camera freely (legacy / direct pan).
    #[allow(dead_code)]
    pub fn pan(&mut self, delta: Point<f32, Logical>) {
        self.camera_pos += delta;
        self.target_camera = self.camera_pos;
    }

    pub fn is_visible(&self) -> bool {
        self.animation_progress > 0.001
    }

    #[allow(dead_code)]
    pub fn is_interactive(&self) -> bool {
        matches!(self.phase, OverviewPhase::Open)
    }

    /// Advance animation by `dt`. Call this every compositor frame.
    /// Returns `true` while an animation is in progress (caller should schedule a redraw).
    pub fn update(&mut self, dt: Duration) -> bool {
        let dt_sec = dt.as_secs_f32();

        // Smooth camera lerp towards target
        let cam_diff = self.target_camera - self.camera_pos;
        if cam_diff.x.abs() > 0.5 || cam_diff.y.abs() > 0.5 {
            let factor = (dt_sec * 12.0).clamp(0.0, 1.0);
            self.camera_pos += Point::from((cam_diff.x * factor, cam_diff.y * factor));
        } else {
            self.camera_pos = self.target_camera;
        }

        match self.phase {
            OverviewPhase::Opening => {
                self.animation_progress += ANIM_SPEED * dt_sec;
                if self.animation_progress >= 1.0 {
                    self.animation_progress = 1.0;
                    self.phase = OverviewPhase::Open;
                    return false;
                }
                true
            }
            OverviewPhase::Closing => {
                self.animation_progress -= ANIM_SPEED * dt_sec;
                if self.animation_progress <= 0.0 {
                    self.animation_progress = 0.0;
                    self.phase = OverviewPhase::Closed;
                    return false;
                }
                true
            }
            _ => cam_diff.x.abs() > 0.5 || cam_diff.y.abs() > 0.5,
        }
    }
}

impl Default for OverviewState {
    fn default() -> Self { Self::new() }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapDirection {
    Right,
    Left,
    Down,
    Up,
}

/// Find the next workspace to snap to in `dir` from `current`.
pub fn find_snap_target(
    current: (i32, i32),
    dir: SnapDirection,
    grid: &crate::workspace::grid::WorkspaceGrid,
) -> Option<(i32, i32)> {
    let workspaces = grid.workspaces();
    if workspaces.is_empty() {
        return None;
    }

    let (curr_r, curr_c) = if workspaces.contains_key(&current) {
        current
    } else {
        grid.active()
    };

    match dir {
        SnapDirection::Right => {
            if workspaces.contains_key(&(curr_r, curr_c + 1)) {
                return Some((curr_r, curr_c + 1));
            }
            workspaces
                .keys()
                .filter(|&&(_r, c)| c > curr_c)
                .min_by_key(|&&(r, c)| (c - curr_c, (r - curr_r).abs()))
                .copied()
        }
        SnapDirection::Left => {
            if workspaces.contains_key(&(curr_r, curr_c - 1)) {
                return Some((curr_r, curr_c - 1));
            }
            workspaces
                .keys()
                .filter(|&&(_r, c)| c < curr_c)
                .min_by_key(|&&(r, c)| (curr_c - c, (r - curr_r).abs()))
                .copied()
        }
        SnapDirection::Down => {
            if workspaces.contains_key(&(curr_r + 1, curr_c)) {
                return Some((curr_r + 1, curr_c));
            }
            let down_candidate = workspaces
                .keys()
                .filter(|&&(r, _c)| r > curr_r)
                .min_by_key(|&&(r, c)| (r - curr_r, (c - curr_c).abs()))
                .copied();

            if down_candidate.is_some() {
                down_candidate
            } else {
                // Fallback for horizontal row layouts: scroll down moves to next workspace on right
                workspaces
                    .keys()
                    .filter(|&&(_r, c)| c > curr_c)
                    .min_by_key(|&&(r, c)| (c - curr_c, (r - curr_r).abs()))
                    .copied()
            }
        }
        SnapDirection::Up => {
            if workspaces.contains_key(&(curr_r - 1, curr_c)) {
                return Some((curr_r - 1, curr_c));
            }
            let up_candidate = workspaces
                .keys()
                .filter(|&&(r, _c)| r < curr_r)
                .min_by_key(|&&(r, c)| (curr_r - r, (c - curr_c).abs()))
                .copied();

            if up_candidate.is_some() {
                up_candidate
            } else {
                // Fallback for horizontal row layouts: scroll up moves to previous workspace on left
                workspaces
                    .keys()
                    .filter(|&&(_r, c)| c < curr_c)
                    .min_by_key(|&&(r, c)| (curr_c - c, (r - curr_r).abs()))
                    .copied()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overview_open_and_close_transitions() {
        let mut state = OverviewState::new();
        assert_eq!(state.phase, OverviewPhase::Closed);
        assert!(!state.is_visible());

        state.open((0, 0), (1920, 1080));
        assert_eq!(state.phase, OverviewPhase::Opening);

        // Advance animation
        let animating = state.update(Duration::from_millis(100));
        assert!(animating);
        assert!(state.animation_progress > 0.0);
        assert!(state.is_visible());

        // Advance to full open
        while state.update(Duration::from_millis(50)) {}
        assert_eq!(state.phase, OverviewPhase::Open);
        assert!((state.animation_progress - 1.0).abs() < 1e-4);
        assert!(state.is_interactive());

        // Close
        state.close();
        assert_eq!(state.phase, OverviewPhase::Closing);
        while state.update(Duration::from_millis(50)) {}
        assert_eq!(state.phase, OverviewPhase::Closed);
        assert!(!state.is_visible());
    }

    #[test]
    fn overview_toggle() {
        let mut state = OverviewState::new();
        state.toggle((0, 0), (1920, 1080));
        assert_eq!(state.phase, OverviewPhase::Opening);

        state.animation_progress = 1.0;
        state.phase = OverviewPhase::Open;

        state.toggle((0, 0), (1920, 1080));
        assert_eq!(state.phase, OverviewPhase::Closing);
    }

    #[test]
    fn overview_pan() {
        let mut state = OverviewState::new();
        state.open((0, 0), (1920, 1080));
        let initial = state.camera_pos;
        state.pan(Point::from((50.0, -30.0)));
        assert!((state.camera_pos.x - (initial.x + 50.0)).abs() < 1e-4);
        assert!((state.camera_pos.y - (initial.y - 30.0)).abs() < 1e-4);
    }

    #[test]
    fn snap_target_horizontal_row() {
        let mut grid = crate::workspace::grid::WorkspaceGrid::new();
        grid.ensure_exists((0, 0));
        grid.ensure_exists((0, 1));
        grid.ensure_exists((0, 2));

        // Right from (0, 0) -> (0, 1)
        assert_eq!(find_snap_target((0, 0), SnapDirection::Right, &grid), Some((0, 1)));
        assert_eq!(find_snap_target((0, 1), SnapDirection::Right, &grid), Some((0, 2)));
        assert_eq!(find_snap_target((0, 2), SnapDirection::Right, &grid), None);

        // Left from (0, 2) -> (0, 1)
        assert_eq!(find_snap_target((0, 2), SnapDirection::Left, &grid), Some((0, 1)));
        assert_eq!(find_snap_target((0, 0), SnapDirection::Left, &grid), None);

        // Vertical scroll fallback on 1D horizontal row:
        // Down navigates rightwards: (0, 0) -> (0, 1)
        assert_eq!(find_snap_target((0, 0), SnapDirection::Down, &grid), Some((0, 1)));
        // Up navigates leftwards: (0, 2) -> (0, 1)
        assert_eq!(find_snap_target((0, 2), SnapDirection::Up, &grid), Some((0, 1)));
    }

    #[test]
    fn snap_target_2d_grid() {
        let mut grid = crate::workspace::grid::WorkspaceGrid::new();
        grid.ensure_exists((0, 0));
        grid.ensure_exists((0, 1));
        grid.ensure_exists((1, 0));
        grid.ensure_exists((1, 1));

        assert_eq!(find_snap_target((0, 0), SnapDirection::Right, &grid), Some((0, 1)));
        assert_eq!(find_snap_target((0, 0), SnapDirection::Down, &grid), Some((1, 0)));
        assert_eq!(find_snap_target((1, 1), SnapDirection::Left, &grid), Some((1, 0)));
        assert_eq!(find_snap_target((1, 1), SnapDirection::Up, &grid), Some((0, 1)));
    }

    #[test]
    fn snap_scroll_updates_target_camera() {
        let mut grid = crate::workspace::grid::WorkspaceGrid::new();
        grid.ensure_exists((0, 0));
        grid.ensure_exists((0, 1));

        let mut state = OverviewState::new();
        let out_size = (1920, 1080);
        state.open((0, 0), out_size);
        assert_eq!(state.focused_cell, (0, 0));

        let origin_cam = OverviewState::cell_center((0, 0), out_size);
        let target_cam = OverviewState::cell_center((0, 1), out_size);
        assert_eq!(state.target_camera, origin_cam);

        // Discrete scroll right
        state.snap_scroll(Point::from((1.0, 0.0)), true, &grid, out_size);
        assert_eq!(state.focused_cell, (0, 1));
        assert_eq!(state.target_camera, target_cam);

        // Update lerps camera towards target
        assert!(state.update(Duration::from_millis(16)));
        assert!(state.camera_pos.x > origin_cam.x);
    }
}
