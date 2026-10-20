//! Custom `RenderElement` types for dewm.
//!
//! Smithay's `render_elements!` macro generates a unified enum that can hold
//! both Smithay's built-in surface elements and our custom overview elements,
//! allowing the damage tracker to reason about them uniformly.

use smithay::{
    backend::renderer::{
        element::{
            solid::SolidColorRenderElement,
            utils::{CropRenderElement, RelocateRenderElement, RescaleRenderElement},
            Wrap,
        },
        ImportAll, ImportMem,
    },
    desktop::space::SpaceRenderElements,
};

smithay::backend::renderer::element::render_elements! {
    /// The complete set of render elements emitted each frame.
    pub DewmRenderElement<R, E> where
        R: ImportAll + ImportMem;

    // Standard space elements (Wayland and Xwayland window surfaces).
    Space=SpaceRenderElements<R, E>,

    // Solid-colour rectangles used for the overview cell hover highlight.
    SolidColor=SolidColorRenderElement,

    // Scaled window preview in overview mode
    Preview=CropRenderElement<RelocateRenderElement<RescaleRenderElement<E>>>,

    // Custom element wrap.
    Custom=Wrap<E>,
}
