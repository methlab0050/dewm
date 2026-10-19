//! Overview renderer.
//!
//! Produces Smithay `RenderElement`s that display all workspaces as
//! live-scaled thumbnails during overview mode.
//!
//! # Layout
//!
//! Each workspace cell gets an equal slice of the output.  Windows within a
//! cell are scaled and translated so they appear at their proportionally
//! correct positions inside the thumbnail.
//!
//! The `animation_progress` from [`OverviewState`] is used to lerp each
//! window's transform between its full-screen position and the thumbnail slot.

use smithay::{
    desktop::Window,
    utils::{Logical, Physical, Point, Rectangle},
};

use crate::overview::state::OverviewState;

/// Compute the rendering transform for a window in overview mode.
///
/// Returns `(position, scale)` where `position` is in output-space pixels and
/// `scale` is the uniform scale factor to apply to the window surface.
///
/// The result is a linear interpolation:
/// - At `t = 0.0`: window at its normal desktop position at scale 1.0
/// - At `t = 1.0`: window at its thumbnail slot scaled to fit the cell
pub fn overview_window_transform(
    _window: &Window,
    window_pos: Point<i32, Logical>,
    cell_row: usize,
    cell_col: usize,
    rows: usize,
    cols: usize,
    output_w: u32,
    output_h: u32,
    t: f32, // animation_progress 0.0..=1.0
) -> (Point<f32, Physical>, f32) {
    let thumb_scale = OverviewState::thumbnail_scale(rows, cols);

    let (cell_x, cell_y) =
        OverviewState::cell_position(cell_row, cell_col, rows, cols, output_w, output_h);

    // Target: thumbnail slot top-left (window scaled to fit cell).
    let target_x = cell_x + window_pos.x as f32 * thumb_scale;
    let target_y = cell_y + window_pos.y as f32 * thumb_scale;

    // Source: actual desktop position.
    let src_x = window_pos.x as f32;
    let src_y = window_pos.y as f32;

    let lerp = |a: f32, b: f32| a + (b - a) * t;

    let pos_x = lerp(src_x, target_x);
    let pos_y = lerp(src_y, target_y);
    let scale  = lerp(1.0, thumb_scale);

    (Point::from((pos_x, pos_y)), scale)
}

/// Produce a hover highlight rectangle for `cell` when it is hovered in the
/// overview.  Returns the cell's on-screen bounds.
pub fn cell_highlight_rect(
    row: usize,
    col: usize,
    rows: usize,
    cols: usize,
    output_w: u32,
    output_h: u32,
) -> Rectangle<i32, Logical> {
    let cell_w = (output_w as f32 / cols as f32) as i32;
    let cell_h = (output_h as f32 / rows as f32) as i32;
    Rectangle::new(
        (col as i32 * cell_w, row as i32 * cell_h).into(),
        (cell_w, cell_h).into(),
    )
}
