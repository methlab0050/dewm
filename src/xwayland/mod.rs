//! Xwayland integration.
//!
//! Spawns an embedded Xwayland process and bridges X11 window lifecycle events
//! into dewm's workspace grid.  X11 windows (`X11Surface`) are treated
//! identically to Wayland toplevels for workspace placement, overview
//! thumbnails, and gesture navigation.

use std::os::unix::io::OwnedFd;
use smithay::{
    desktop::Window,
    utils::{Logical, Point, Rectangle},
    wayland::selection::{
        data_device, primary_selection, SelectionTarget,
    },
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
    /// ready.  If Xwayland is not installed or fails to launch, a warning is
    /// logged and Wayland execution continues uninterrupted.
    pub fn start_xwayland(&mut self) -> anyhow::Result<()> {
        let (xwayland, client) = match XWayland::spawn(
            &self.display_handle,
            None,
            std::iter::empty::<(String, String)>(),
            std::iter::empty::<String>(),
            true,
            std::process::Stdio::null(),
            std::process::Stdio::null(),
            |_| (),
        ) {
            Ok(res) => res,
            Err(e) => {
                tracing::warn!("Xwayland could not be spawned (is Xwayland installed?): {e}");
                return Ok(());
            }
        };

        let display_handle = self.display_handle.clone();
        let loop_handle = self.loop_handle.clone();

        self.loop_handle
            .insert_source(xwayland, move |event, _, state: &mut DewmState| match event {
                XWaylandEvent::Ready {
                    x11_socket,
                    display_number,
                } => {
                    tracing::info!(display_number, "Xwayland ready");
                    match X11Wm::start_wm(loop_handle.clone(), &display_handle, x11_socket, client.clone()) {
                        Ok(wm) => {
                            state.xwm = Some(wm);
                            state.xdisplay = Some(display_number);
                            std::env::set_var("DISPLAY", format!(":{}", display_number));
                            tracing::info!(display = display_number, "X11 Window Manager attached; DISPLAY set");
                        }
                        Err(e) => {
                            tracing::error!("Failed to attach X11 Window Manager: {e}");
                        }
                    }
                }
                XWaylandEvent::Error => {
                    tracing::warn!("Xwayland process crashed or exited with error");
                    state.xwm = None;
                    state.xdisplay = None;
                }
            })
            .map_err(|e| anyhow::anyhow!("Failed to register Xwayland source: {e}"))?;

        Ok(())
    }
}

// ─── X11 Window Manager handler ───────────────────────────────────────────────

impl XwmHandler for DewmState {
    fn xwm_state(&mut self, _xwm: XwmId) -> &mut X11Wm {
        self.xwm.as_mut().expect("xwm must be initialized")
    }

    fn new_window(&mut self, _xwm: XwmId, window: X11Surface) {
        tracing::debug!(title = ?window.title(), "Xwayland: new X11 window");
    }

    fn new_override_redirect_window(&mut self, _xwm: XwmId, _window: X11Surface) {
        tracing::debug!("Xwayland: override-redirect window");
    }

    fn map_window_request(&mut self, _xwm: XwmId, window: X11Surface) {
        let _ = window.set_mapped(true);
        let mapped = Window::new_x11_window(window.clone());
        let active = self.workspace_grid.active();
        let ws_origin = self.workspace_origin(active);
        let pos = ws_origin + Point::<i32, Logical>::from((100, 100));
        self.space.map_element(mapped.clone(), pos, true);
        self.workspace_grid.active_workspace_mut().add_window(mapped.clone());
        if let Some(bbox) = self.space.element_bbox(&mapped) {
            let _ = window.configure(Some(bbox));
        }
        tracing::info!("Xwayland: mapped X11 window to workspace {:?}", active);
    }

    fn mapped_override_redirect_window(&mut self, _xwm: XwmId, window: X11Surface) {
        let location = window.last_configure().loc;
        let window = Window::new_x11_window(window);
        self.space.map_element(window, location, true);
    }

    fn unmapped_window(&mut self, _xwm: XwmId, window: X11Surface) {
        if let Some(surface) = window.wl_surface() {
            self.workspace_grid.remove_window_by_surface(&surface);
        }
        let elem = self
            .space
            .elements()
            .find(|e| matches!(e.x11_surface(), Some(w) if w == &window))
            .cloned();
        if let Some(elem) = elem {
            self.space.unmap_elem(&elem);
        }
        if !window.is_override_redirect() {
            let _ = window.set_mapped(false);
        }
        tracing::debug!("Xwayland: unmapped X11 window");
    }

