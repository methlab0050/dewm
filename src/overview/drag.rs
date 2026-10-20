//! Window drag logic for the overview mode.
//!
//! When the user presses a mouse button over a window thumbnail in the
//! overview, a drag begins.  The window floats under the cursor until the
//! button is released.  On release, the window is moved to whichever
//! workspace cell the cursor is over.
//!
//! Dropping onto an existing or new cell reparents the window to that workspace.

use smithay::{
    desktop::{space::Space, Window},
    utils::{Logical, Point, Rectangle, Size},
};

use crate::workspace::grid::WorkspaceGrid;

/// Minimum pixel distance the pointer must move before a click turns into a window drag.
pub const DRAG_THRESHOLD: f32 = 8.0;

/// Active drag operation inside the overview.
#[derive(Debug, Clone)]
pub struct DragState {
    /// The window being dragged.
    pub window: Window,
    /// The workspace cell the window was originally in.
    pub source_cell: (i32, i32),
    /// Initial cursor position where mouse was pressed.
    pub start_pos: (f32, f32),
    /// Current cursor position in output-space pixels.
    pub cursor_pos: (f32, f32),
    /// True once the pointer moves beyond DRAG_THRESHOLD.
    pub is_dragging: bool,
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
    /// over. If a window is found, begins tracking in pending state (tap vs drag).
    pub fn try_begin(
        &mut self,
        cursor_pos: (f32, f32),
        grid: &WorkspaceGrid,
        space: &Space<Window>,
        active_cell: (i32, i32),
        output_size: (i32, i32),
        camera_pos: Point<f32, Logical>,
        animation_progress: f32,
    ) -> bool {
        if let Some((window, cell)) = hit_test_window(
            cursor_pos,
            grid,
            space,
            active_cell,
            output_size,
            camera_pos,
            animation_progress,
        ) {
            self.active = Some(DragState {
                window,
                source_cell: cell,
                start_pos: cursor_pos,
                cursor_pos,
                is_dragging: false,
            });
            tracing::debug!(?cell, "Overview drag pending (tap vs drag)");
            true
        } else {
            false
        }
    }

    /// Update drag cursor position and check threshold.
    pub fn update(&mut self, cursor_pos: (f32, f32)) {
        if let Some(drag) = &mut self.active {
            drag.cursor_pos = cursor_pos;
            if !drag.is_dragging {
                let dx = cursor_pos.0 - drag.start_pos.0;
                let dy = cursor_pos.1 - drag.start_pos.1;
                if dx * dx + dy * dy >= DRAG_THRESHOLD * DRAG_THRESHOLD {
                    drag.is_dragging = true;
                    tracing::debug!(?drag.source_cell, "Overview drag threshold exceeded -> dragging window");
                }
            }
        }
    }

    /// Finish a drag on pointer-button-up. Returns `(window, from_cell, to_cell, is_dragging)`
    /// so the compositor can distinguish between a tap (is_dragging == false) and a move.
    pub fn finish(
        &mut self,
        cursor_pos: (f32, f32),
        grid: &WorkspaceGrid,
        active_cell: (i32, i32),
        output_size: (i32, i32),
        camera_pos: Point<f32, Logical>,
        animation_progress: f32,
    ) -> Option<(Window, (i32, i32), (i32, i32), bool)> {
        let drag = self.active.take()?;
        let target = hit_test_cell(
            cursor_pos,
            grid,
            active_cell,
            output_size,
            camera_pos,
            animation_progress,
        )
        .unwrap_or(drag.source_cell); // cancel → return to source

        Some((drag.window, drag.source_cell, target, drag.is_dragging))
    }

    /// Cancel an active drag (e.g. focus stolen, Escape pressed).
    pub fn cancel(&mut self) {
        if self.active.take().is_some() {
            tracing::debug!("Overview drag cancelled");
        }
    }

    /// Whether an active window is currently being dragged (past threshold).
    pub fn is_dragging(&self) -> bool {
        self.active.as_ref().map(|d| d.is_dragging).unwrap_or(false)
    }

    /// Whether a window is pending or being dragged.
    #[allow(dead_code)]
    pub fn is_active(&self) -> bool { self.active.is_some() }
}

// ─── Hit testing ─────────────────────────────────────────────────────────────

/// Return the `(row, col)` of the workspace thumbnail under `cursor_pos`, or
/// `None` if the cursor is outside the thumbnail area.
pub fn hit_test_cell(
    (px, py): (f32, f32),
    grid: &WorkspaceGrid,
    active_cell: (i32, i32),
    output_size: (i32, i32),
    camera_pos: Point<f32, Logical>,
    animation_progress: f32,
) -> Option<(i32, i32)> {
    let layouts = crate::overview::renderer::calculate_overview_layout(
        grid,
        active_cell,
        output_size,
        camera_pos,
        animation_progress,
    );
    let p = Point::from((px as i32, py as i32));
    layouts.into_iter().find(|l| l.rect.contains(p)).map(|l| l.coord)
}

/// Return the `(Window, cell)` of the window thumbnail under `cursor_pos`, or `None`.
pub fn hit_test_window(
    (px, py): (f32, f32),
    grid: &WorkspaceGrid,
    space: &Space<Window>,
    active_cell: (i32, i32),
    output_size: (i32, i32),
    camera_pos: Point<f32, Logical>,
    animation_progress: f32,
) -> Option<(Window, (i32, i32))> {
    let layouts = crate::overview::renderer::calculate_overview_layout(
        grid,
        active_cell,
        output_size,
        camera_pos,
        animation_progress,
    );
    let p = Point::from((px as i32, py as i32));

    for layout in layouts {
        if !layout.rect.contains(p) {
            continue;
        }
        let Some(ws) = grid.get(layout.coord) else { continue; };
        let ws_origin = Point::from((layout.coord.1 * output_size.0, layout.coord.0 * output_size.1));

        // Check windows in reverse order (topmost first)
        for window in ws.windows().rev() {
            let win_loc = space.element_location(window).unwrap_or(ws_origin);
            let rel_pos = win_loc - ws_origin;
            let win_geo = window.geometry();

            let thumb_x = layout.rect.loc.x + (rel_pos.x as f32 * layout.scale) as i32;
            let thumb_y = layout.rect.loc.y + (rel_pos.y as f32 * layout.scale) as i32;
            let thumb_w = ((win_geo.size.w as f32 * layout.scale) as i32).max(1);
            let thumb_h = ((win_geo.size.h as f32 * layout.scale) as i32).max(1);

            let thumb_rect = Rectangle::new(
                Point::from((thumb_x, thumb_y)),
                Size::from((thumb_w, thumb_h)),
            );

            if thumb_rect.contains(p) {
                return Some((window.clone(), layout.coord));
            }
        }
    }
    None
}


