//! An individual workspace that owns a set of mapped windows.

use smithay::{
    desktop::Window,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    wayland::seat::WaylandFocus,
};

/// A single cell in the 2D workspace grid.
#[derive(Default, Clone)]
pub struct Workspace {
    /// Windows currently assigned to this workspace.
    pub windows: Vec<Window>,
    #[cfg(test)]
    pub test_occupied: bool,
}

impl Workspace {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true if this workspace contains at least one window.
    pub fn is_occupied(&self) -> bool {
        #[cfg(test)]
        if self.test_occupied {
            return true;
        }
        !self.windows.is_empty()
    }

    /// Add a window to this workspace.
    pub fn add_window(&mut self, window: Window) {
        if !self.windows.contains(&window) {
            self.windows.push(window);
        }
    }

    /// Remove a window from this workspace by its Wayland surface.
    pub fn remove_by_surface(&mut self, surface: &WlSurface) -> Option<Window> {
        if let Some(pos) = self.windows.iter().position(|w| {
            w.wl_surface().as_deref() == Some(surface)
        }) {
            Some(self.windows.remove(pos))
        } else {
            None
        }
    }

    /// Remove a window directly.
    pub fn remove_window(&mut self, window: &Window) -> bool {
        if let Some(pos) = self.windows.iter().position(|w| w == window) {
            self.windows.remove(pos);
            true
        } else {
            false
        }
    }

    /// Iterate over all windows in this workspace.
    pub fn windows(&self) -> impl DoubleEndedIterator<Item = &Window> {
        self.windows.iter()
    }
}
