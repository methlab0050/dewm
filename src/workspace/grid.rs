//! 2D workspace grid with conditional auto-expansion.
//!
//! # Grid Layout
//!
//! Workspaces are addressed by `(row, col)` coordinates:
//!
//! ```text
//! (0,0) ←→ (0,1) ←→ (0,2)
//!   ↕         ↕         ↕
//! (1,0) ←→ (1,1) ←→ (1,2)
//! ```
//!
//! # Expansion Rule
//!
//! A swipe can only **create** a new workspace if the *source* workspace has
//! at least one window.  An empty workspace will block swipe attempts toward
//! non-existent neighbours, snapping the animation back.  However, swiping
//! *toward an existing* workspace is always permitted regardless of occupancy.

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

/// 2D grid of [`Workspace`]s.
pub struct WorkspaceGrid {
    /// `workspaces[row][col]`
    workspaces: Vec<Vec<Workspace>>,
    /// Currently focused cell `(row, col)`.
    active: (usize, usize),
}

impl WorkspaceGrid {
    /// Create a 1×1 grid with a single empty workspace.
    pub fn new() -> Self {
        Self {
            workspaces: vec![vec![Workspace::new()]],
            active: (0, 0),
        }
    }

    // ── Accessors ─────────────────────────────────────────────────────────────

    /// Currently active workspace coordinates `(row, col)`.
    pub fn active(&self) -> (usize, usize) {
        self.active
    }

    /// Immutable reference to a workspace, if it exists.
    pub fn get(&self, (r, c): (usize, usize)) -> Option<&Workspace> {
        self.workspaces.get(r)?.get(c)
    }

    /// Mutable reference to a workspace, if it exists.
    pub fn get_mut(&mut self, (r, c): (usize, usize)) -> Option<&mut Workspace> {
        self.workspaces.get_mut(r)?.get_mut(c)
    }

    /// Mutable access to the entire grid (used by drag-in-overview).
    pub fn workspaces_mut(&mut self) -> &mut Vec<Vec<Workspace>> {
        &mut self.workspaces
    }

    /// Currently active [`Workspace`].
    pub fn active_workspace(&self) -> &Workspace {
        let (r, c) = self.active;
        &self.workspaces[r][c]
    }

    /// Currently active [`Workspace`] mutably.
    pub fn active_workspace_mut(&mut self) -> &mut Workspace {
        let (r, c) = self.active;
        &mut self.workspaces[r][c]
    }

    /// Returns the grid dimensions `(rows, cols)`.
    pub fn dimensions(&self) -> (usize, usize) {
        let rows = self.workspaces.len();
        let cols = self.workspaces.first().map_or(0, |r| r.len());
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
    pub fn navigate(&mut self, dir: Direction) -> Option<(usize, usize)> {
        let (r, c) = self.active;
        let target = self.neighbour_coords(r, c, dir)?;

        if self.exists(target) {
            // Navigating to an existing workspace is unconditionally allowed.
            self.active = target;
            Some(target)
        } else {
            // Only expand if the current workspace has at least one window.
            if self.workspaces[r][c].is_occupied() {
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
    pub fn teleport_to(&mut self, target: (usize, usize)) {
        self.ensure_exists(target);
        self.active = target;
    }

    // ── Window management ─────────────────────────────────────────────────────

    /// Remove a window from whichever workspace currently owns it, identified
    /// by its `WlSurface`.  Returns the owning cell coordinates if found.
    pub fn remove_window_by_surface(&mut self, surface: &WlSurface) -> Option<(usize, usize)> {
        for (r, row) in self.workspaces.iter_mut().enumerate() {
            for (c, ws) in row.iter_mut().enumerate() {
                if ws.remove_by_surface(surface).is_some() {
                    return Some((r, c));
                }
            }
        }
        None
    }

    /// Move `window` from its current workspace cell to `target`.
    /// Creates `target` workspace if needed (drag-to-new-cell in overview).
    pub fn move_window_to(&mut self, window: &Window, target: (usize, usize)) {
        // Remove from current location.
        for row in self.workspaces.iter_mut() {
            for ws in row.iter_mut() {
                if ws.remove_window(window) {
                    break;
                }
            }
        }
        self.ensure_exists(target);
        self.workspaces[target.0][target.1].add_window(window.clone());
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    /// Compute the `(row, col)` of the neighbour in `dir`, or `None` if the
    /// result would underflow (e.g. moving Up from row 0).
    fn neighbour_coords(&self, r: usize, c: usize, dir: Direction) -> Option<(usize, usize)> {
        Some(match dir {
            Direction::Up    => (r.checked_sub(1)?, c),
            Direction::Down  => (r + 1,             c),
            Direction::Left  => (r,                 c.checked_sub(1)?),
            Direction::Right => (r,                 c + 1),
        })
    }

    /// Returns `true` if `(r, c)` is within the current bounds of the grid.
    fn exists(&self, (r, c): (usize, usize)) -> bool {
        self.workspaces.get(r).and_then(|row| row.get(c)).is_some()
    }

    /// Grow the grid until `(r, c)` is a valid index.
    fn ensure_exists(&mut self, (r, c): (usize, usize)) {
        // Extend rows.
        while self.workspaces.len() <= r {
            let cols = self.workspaces[0].len();
            self.workspaces.push(vec![Workspace::new(); cols]);
        }
        // Extend columns in every row.
        for row in self.workspaces.iter_mut() {
            while row.len() <= c {
                row.push(Workspace::new());
            }
        }
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
    }

    #[test]
    fn blocked_from_empty_workspace() {
        let mut grid = WorkspaceGrid::new();
        // (0,0) is empty → swiping Right should be blocked.
        assert!(grid.navigate(Direction::Right).is_none());
        assert_eq!(grid.active(), (0, 0), "Should not have moved");
    }

    #[test]
    fn up_from_row_zero_is_none() {
        let mut grid = WorkspaceGrid::new();
        assert!(grid.navigate(Direction::Up).is_none());
    }

    #[test]
    fn left_from_col_zero_is_none() {
        let mut grid = WorkspaceGrid::new();
        assert!(grid.navigate(Direction::Left).is_none());
    }

    #[test]
    fn navigate_to_existing_workspace_always_allowed() {
        let mut grid = WorkspaceGrid::new();
        // Manually expand the grid and place active at (0,1).
        grid.ensure_exists((0, 1));
        grid.active = (0, 1);
        // (0,0) already exists → moving Left should succeed even though (0,1) is empty.
        assert_eq!(grid.navigate(Direction::Left), Some((0, 0)));
    }

    #[test]
    fn teleport_creates_workspace() {
        let mut grid = WorkspaceGrid::new();
        grid.teleport_to((2, 3));
        assert_eq!(grid.active(), (2, 3));
        assert!(grid.get((2, 3)).is_some());
    }
}
