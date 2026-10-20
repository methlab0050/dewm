//! Xwayland integration.
//!
//! Spawns an embedded Xwayland process and bridges X11 window lifecycle events
//! into dewm's workspace grid.  X11 windows (`X11Surface`) are treated
//! identically to Wayland toplevels for workspace placement, overview
//! thumbnails, and gesture navigation.

use smithay::{
    desktop::Window,
    utils::{Logical, Point},
    xwayland::{
        xwm::{Reorder, ResizeEdge as X11ResizeEdge, XwmHandler, XwmId},
        X11Surface, X11Wm, XWayland, XWaylandEvent,
    },
};

use crate::state::DewmState;

impl DewmState {
    /// Initialise the Xwayland process and register it with the event loop.
    ///
    /// Call this once during compositor startup after the Wayland display is
    /// ready.  The returned `XWayland` handle must be kept alive.
    #[allow(dead_code)]
    pub fn start_xwayland(
        &mut self,
        handle: &smithay::reexports::calloop::LoopHandle<'_, DewmState>,
    ) -> anyhow::Result<()> {
        let (xwayland, _client) = XWayland::spawn(
            &self.display_handle,
            None,
            std::iter::empty::<(String, String)>(),
            std::iter::empty::<String>(),
            true,
            std::process::Stdio::null(),
            std::process::Stdio::null(),
            |_| (),
        )
        .map_err(|e| anyhow::anyhow!("Failed to spawn Xwayland: {e}"))?;

        handle
            .insert_source(xwayland, move |event, _, _state| match event {
                XWaylandEvent::Ready {
                    x11_socket: _,
                    display_number,
                } => {
                    tracing::info!(display_number, "Xwayland ready");
                    // TODO: initialise XWm here once we have a way to store it in state
                }
                XWaylandEvent::Error => {
                    tracing::warn!("Xwayland exited with error");
                }
            })
            .map_err(|e| anyhow::anyhow!("Failed to register Xwayland source: {e}"))?;

        Ok(())
    }
}

// ─── X11 Window Manager handler ───────────────────────────────────────────────

impl XwmHandler for DewmState {
    fn xwm_state(&mut self, _xwm: XwmId) -> &mut X11Wm {
        // TODO: store X11Wm in DewmState and return reference.
        unimplemented!("Store X11Wm in DewmState")
    }

    fn new_window(&mut self, _xwm: XwmId, window: X11Surface) {
        tracing::debug!(title = ?window.title(), "Xwayland: new X11 window");
        // X11 windows start unmapped; we wait for map_window_request to place them.
    }

    fn new_override_redirect_window(&mut self, _xwm: XwmId, _window: X11Surface) {
        // Override-redirect windows (menus, tooltips) don't go through map_window_request.
        // TODO: display them as floating popups.
        tracing::debug!("Xwayland: override-redirect window");
    }

    fn map_window_request(&mut self, _xwm: XwmId, window: X11Surface) {
        window.set_mapped(true).ok();
        let mapped = Window::new_x11_window(window);
        let active = self.workspace_grid.active();
        let ws_origin = self.workspace_origin(active);
        let pos    = ws_origin + Point::<i32, Logical>::from((100, 100));
        self.space.map_element(mapped.clone(), pos, true);
        self.workspace_grid.active_workspace_mut().add_window(mapped);
        tracing::info!("Xwayland: mapped X11 window to workspace {:?}", active);
    }

    fn mapped_override_redirect_window(&mut self, _xwm: XwmId, _window: X11Surface) {}

    fn unmapped_window(&mut self, _xwm: XwmId, window: X11Surface) {
        let surface = window.wl_surface().expect("X11Surface has wl_surface");
        self.workspace_grid.remove_window_by_surface(&surface);
        tracing::debug!("Xwayland: unmapped X11 window");
    }

    fn destroyed_window(&mut self, _xwm: XwmId, window: X11Surface) {
        if let Some(surface) = window.wl_surface() {
            self.workspace_grid.remove_window_by_surface(&surface);
        }
    }

    fn configure_request(&mut self, _xwm: XwmId, window: X11Surface, _x: Option<i32>, _y: Option<i32>, w: Option<u32>, h: Option<u32>, _reorder: Option<Reorder>) {
        // Honour resize requests; ignore position (we manage placement).
        let mut geo = window.geometry();
        if let Some(w) = w { geo.size.w = w as i32; }
        if let Some(h) = h { geo.size.h = h as i32; }
        window.configure(geo).ok();
    }

    fn configure_notify(&mut self, _xwm: XwmId, _window: X11Surface, _geometry: smithay::utils::Rectangle<i32, Logical>, _above: Option<smithay::reexports::x11rb::protocol::xproto::Window>) {}

    fn resize_request(&mut self, _xwm: XwmId, _window: X11Surface, _button: u32, _resize_edge: X11ResizeEdge) {}

    fn move_request(&mut self, _xwm: XwmId, _window: X11Surface, _button: u32) {}
}
