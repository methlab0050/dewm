//! 2D workspace grid backed by BTreeMap with conditional auto-expansion.
//!
//! # Grid Layout
//!
//! Workspaces are addressed by signed `(row, col)` coordinates in Z x Z:
//!
//! ```text
//! (-1,-1) ←→ (-1,0) ←→ (-1,1)
//!    ↕          ↕         ↕
//!  (0,-1) ←→  (0,0) ←→  (0,1)
//!    ↕          ↕         ↕
//!  (1,-1) ←→  (1,0) ←→  (1,1)
//! ```
//!
//! # Expansion Rule
//!
//! A swipe can only **create** a new workspace if the *source* workspace has
//! at least one window. An empty workspace will block swipe attempts toward
//! non-existent neighbours in all 4 directions, snapping the animation back.
//! However, swiping *toward an existing* workspace is always permitted regardless of occupancy.

use std::collections::BTreeMap;
use smithay::{
    desktop::Window,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
};

use super::workspace::Workspace;

/// Cardinal direction of a workspace-navigation gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// 2D grid of [`Workspace`]s backed by a [`BTreeMap`].
pub struct WorkspaceGrid {
    /// `workspaces[(row, col)]`
    workspaces: BTreeMap<(i32, i32), Workspace>,
    /// Currently focused cell `(row, col)`.
    active: (i32, i32),
}

impl WorkspaceGrid {
    /// Create a grid initialized with a single empty workspace at origin `(0, 0)`.
    pub fn new() -> Self {
        let mut workspaces = BTreeMap::new();
        workspaces.insert((0, 0), Workspace::new());
        Self {
            workspaces,
            active: (0, 0),
        }
    }

    // ── Accessors ─────────────────────────────────────────────────────────────

    /// Currently active workspace coordinates `(row, col)`.
    pub fn active(&self) -> (i32, i32) {
        self.active
    }

    /// Immutable reference to a workspace, if it exists.
    pub fn get(&self, coord: (i32, i32)) -> Option<&Workspace> {
        self.workspaces.get(&coord)
    }

    /// Mutable reference to a workspace, if it exists.
    pub fn get_mut(&mut self, coord: (i32, i32)) -> Option<&mut Workspace> {
        self.workspaces.get_mut(&coord)
    }

    /// Read-only access to all workspaces map.
    pub fn workspaces(&self) -> &BTreeMap<(i32, i32), Workspace> {
        &self.workspaces
    }

    /// Mutable access to all workspaces map.
    pub fn workspaces_mut(&mut self) -> &mut BTreeMap<(i32, i32), Workspace> {
        &mut self.workspaces
    }

    /// Currently active [`Workspace`].
    pub fn active_workspace(&self) -> &Workspace {
        self.workspaces.get(&self.active).expect("active workspace must exist")
    }

    /// Currently active [`Workspace`] mutably.
    pub fn active_workspace_mut(&mut self) -> &mut Workspace {
        self.workspaces.get_mut(&self.active).expect("active workspace must exist")
    }

    /// Returns the coordinate bounds `(min_r, max_r, min_c, max_c)` encompassing all allocated workspaces.
    pub fn bounds(&self) -> (i32, i32, i32, i32) {
        let mut min_r = self.active.0;
        let mut max_r = self.active.0;
        let mut min_c = self.active.1;
        let mut max_c = self.active.1;
        for &(r, c) in self.workspaces.keys() {
            min_r = min_r.min(r);
            max_r = max_r.max(r);
            min_c = min_c.min(c);
            max_c = max_c.max(c);
        }
        (min_r, max_r, min_c, max_c)
    }

    /// Returns the grid dimensions `(rows, cols)` covering the bounding box.
    pub fn dimensions(&self) -> (usize, usize) {
        let (min_r, max_r, min_c, max_c) = self.bounds();
        let rows = (max_r - min_r + 1).max(1) as usize;
        let cols = (max_c - min_c + 1).max(1) as usize;
        (rows, cols)
    }

    // ── Navigation ────────────────────────────────────────────────────────────

    /// Attempt to navigate in `dir`.
    ///
    /// Returns `Some((row, col))` of the new active workspace on success, or
    /// `None` if the move is blocked (swipe should snap back).
    ///
    /// # Expansion rule
    ///
    /// | Situation                                 | Result              |
    /// |-------------------------------------------|---------------------|
    /// | Target workspace already exists           | Always allowed      |
    /// | Target does not exist AND source occupied | Creates target      |
    /// | Target does not exist AND source empty    | **Blocked** → None  |
    pub fn navigate(&mut self, dir: Direction) -> Option<(i32, i32)> {
        let (r, c) = self.active;
        let target = Self::neighbour_coords(r, c, dir);

        if self.exists(target) {
            // Navigating to an existing workspace is unconditionally allowed.
            self.active = target;
            Some(target)
        } else {
            // Only expand if the current workspace has at least one window.
            if self.active_workspace().is_occupied() {
                self.ensure_exists(target);
                self.active = target;
                Some(target)
            } else {
                // Empty source → snap back.
                None
            }
        }
    }

