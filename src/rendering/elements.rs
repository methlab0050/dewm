//! Custom `RenderElement` types for dewm.
//!
//! Smithay's `render_elements!` macro generates a unified enum that can hold
//! both Smithay's built-in surface elements and our custom overview elements,
//! allowing the damage tracker to reason about them uniformly.

use smithay::{
    backend::renderer::{
        element::{
            surface::WaylandSurfaceRenderElement,
            solid::SolidColorRenderElement,
            RenderElement,
        },
        ImportAll, ImportMem, Renderer,
    },
    desktop::space::SpaceRenderElements,
};

smithay::render_elements! {
    /// The complete set of render elements emitted each frame.
    pub DewmRenderElement<R> where R: ImportAll + ImportMem;

    // Standard window surface (Wayland or Xwayland).
    Space=SpaceRenderElements<R, WaylandSurfaceRenderElement<R>>,

    // Solid-colour rectangles used for the overview cell hover highlight.
    SolidColor=SolidColorRenderElement,

    // TODO: cursor element, drag-floating window element, workspace label
}
