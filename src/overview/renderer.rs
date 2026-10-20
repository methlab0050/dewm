//! Overview renderer.
//!
//! Produces Smithay `RenderElement`s that display all workspaces as
//! live-scaled thumbnails during overview mode.

use smithay::{
    backend::renderer::{
        element::{
            solid::SolidColorRenderElement,
            utils::{ConstrainAlign, ConstrainScaleBehavior},
            Id, Kind,
        },
        utils::CommitCounter,
        ImportAll, ImportMem, Renderer,
    },
    desktop::space::{constrain_space_element, ConstrainBehavior, ConstrainReference},
    output::Output,
    utils::{Logical, Point, Rectangle, Scale, Size},
};

use crate::{
    rendering::elements::DewmRenderElement,
    state::DewmState,
};

/// Layout information for a workspace cell in the overview.
#[derive(Debug, Clone, Copy)]
pub struct CellLayout {
    pub coord: (i32, i32),
    pub rect: Rectangle<i32, Logical>,
    pub scale: f32,
}

// ─── Catppuccin Mocha Color Palette ──────────────────────────────────────────
const COLOR_MAUVE: [f32; 4]    = [0.796, 0.651, 0.969, 1.0];  // #cba6f7 (active workspace)
const COLOR_BLUE: [f32; 4]     = [0.537, 0.706, 0.980, 1.0];  // #89b4fa (drop target)
const COLOR_BORDER: [f32; 4]   = [0.345, 0.357, 0.439, 0.6];  // #585b70 (inactive border)

/// Calculate the layout for all workspaces in the overview grid.
/// Workspaces maintain a consistent 0.5x scale and are arranged on a 2D map
/// centered around the overview camera.
pub fn calculate_overview_layout(
    grid: &crate::workspace::grid::WorkspaceGrid,
    active_cell: (i32, i32),
    output_size: (i32, i32),
    camera_pos: Point<f32, Logical>,
    animation_progress: f32,
) -> Vec<CellLayout> {
    let mut layouts = Vec::new();
    let (out_w, out_h) = output_size;
    let t = animation_progress.clamp(0.0, 1.0);

    // Current uniform scale: lerps from 1.0 down to OVERVIEW_SCALE (0.5)
    let current_scale = 1.0 - (1.0 - crate::overview::state::OVERVIEW_SCALE) * t;
    let card_w = (out_w as f32) * current_scale;
    let card_h = (out_h as f32) * current_scale;

    let screen_center_x = (out_w as f32) / 2.0;
    let screen_center_y = (out_h as f32) / 2.0;

    for &coord in grid.workspaces().keys() {
        let cell_overview_center =
            crate::overview::state::OverviewState::cell_center(coord, output_size);

        // Position on screen in full overview (t = 1.0)
        let overview_screen_x = screen_center_x + (cell_overview_center.x - camera_pos.x);
        let overview_screen_y = screen_center_y + (cell_overview_center.y - camera_pos.y);

        // Position on screen in full desktop mode (t = 0.0) relative to active_cell
        let desktop_screen_x = screen_center_x + (coord.1 - active_cell.1) as f32 * out_w as f32;
        let desktop_screen_y = screen_center_y + (coord.0 - active_cell.0) as f32 * out_h as f32;

        // Interpolate between desktop position and overview position
        let on_screen_x = desktop_screen_x + (overview_screen_x - desktop_screen_x) * t;
        let on_screen_y = desktop_screen_y + (overview_screen_y - desktop_screen_y) * t;

        let rect = Rectangle::new(
            Point::from((
                (on_screen_x - card_w / 2.0).round() as i32,
                (on_screen_y - card_h / 2.0).round() as i32,
            )),
            Size::from((card_w.round() as i32, card_h.round() as i32)),
        );

        layouts.push(CellLayout {
            coord,
            rect,
            scale: current_scale,
        });
    }

    layouts
}