    /// Teleport directly to a coordinate (used by overview click / drag-drop).
    /// Creates the workspace if it does not exist.
    pub fn teleport_to(&mut self, target: (i32, i32)) {
        self.ensure_exists(target);
        self.active = target;
    }

    // ── Window management ─────────────────────────────────────────────────────

    /// Remove a window from whichever workspace currently owns it, identified
    /// by its `WlSurface`. Returns the owning cell coordinates if found.
    pub fn remove_window_by_surface(&mut self, surface: &WlSurface) -> Option<(i32, i32)> {
        for (&coord, ws) in self.workspaces.iter_mut() {
            if ws.remove_by_surface(surface).is_some() {
                return Some(coord);
            }
        }
        None
    }

    /// Move `window` from its current workspace cell to `target`.
    /// Creates `target` workspace if needed (drag-to-new-cell in overview).
    pub fn move_window_to(&mut self, window: &Window, target: (i32, i32)) {
        // Remove from current location.
        for ws in self.workspaces.values_mut() {
            if ws.remove_window(window) {
                break;
            }
        }
        self.ensure_exists(target);
        self.workspaces.get_mut(&target).unwrap().add_window(window.clone());
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    /// Compute the `(row, col)` of the neighbour in `dir`.
    pub fn neighbour_coords(r: i32, c: i32, dir: Direction) -> (i32, i32) {
        match dir {
            Direction::Up    => (r - 1, c),
            Direction::Down  => (r + 1, c),
            Direction::Left  => (r,     c - 1),
            Direction::Right => (r,     c + 1),
        }
    }

    /// Returns `true` if `(r, c)` exists in the grid.
    pub fn exists(&self, target: (i32, i32)) -> bool {
        self.workspaces.contains_key(&target)
    }

    /// Ensure that `(r, c)` exists in the grid.
    pub fn ensure_exists(&mut self, target: (i32, i32)) {
        self.workspaces.entry(target).or_default();
    }
}

impl Default for WorkspaceGrid {
    fn default() -> Self { Self::new() }
}

// ─── Unit Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_at_origin() {
        let grid = WorkspaceGrid::new();
        assert_eq!(grid.active(), (0, 0));
        assert!(grid.exists((0, 0)));
    }

    #[test]
    fn blocked_from_empty_workspace_in_all_directions() {
        let mut grid = WorkspaceGrid::new();
        // (0,0) is empty → swiping any direction into non-existent space should be blocked.
        assert!(grid.navigate(Direction::Right).is_none());
        assert!(grid.navigate(Direction::Left).is_none());
        assert!(grid.navigate(Direction::Up).is_none());
        assert!(grid.navigate(Direction::Down).is_none());
        assert_eq!(grid.active(), (0, 0), "Should not have moved");
    }

    #[test]
    fn expand_into_negative_quadrant() {
        let mut grid = WorkspaceGrid::new();
        // Manually place a placeholder window in (0, 0) to simulate occupancy.
        // We'll mark it occupied by adding a window or directly testing navigate when occupied.
        // Let's create an existing negative workspace to test navigation:
        grid.ensure_exists((-1, -1));
        // Direct navigation to existing negative cell always succeeds
        grid.teleport_to((-1, -1));
        assert_eq!(grid.active(), (-1, -1));
        assert!(grid.exists((-1, -1)));
    }

    #[test]
    fn navigate_to_existing_workspace_always_allowed() {
        let mut grid = WorkspaceGrid::new();
        // Create an adjacent workspace at (-1, 0)
        grid.ensure_exists((-1, 0));
        // Moving Up into an existing workspace succeeds even if source is empty
        assert_eq!(grid.navigate(Direction::Up), Some((-1, 0)));
        assert_eq!(grid.active(), (-1, 0));
    }

    #[test]
    fn teleport_creates_workspace() {
        let mut grid = WorkspaceGrid::new();
        grid.teleport_to((-2, 3));
        assert_eq!(grid.active(), (-2, 3));
        assert!(grid.get((-2, 3)).is_some());
        let (min_r, max_r, min_c, max_c) = grid.bounds();
        assert_eq!(min_r, -2);
        assert_eq!(max_r, 0);
        assert_eq!(min_c, 0);
        assert_eq!(max_c, 3);
    }

    #[test]
    fn neighbour_coords_signed() {
        assert_eq!(WorkspaceGrid::neighbour_coords(0, 0, Direction::Left), (0, -1));
        assert_eq!(WorkspaceGrid::neighbour_coords(0, 0, Direction::Up), (-1, 0));
        assert_eq!(WorkspaceGrid::neighbour_coords(-1, -1, Direction::Left), (-1, -2));
        assert_eq!(WorkspaceGrid::neighbour_coords(-1, -1, Direction::Up), (-2, -1));
        assert_eq!(WorkspaceGrid::neighbour_coords(-1, -1, Direction::Right), (-1, 0));
        assert_eq!(WorkspaceGrid::neighbour_coords(-1, -1, Direction::Down), (0, -1));
    }
}