    fn destroyed_window(&mut self, _xwm: XwmId, window: X11Surface) {
        if let Some(surface) = window.wl_surface() {
            self.workspace_grid.remove_window_by_surface(&surface);
        }
        let elem = self
            .space
            .elements()
            .find(|e| matches!(e.x11_surface(), Some(w) if w == &window))
            .cloned();
        if let Some(elem) = elem {
            self.space.unmap_elem(&elem);
        }
    }

    fn configure_request(
        &mut self,
        _xwm: XwmId,
        window: X11Surface,
        _x: Option<i32>,
        _y: Option<i32>,
        w: Option<u32>,
        h: Option<u32>,
        _reorder: Option<Reorder>,
    ) {
        let mut geo = window.last_configure();
        if let Some(w) = w {
            geo.size.w = w as i32;
        }
        if let Some(h) = h {
            geo.size.h = h as i32;
        }
        let _ = window.configure(geo);
    }

    fn configure_notify(
        &mut self,
        _xwm: XwmId,
        window: X11Surface,
        geometry: Rectangle<i32, Logical>,
        _above: Option<smithay::reexports::x11rb::protocol::xproto::Window>,
    ) {
        let elem = self
            .space
            .elements()
            .find(|e| matches!(e.x11_surface(), Some(w) if w == &window))
            .cloned();
        if let Some(elem) = elem {
            self.space.map_element(elem, geometry.loc, false);
        }
    }

    fn maximize_request(&mut self, _xwm: XwmId, window: X11Surface) {
        let elem = self
            .space
            .elements()
            .find(|e| matches!(e.x11_surface(), Some(w) if w == &window))
            .cloned();
        if let Some(elem) = elem {
            let output = self.space.outputs().next().cloned();
            if let Some(output) = output {
                if let Some(geo) = self.space.output_geometry(&output) {
                    let _ = window.set_maximized(true);
                    let _ = window.configure(geo);
                    self.space.map_element(elem, geo.loc, false);
                }
            }
        }
    }

    fn unmaximize_request(&mut self, _xwm: XwmId, window: X11Surface) {
        let _ = window.set_maximized(false);
    }

    fn fullscreen_request(&mut self, _xwm: XwmId, window: X11Surface) {
        let elem = self
            .space
            .elements()
            .find(|e| matches!(e.x11_surface(), Some(w) if w == &window))
            .cloned();
        if let Some(elem) = elem {
            let output = self.space.outputs().next().cloned();
            if let Some(output) = output {
                if let Some(geo) = self.space.output_geometry(&output) {
                    let _ = window.set_fullscreen(true);
                    let _ = window.configure(geo);
                    self.space.map_element(elem, geo.loc, false);
                }
            }
        }
    }

    fn unfullscreen_request(&mut self, _xwm: XwmId, window: X11Surface) {
        let _ = window.set_fullscreen(false);
    }

    fn resize_request(&mut self, _xwm: XwmId, _window: X11Surface, _button: u32, _resize_edge: X11ResizeEdge) {}

    fn move_request(&mut self, _xwm: XwmId, _window: X11Surface, _button: u32) {}

    fn send_selection(
        &mut self,
        _xwm: XwmId,
        selection: SelectionTarget,
        mime_type: String,
        fd: OwnedFd,
    ) {
        match selection {
            SelectionTarget::Clipboard => {
                if let Err(err) = data_device::request_data_device_client_selection(&self.seat, mime_type, fd) {
                    tracing::error!(?err, "Failed to request wayland clipboard for Xwayland");
                }
            }
            SelectionTarget::Primary => {
                if let Err(err) = primary_selection::request_primary_client_selection(&self.seat, mime_type, fd) {
                    tracing::error!(?err, "Failed to request wayland primary selection for Xwayland");
                }
            }
        }
    }

    fn new_selection(
        &mut self,
        _xwm: XwmId,
        selection: SelectionTarget,
        mime_types: Vec<String>,
    ) {
        match selection {
            SelectionTarget::Clipboard => {
                data_device::set_data_device_selection(&self.display_handle, &self.seat, mime_types, ())
            }
            SelectionTarget::Primary => {
                primary_selection::set_primary_selection(&self.display_handle, &self.seat, mime_types, ())
            }
        }
    }

    fn cleared_selection(&mut self, _xwm: XwmId, selection: SelectionTarget) {
        match selection {
            SelectionTarget::Clipboard => {
                if data_device::current_data_device_selection_userdata(&self.seat).is_some() {
                    data_device::clear_data_device_selection(&self.display_handle, &self.seat)
                }
            }
            SelectionTarget::Primary => {
                if primary_selection::current_primary_selection_userdata(&self.seat).is_some() {
                    primary_selection::clear_primary_selection(&self.display_handle, &self.seat)
                }
            }
        }
    }

    fn disconnected(&mut self, _xwm: XwmId) {
        tracing::info!("Xwayland disconnected");
        self.xwm = None;
        self.xdisplay = None;
    }
}