/// Helper to produce a solid color element.
fn solid_color_element(
    rect: Rectangle<i32, Logical>,
    color: [f32; 4],
    scale: Scale<f64>,
) -> SolidColorRenderElement {
    let phys_rect = rect.to_physical_precise_round(scale);
    SolidColorRenderElement::new(
        Id::new(),
        phys_rect,
        CommitCounter::default(),
        color,
        Kind::Unspecified,
    )
}

/// Helper to produce border elements around a rectangle.
fn border_elements(
    rect: Rectangle<i32, Logical>,
    thickness: i32,
    color: [f32; 4],
    scale: Scale<f64>,
) -> Vec<SolidColorRenderElement> {
    let t = thickness;
    let (x, y) = (rect.loc.x, rect.loc.y);
    let (w, h) = (rect.size.w, rect.size.h);

    vec![
        // Top
        solid_color_element(Rectangle::new((x, y).into(), (w, t).into()), color, scale),
        // Bottom
        solid_color_element(Rectangle::new((x, y + h - t).into(), (w, t).into()), color, scale),
        // Left
        solid_color_element(Rectangle::new((x, y + t).into(), (t, (h - 2 * t).max(0)).into()), color, scale),
        // Right
        solid_color_element(Rectangle::new((x + w - t, y + t).into(), (t, (h - 2 * t).max(0)).into()), color, scale),
    ]
}

