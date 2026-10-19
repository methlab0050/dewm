//! Window drag logic for the overview mode.
//!
//! When the user presses a mouse button over a window thumbnail in the
//! overview, a drag begins.  The window floats under the cursor until the
//! button is released.  On release, the window is moved to whichever
//! workspace cell the cursor is over.
//!
//! Dropping onto a non-existent cell is allowed and *creates* that workspace
//! (this is an intentional exception to the expansion rule — the user is
//! explicitly placing a window there, which necessarily makes it occupied).

use smithay::desktop::Window;

use crate::{
    overview::state::OverviewState,
    workspace::grid::WorkspaceGrid,
};

/// Active drag operation inside the overview.
#[derive(Debug, Clone)]
pub struct DragState {
    /// The window being dragged.
    pub window: Window,
    /// The workspace cell the window was originally in.
    pub source_cell: (i32, i32),
    /// Current cursor position in output-space pixels.
    pub cursor_pos: (f32, f32),
}

/// Owns the (optional) current overview drag operation.
#[derive(Default)]
pub struct OverviewDragger {
    pub active: Option<DragState>,
}

impl OverviewDragger {
    pub fn new() -> Self { Self::default() }

    // ── Pointer events (forwarded from compositor pointer handler) ────────────

    /// Attempt to start a drag from a pointer-button-down event at `cursor_pos`.
    ///
    /// Checks which workspace cell and which window thumbnail the cursor is
    /// over.  If a window is found, begins the drag.
    pub fn try_begin(
        &mut self,
        cursor_pos: (f32, f32),
        grid: &WorkspaceGrid,
        overview: &OverviewState,
        output_size: (u32, u32),
    ) -> bool {
        let Some(cell) = hit_test_cell(cursor_pos, grid, overview, output_size) else {
            return false;
        };
        let Some(ws) = grid.get(cell) else { return false };

        // Find which window thumbnail was clicked.
        // TODO: compute per-window thumbnail bounds; for now grab the first window.
        if let Some(window) = ws.windows().next() {
            self.active = Some(DragState {
                window:      window.clone(),
                source_cell: cell,
                cursor_pos,
            });
            tracing::debug!(?cell, "Overview drag begin");
            true
        } else {
            false
        }
    }

    /// Update drag cursor position.
    pub fn update(&mut self, cursor_pos: (f32, f32)) {
        if let Some(drag) = &mut self.active {
            drag.cursor_pos = cursor_pos;
        }
    }

    /// Finish a drag on pointer-button-up.  Moves the window to the target
    /// cell (creating it if necessary) or cancels back to the source cell.
    pub fn finish(
        &mut self,
        cursor_pos: (f32, f32),
        grid: &mut WorkspaceGrid,
        overview: &OverviewState,
        output_size: (u32, u32),
    ) {
        let Some(drag) = self.active.take() else { return };

        let target = hit_test_cell(cursor_pos, grid, overview, output_size)
            .unwrap_or(drag.source_cell); // cancel → return to source

        if target != drag.source_cell {
            tracing::info!(from = ?drag.source_cell, to = ?target, "Overview drag: move window");
            grid.move_window_to(&drag.window, target);
        } else {
            tracing::debug!("Overview drag cancelled, window returned to source");
        }
    }

    /// Cancel an active drag (e.g. focus stolen, Escape pressed).
    pub fn cancel(&mut self) {
        if self.active.take().is_some() {
            tracing::debug!("Overview drag cancelled");
        }
    }

    pub fn is_active(&self) -> bool { self.active.is_some() }
}

// ─── Hit testing ─────────────────────────────────────────────────────────────

/// Return the `(row, col)` of the workspace thumbnail under `cursor_pos`, or
/// `None` if the cursor is outside the thumbnail area.
fn hit_test_cell(
    (px, py): (f32, f32),
    grid: &WorkspaceGrid,
    _overview: &OverviewState,
    (ow, oh): (u32, u32),
) -> Option<(i32, i32)> {
    let (min_r, max_r, min_c, max_c) = grid.bounds();
    let rows = (max_r - min_r + 1).max(1);
    let cols = (max_c - min_c + 1).max(1);

    let cell_w = ow as f32 / cols as f32;
    let cell_h = oh as f32 / rows as f32;

    let c_idx = (px / cell_w) as i32;
    let r_idx = (py / cell_h) as i32;

    if r_idx < rows && c_idx < cols {
        Some((min_r + r_idx, min_c + c_idx))
    } else {
        None
    }
}
