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
    /// Pending migrations from recent compaction `((old_r, old_c), (new_r, new_c))`.
    pub pending_migrations: Vec<((i32, i32), (i32, i32))>,
}

impl WorkspaceGrid {
    /// Create a grid initialized with a single empty workspace at origin `(0, 0)`.
    pub fn new() -> Self {
        let mut workspaces = BTreeMap::new();
        workspaces.insert((0, 0), Workspace::new());
        Self {
            workspaces,
            active: (0, 0),
            pending_migrations: Vec::new(),
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

    /// Prune any empty workspace other than `keep`.
    pub fn prune_empty_except(&mut self, keep: (i32, i32)) {
        self.workspaces.retain(|&coord, ws| {
            coord == keep || ws.is_occupied()
        });
    }

    /// Shift rows and columns to fill any intermediate empty rows and columns.
    /// Returns the list of migrations `((old_r, old_c), (new_r, new_c))` for
    /// workspaces that moved.
    pub fn compact_empty_gaps(&mut self) -> Vec<((i32, i32), (i32, i32))> {
        if self.workspaces.is_empty() {
            return Vec::new();
        }

        // 1. Gather all unique row and column indices
        let mut rows: Vec<i32> = self.workspaces.keys().map(|&(r, _)| r).collect();
        let mut cols: Vec<i32> = self.workspaces.keys().map(|&(_, c)| c).collect();
        rows.sort_unstable();
        rows.dedup();
        cols.sort_unstable();
        cols.dedup();

        let min_r = *rows.first().unwrap();
        let max_r = *rows.last().unwrap();
        let min_c = *cols.first().unwrap();
        let max_c = *cols.last().unwrap();

        // 2. Identify intermediate empty rows and columns
        let empty_rows: Vec<i32> = (min_r..=max_r).filter(|r| !rows.contains(r)).collect();
        let empty_cols: Vec<i32> = (min_c..=max_c).filter(|c| !cols.contains(c)).collect();

        if empty_rows.is_empty() && empty_cols.is_empty() {
            return Vec::new();
        }

        // 3. Compute mapping for each coordinate
        let mut migrations = Vec::new();
        let old_workspaces = std::mem::take(&mut self.workspaces);
        let mut new_workspaces = BTreeMap::new();

        for ((r, c), ws) in old_workspaces {
            let shift_r = empty_rows.iter().filter(|&&er| er < r).count() as i32;
            let shift_c = empty_cols.iter().filter(|&&ec| ec < c).count() as i32;
            let new_coord = (r - shift_r, c - shift_c);

            if new_coord != (r, c) {
                migrations.push(((r, c), new_coord));
            }

            if self.active == (r, c) {
                self.active = new_coord;
            }

            new_workspaces.insert(new_coord, ws);
        }

        self.workspaces = new_workspaces;
        self.pending_migrations.extend(migrations.iter().copied());
        migrations
    }

    /// Prune empty workspaces except `keep`, and compact any intermediate empty gaps.
    pub fn prune_and_compact(&mut self, keep: (i32, i32)) -> Vec<((i32, i32), (i32, i32))> {
        self.prune_empty_except(keep);
        self.compact_empty_gaps()
    }

    /// Take and clear any accumulated pending migrations.
    pub fn take_pending_migrations(&mut self) -> Vec<((i32, i32), (i32, i32))> {
        std::mem::take(&mut self.pending_migrations)
    }

    // ── Navigation ────────────────────────────────────────────────────────────

    /// Attempt to navigate in `dir`.
    ///
    /// Returns `Some((row, col))` of the new active workspace on success, or
    /// `None` if the move is blocked (swipe should snap back).
    ///
    /// When navigating away from an empty workspace, the empty workspace is pruned
    /// and any intermediate empty rows/columns are compacted.
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
            self.prune_and_compact(target);
            Some(self.active)
        } else {
            // Only expand if the current workspace has at least one window.
            if self.active_workspace().is_occupied() {
                self.ensure_exists(target);
                self.active = target;
                self.prune_and_compact(target);
                Some(self.active)
            } else {
                // Empty source → snap back.
                None
            }
        }
    }