/// Render all overview elements (workspace cards, borders, window previews, drag preview).
pub fn overview_render_elements<R>(
    renderer: &mut R,
    state: &DewmState,
    output: &Output,
) -> Vec<DewmRenderElement<R, smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement<R>>>
where
    R: Renderer + ImportAll + ImportMem + 'static,
    R::TextureId: Clone + 'static,
{
    let mut elements = Vec::new();

    let Some(output_geo) = state.space.output_geometry(output) else {
        return elements;
    };
    let output_size = (output_geo.size.w, output_geo.size.h);
    let output_scale = output.current_scale().fractional_scale();
    let scale: Scale<f64> = output_scale.into();

    let active_cell = state.workspace_grid.active();
    let layouts = calculate_overview_layout(
        &state.workspace_grid,
        active_cell,
        output_size,
        state.overview_state.camera_pos,
        state.overview_state.animation_progress,
    );
    let hovered_cell = state.overview_state.hovered_cell;
    let is_dragging = state.overview_dragger.is_dragging();
    let dragged_window = if is_dragging {
        state.overview_dragger.active.as_ref().map(|d| &d.window)
    } else {
        None
    };

    let progress = state.overview_state.animation_progress.clamp(0.0, 1.0);
    let screen_rect = Rectangle::from_size(output_size.into());

    // 1. Render workspace cell borders
    for layout in &layouts {
        // Frustum culling: skip workspaces outside the viewport
        if !screen_rect.overlaps(layout.rect) {
            continue;
        }

        let is_active = layout.coord == active_cell;
        let is_hovered = Some(layout.coord) == hovered_cell;

        // Border: highlight blue only when actively dragging over drop target; active workspace gets mauve
        let (mut border_color, border_thickness) = if is_dragging && is_hovered {
            (COLOR_BLUE, 4)
        } else if is_active {
            (COLOR_MAUVE, 3)
        } else {
            (COLOR_BORDER, 1)
        };
        border_color[3] *= progress;

        for elem in border_elements(layout.rect, border_thickness, border_color, scale) {
            elements.push(DewmRenderElement::SolidColor(elem));
        }

        // If workspace is empty, render a subtle empty indicator tile in center
        let ws_is_occupied = state
            .workspace_grid
            .get(layout.coord)
            .map(|w| w.is_occupied())
            .unwrap_or(false);

        if !ws_is_occupied {
            let ind_w = ((48.0 * layout.scale) as i32).max(16);
            let ind_h = ((48.0 * layout.scale) as i32).max(16);
            let ind_x = layout.rect.loc.x + (layout.rect.size.w - ind_w) / 2;
            let ind_y = layout.rect.loc.y + (layout.rect.size.h - ind_h) / 2;
            let ind_rect = Rectangle::new((ind_x, ind_y).into(), (ind_w, ind_h).into());
            let ind_elem = solid_color_element(ind_rect, [0.345, 0.357, 0.439, 0.4 * progress], scale);
            elements.push(DewmRenderElement::SolidColor(ind_elem));
        }
    }

    // 2. Render window thumbnails inside each workspace cell
    let constrain_behavior = ConstrainBehavior {
        reference: ConstrainReference::BoundingBox,
        behavior: ConstrainScaleBehavior::Fit,
        align: ConstrainAlign::CENTER,
    };

    for layout in &layouts {
        // Frustum culling: skip window thumbnails for workspaces outside the viewport
        if !screen_rect.overlaps(layout.rect) {
            continue;
        }

        let Some(ws) = state.workspace_grid.get(layout.coord) else {
            continue;
        };
        let ws_origin = state.workspace_origin(layout.coord);

        for window in ws.windows() {
            // If this window is currently being dragged, don't draw it in its static slot
            if is_dragging && dragged_window == Some(window) {
                continue;
            }

            let win_loc = state.space.element_location(window).unwrap_or(ws_origin);
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

            let win_elements = constrain_space_element(
                renderer,
                window,
                thumb_rect.loc,
                progress,
                scale,
                thumb_rect,
                constrain_behavior,
            );

            for elem in win_elements {
                elements.push(DewmRenderElement::Preview(elem));
            }
        }
    }

    // 3. Render dragged window floating under cursor (only when dragging past threshold)
    if is_dragging {
        if let Some(drag) = &state.overview_dragger.active {
            let (cx, cy) = drag.cursor_pos;
            let win_geo = drag.window.geometry();
            let thumb_scale = crate::overview::state::OVERVIEW_SCALE;

            let drag_w = ((win_geo.size.w as f32 * thumb_scale) as i32).max(1);
            let drag_h = ((win_geo.size.h as f32 * thumb_scale) as i32).max(1);
            let drag_x = cx as i32 - drag_w / 2;
            let drag_y = cy as i32 - drag_h / 2;

            let drag_rect = Rectangle::new(
                Point::from((drag_x, drag_y)),
                Size::from((drag_w, drag_h)),
            );

            // Highlight border around floating dragged thumbnail
            for elem in border_elements(drag_rect, 2, COLOR_BLUE, scale) {
                elements.push(DewmRenderElement::SolidColor(elem));
            }

            let drag_elements = constrain_space_element(
                renderer,
                &drag.window,
                drag_rect.loc,
                0.9,
                scale,
                drag_rect,
                constrain_behavior,
            );

            for elem in drag_elements {
                elements.push(DewmRenderElement::Preview(elem));
            }
        }
    }

    elements
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::grid::WorkspaceGrid;

    #[test]
    fn layout_single_cell_centered() {
        let grid = WorkspaceGrid::new();
        let camera_pos = Point::from((0.0, 0.0));
        let layouts = calculate_overview_layout(&grid, (0, 0), (1920, 1080), camera_pos, 1.0);
        assert_eq!(layouts.len(), 1);

        let cell = &layouts[0];
        assert_eq!(cell.coord, (0, 0));
        // Half scale: 960 x 540, centered at (480, 270)
        assert_eq!(cell.rect.size.w, 960);
        assert_eq!(cell.rect.size.h, 540);
        assert_eq!(cell.rect.loc.x, 480);
        assert_eq!(cell.rect.loc.y, 270);
        assert!((cell.scale - 0.5).abs() < 1e-4);
    }

    #[test]
    fn layout_multi_cell_signed_grid() {
        let mut grid = WorkspaceGrid::new();
        grid.ensure_exists((0, 0));
        grid.ensure_exists((-1, 0));

        let camera_pos = Point::from((0.0, 0.0));
        let layouts = calculate_overview_layout(&grid, (0, 0), (1920, 1080), camera_pos, 1.0);
        assert_eq!(layouts.len(), 2);

        let c_origin = layouts.iter().find(|l| l.coord == (0, 0)).unwrap();
        let c_up = layouts.iter().find(|l| l.coord == (-1, 0)).unwrap();

        assert_eq!(c_origin.rect.size.w, 960);
        assert_eq!(c_up.rect.size.w, 960);
        // c_up is positioned above c_origin
        assert!(c_up.rect.loc.y < c_origin.rect.loc.y);
    }

    #[test]
    fn hit_test_cell_accuracy() {
        let mut grid = WorkspaceGrid::new();
        grid.ensure_exists((0, 0));
        grid.ensure_exists((0, 1));

        let output_size = (1920, 1080);
        let camera_pos = Point::from((0.0, 0.0));
        let layouts = calculate_overview_layout(&grid, (0, 0), output_size, camera_pos, 1.0);

        for layout in layouts {
            let center_x = layout.rect.loc.x as f32 + layout.rect.size.w as f32 / 2.0;
            let center_y = layout.rect.loc.y as f32 + layout.rect.size.h as f32 / 2.0;
            let hit = crate::overview::drag::hit_test_cell(
                (center_x, center_y),
                &grid,
                (0, 0),
                output_size,
                camera_pos,
                1.0,
            );
            assert_eq!(hit, Some(layout.coord));
        }

        // Test outside bounds (e.g. at (0.0, 0.0) which is margin outside the cards)
        let hit_outside = crate::overview::drag::hit_test_cell(
            (0.0, 0.0),
            &grid,
            (0, 0),
            output_size,
            camera_pos,
            1.0,
        );
        assert_eq!(hit_outside, None);
    }

    #[test]
    fn layout_zoom_interpolation_bounds() {
        let mut grid = WorkspaceGrid::new();
        grid.ensure_exists((0, 0));
        grid.ensure_exists((0, 1));

        let camera_pos = Point::from((0.0, 0.0));
        // At t = 0.0 (full desktop mode focused on (0, 0)):
        let layouts_t0 = calculate_overview_layout(&grid, (0, 0), (1920, 1080), camera_pos, 0.0);
        let cell_0_t0 = layouts_t0.iter().find(|l| l.coord == (0, 0)).unwrap();
        let cell_1_t0 = layouts_t0.iter().find(|l| l.coord == (0, 1)).unwrap();

        // (0, 0) is full screen at (0, 0, 1920, 1080)
        assert_eq!(cell_0_t0.rect.loc.x, 0);
        assert_eq!(cell_0_t0.rect.loc.y, 0);
        assert_eq!(cell_0_t0.rect.size.w, 1920);
        assert_eq!(cell_0_t0.rect.size.h, 1080);
        assert!((cell_0_t0.scale - 1.0).abs() < 1e-4);

        // (0, 1) is exactly one screen to the right: loc.x = 1920
        assert_eq!(cell_1_t0.rect.loc.x, 1920);
        assert_eq!(cell_1_t0.rect.loc.y, 0);
        assert_eq!(cell_1_t0.rect.size.w, 1920);

        // At t = 1.0 (overview mode):
        let layouts_t1 = calculate_overview_layout(&grid, (0, 0), (1920, 1080), camera_pos, 1.0);
        let cell_0_t1 = layouts_t1.iter().find(|l| l.coord == (0, 0)).unwrap();
        let cell_1_t1 = layouts_t1.iter().find(|l| l.coord == (0, 1)).unwrap();

        // (0, 0) is 0.5x centered at (480, 270)
        assert_eq!(cell_0_t1.rect.loc.x, 480);
        assert_eq!(cell_0_t1.rect.loc.y, 270);
        assert_eq!(cell_0_t1.rect.size.w, 960);
        assert!((cell_0_t1.scale - 0.5).abs() < 1e-4);

        // (0, 1) is card_w + gap = 960 + 32 = 992 pixels to the right
        assert_eq!(cell_1_t1.rect.loc.x, 480 + 992);
        assert_eq!(cell_1_t1.rect.size.w, 960);
    }
}