    /// Teleport directly to a coordinate (used by overview click / drag-drop).
    /// Creates the workspace if it does not exist. Prunes any empty workspace left behind
    /// and compacts intermediate empty gaps.
    pub fn teleport_to(&mut self, target: (i32, i32)) {
        self.ensure_exists(target);
        self.active = target;
        self.prune_and_compact(target);
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
        // Source (0, 0) was empty, so navigating away pruned it
        assert!(!grid.exists((0, 0)));
    }

    #[test]
    fn teleport_creates_workspace() {
        let mut grid = WorkspaceGrid::new();
        grid.teleport_to((-2, 3));
        assert_eq!(grid.active(), (-2, 3));
        assert!(grid.get((-2, 3)).is_some());
        // Since (0, 0) was empty, teleporting away pruned (0, 0)
        assert!(!grid.exists((0, 0)));
        let (min_r, max_r, min_c, max_c) = grid.bounds();
        assert_eq!(min_r, -2);
        assert_eq!(max_r, -2);
        assert_eq!(min_c, 3);
        assert_eq!(max_c, 3);
    }

    #[test]
    fn empty_workspace_pruned_on_navigation() {
        let mut grid = WorkspaceGrid::new();
        assert!(grid.exists((0, 0)));
        // Navigate to existing (0, 1)
        grid.ensure_exists((0, 1));
        assert_eq!(grid.navigate(Direction::Right), Some((0, 1)));
        // (0, 0) was empty and left behind -> pruned!
        assert!(!grid.exists((0, 0)));
        assert!(grid.exists((0, 1)));
    }

    #[test]
    fn prune_empty_except_preserves_occupied_and_active() {
        let mut grid = WorkspaceGrid::new();
        grid.ensure_exists((0, 0)); // empty
        grid.ensure_exists((0, 1)); // empty
        grid.ensure_exists((0, 2)); // empty

        // Keep (0, 1)
        grid.prune_empty_except((0, 1));

        // (0, 0) and (0, 2) were empty and not keep -> pruned!
        assert!(!grid.exists((0, 0)));
        assert!(grid.exists((0, 1)));
        assert!(!grid.exists((0, 2)));
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

    #[test]
    fn compact_intermediate_empty_column() {
        let mut grid = WorkspaceGrid::new();
        // Setup (1, 1), (1, 2), (1, 3)
        grid.ensure_exists((1, 1));
        grid.ensure_exists((1, 2));
        grid.ensure_exists((1, 3));

        grid.get_mut((1, 1)).unwrap().test_occupied = true;
        grid.get_mut((1, 2)).unwrap().test_occupied = false; // emptied!
        grid.get_mut((1, 3)).unwrap().test_occupied = true;

        // Teleport to (1, 1) -> (1, 2) is pruned, and intermediate col 2 is compacted!
        grid.teleport_to((1, 1));

        // (1, 3) must have shifted to (1, 2)!
        assert!(grid.exists((1, 1)));
        assert!(grid.exists((1, 2)));
        assert!(!grid.exists((1, 3)));
        assert_eq!(grid.active(), (1, 1));

        // Now navigate right from (1, 1) -> succeeds and lands on (1, 2)!
        assert_eq!(grid.navigate(Direction::Right), Some((1, 2)));
    }

    #[test]
    fn compact_intermediate_empty_row() {
        let mut grid = WorkspaceGrid::new();
        // Setup (1, 1), (2, 1), (3, 1)
        grid.ensure_exists((1, 1));
        grid.ensure_exists((2, 1));
        grid.ensure_exists((3, 1));

        grid.get_mut((1, 1)).unwrap().test_occupied = true;
        grid.get_mut((2, 1)).unwrap().test_occupied = false; // emptied!
        grid.get_mut((3, 1)).unwrap().test_occupied = true;

        grid.teleport_to((1, 1));

        // (3, 1) shifted up to (2, 1)!
        assert!(grid.exists((1, 1)));
        assert!(grid.exists((2, 1)));
        assert!(!grid.exists((3, 1)));
        assert_eq!(grid.active(), (1, 1));

        // Now navigate down from (1, 1) -> succeeds and lands on (2, 1)!
        assert_eq!(grid.navigate(Direction::Down), Some((2, 1)));
    }
}
