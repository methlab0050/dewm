//! Global compositor state.
//!
//! [`DewmState`] is the single struct that owns all compositor sub-systems and
//! implements every Smithay handler trait required to run a Wayland compositor.
//! Smithay's `delegate_*!` macros wire those trait impls to the Wayland protocol
//! dispatch machinery automatically.

use smithay::{
    backend::renderer::utils::on_commit_buffer_handler,
    delegate_dispatch2,
    desktop::{
        find_popup_root_surface, get_popup_toplevel_coords, PopupKind, PopupManager, Space, Window,
        WindowSurfaceType,
    },
    input::{
        pointer::{CursorImageStatus, Focus},
        Seat, SeatHandler, SeatState,
    },
    reexports::{
        calloop::{EventLoop, LoopSignal},
        wayland_server::{
            backend::{ClientData, ClientId, DisconnectReason},
            protocol::{wl_buffer::WlBuffer, wl_surface::WlSurface},
            Display, DisplayHandle, Resource,
        },
    },
    utils::{Logical, Point},
    wayland::{
        buffer::BufferHandler,
        compositor::{CompositorClientState, CompositorHandler, CompositorState},
        output::{OutputHandler, OutputManagerState},
        pointer_constraints::PointerConstraintsHandler,
        selection::{
            data_device::{DataDeviceHandler, DataDeviceState, WaylandDndGrabHandler},
            SelectionHandler,
        },
        shell::xdg::{
            decoration::XdgDecorationHandler, PopupSurface, PositionerState,
            ToplevelSurface, XdgShellHandler, XdgShellState,
        },
        shm::{ShmHandler, ShmState},
    },
};

use crate::{
    gesture::tracker::GestureTracker,
    input::keybindings::KeybindingConfig,
    overview::state::OverviewState,
    workspace::grid::WorkspaceGrid,
};

// ─── State struct ─────────────────────────────────────────────────────────────

pub struct DewmState {
    // ── Smithay protocol state ────────────────────────────────────────────────
    pub display_handle:       DisplayHandle,
    pub compositor_state:     CompositorState,
    pub xdg_shell_state:      XdgShellState,
    pub shm_state:            ShmState,
    pub output_manager_state: OutputManagerState,
    pub data_device_state:    DataDeviceState,
    pub seat_state:           SeatState<Self>,
    pub seat:                 Seat<Self>,
    pub popups:               PopupManager,

    /// Spatial layout of all mapped windows.
    pub space: Space<Window>,

    // ── dewm subsystems ───────────────────────────────────────────────────────
    /// 2D grid of workspaces — the core of dewm's layout model.
    pub workspace_grid: WorkspaceGrid,
    /// Tracks multi-touch swipe gesture phases and dispatches actions.
    pub gesture_tracker: GestureTracker,
    /// Overview mode animation and drag state.
    pub overview_state: OverviewState,
    /// Keybinding configuration (e.g. Super+D → Vicinae).
    pub keybindings: KeybindingConfig,

    // ── Compositor loop control ───────────────────────────────────────────────
    pub loop_signal: LoopSignal,
    pub start_time:  std::time::Instant,

    // ── Cursor ───────────────────────────────────────────────────────────────
    pub cursor_status: CursorImageStatus,
}

impl DewmState {
    /// Construct initial compositor state.
    pub fn new(
        display:     &mut Display<Self>,
        event_loop:  &EventLoop<'_, Self>,
    ) -> Self {
        let dh = display.handle();
        let clock = std::time::Instant::now();

        let compositor_state     = CompositorState::new::<Self>(&dh);
        let xdg_shell_state      = XdgShellState::new::<Self>(&dh);
        let shm_state            = ShmState::new::<Self>(&dh, vec![]);
        let output_manager_state = OutputManagerState::new_with_xdg_output::<Self>(&dh);
        let data_device_state    = DataDeviceState::new::<Self>(&dh);
        let popups               = PopupManager::default();
        let mut seat_state       = SeatState::new();
        let mut seat             = seat_state.new_wl_seat(&dh, "dewm-seat0");

        // Enable keyboard and pointer capabilities on the seat
        seat.add_keyboard(Default::default(), 200, 25).unwrap();
        seat.add_pointer();

        Self {
            display_handle: dh,
            compositor_state,
            xdg_shell_state,
            shm_state,
            output_manager_state,
            data_device_state,
            seat_state,
            seat,
            popups,
            space:           Space::default(),
            workspace_grid:  WorkspaceGrid::new(),
            gesture_tracker: GestureTracker::new(),
            overview_state:  OverviewState::new(),
            keybindings:     KeybindingConfig::default(),
            loop_signal:     event_loop.get_signal(),
            start_time:      clock,
            cursor_status:   CursorImageStatus::default_named(),
        }
    }

    /// Find the surface under the given point in Space coordinates.
    pub fn surface_under(&self, pos: Point<f64, Logical>) -> Option<(WlSurface, Point<f64, Logical>)> {
        self.space.element_under(pos).and_then(|(window, location)| {
            window
                .surface_under(pos - location.to_f64(), WindowSurfaceType::ALL)
                .map(|(s, p)| (s, (p + location).to_f64()))
        })
    }

    /// Unconstrain a popup surface within output geometry.
    pub fn unconstrain_popup(&self, popup: &PopupSurface) {
        let Ok(root) = find_popup_root_surface(&PopupKind::Xdg(popup.clone())) else {
            return;
        };
        let Some(window) = self
            .space
            .elements()
            .find(|w| w.toplevel().map(|t| t.wl_surface() == &root).unwrap_or(false))
        else {
            return;
        };

        let Some(output) = self.space.outputs().next() else { return; };
        let Some(output_geo) = self.space.output_geometry(output) else { return; };
        let Some(window_geo) = self.space.element_geometry(window) else { return; };

        let mut target = output_geo;
        target.loc -= get_popup_toplevel_coords(&PopupKind::Xdg(popup.clone()));
        target.loc -= window_geo.loc;

        popup.with_pending_state(|state| {
            state.geometry = state.positioner.get_unconstrained_geometry(target);
        });
    }
}

// ─── Smithay trait implementations ───────────────────────────────────────────

impl CompositorHandler for DewmState {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a smithay::reexports::wayland_server::Client) -> &'a CompositorClientState {
        &client.get_data::<ClientState>().unwrap().compositor_state
    }

    fn commit(&mut self, surface: &WlSurface) {
        // Forward commit to the desktop layer so damage tracking and
        // window geometry stay in sync.
        on_commit_buffer_handler::<Self>(surface);

        // Initial configure for toplevels
        if let Some(window) = self
            .space
            .elements()
            .find(|w| w.toplevel().map(|t| t.wl_surface() == surface).unwrap_or(false))
            .cloned()
        {
            let initial_configure_sent = smithay::wayland::compositor::with_states(surface, |states| {
                states
                    .data_map
                    .get::<smithay::wayland::shell::xdg::XdgToplevelSurfaceData>()
                    .unwrap()
                    .lock()
                    .unwrap()
                    .initial_configure_sent
            });

            if !initial_configure_sent {
                window.toplevel().unwrap().send_configure();
            }
        }

        // Commit popups
        self.popups.commit(surface);
        if let Some(popup) = self.popups.find_popup(surface) {
            match popup {
                PopupKind::Xdg(ref xdg) => {
                    if !xdg.is_initial_configure_sent() {
                        let _ = xdg.send_configure();
                    }
                }
                PopupKind::InputMethod(_) => {}
            }
        }

        // Handle resize geometry adjustments on commit
        crate::input::grabs::handle_resize_commit(&mut self.space, surface);
    }
}


impl XdgShellHandler for DewmState {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        let window = Window::new_wayland_window(surface);
        // Place new windows on the currently active workspace.
        let active = self.workspace_grid.active();
        let pos    = Point::<i32, Logical>::from((100, 100)); // TODO: smart placement
        self.space.map_element(window.clone(), pos, true);
        self.workspace_grid.workspaces_mut()[active.0][active.1]
            .add_window(window);
    }

    fn new_popup(&mut self, surface: PopupSurface, _positioner: PositionerState) {
        self.unconstrain_popup(&surface);
        let _ = self.popups.track_popup(PopupKind::Xdg(surface));
    }

    fn reposition_request(&mut self, surface: PopupSurface, positioner: PositionerState, token: u32) {
        surface.with_pending_state(|state| {
            let geometry = positioner.get_geometry();
            state.geometry = geometry;
            state.positioner = positioner;
        });
        self.unconstrain_popup(&surface);
        surface.send_repositioned(token);
    }

    fn move_request(
        &mut self,
        surface: ToplevelSurface,
        seat: smithay::reexports::wayland_server::protocol::wl_seat::WlSeat,
        serial: smithay::utils::Serial,
    ) {
        let seat = Seat::from_resource(&seat).unwrap();
        let wl_surface = surface.wl_surface();

        if let Some(start_data) = crate::input::grabs::check_grab(&seat, wl_surface, serial) {
            let pointer = seat.get_pointer().unwrap();

            let Some(window) = self
                .space
                .elements()
                .find(|w| w.toplevel().map(|t| t.wl_surface() == wl_surface).unwrap_or(false))
                .cloned()
            else { return; };

            let initial_window_location = self.space.element_location(&window).unwrap();

            let grab = crate::input::grabs::MoveSurfaceGrab {
                start_data,
                window,
                initial_window_location,
            };

            pointer.set_grab(self, grab, serial, Focus::Clear);
        }
    }

    fn resize_request(
        &mut self,
        surface: ToplevelSurface,
        seat: smithay::reexports::wayland_server::protocol::wl_seat::WlSeat,
        serial: smithay::utils::Serial,
        edges: smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::ResizeEdge,
    ) {
        let seat = Seat::from_resource(&seat).unwrap();
        let wl_surface = surface.wl_surface();

        if let Some(start_data) = crate::input::grabs::check_grab(&seat, wl_surface, serial) {
            let pointer = seat.get_pointer().unwrap();

            let Some(window) = self
                .space
                .elements()
                .find(|w| w.toplevel().map(|t| t.wl_surface() == wl_surface).unwrap_or(false))
                .cloned()
            else { return; };

            let initial_window_location = self.space.element_location(&window).unwrap();
            let initial_window_size = window.geometry().size;

            surface.with_pending_state(|state| {
                state.states.set(smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::State::Resizing);
            });
            surface.send_pending_configure();

            let grab = crate::input::grabs::ResizeSurfaceGrab::start(
                start_data,
                window,
                edges.into(),
                smithay::utils::Rectangle::new(initial_window_location, initial_window_size),
            );

            pointer.set_grab(self, grab, serial, Focus::Clear);
        }
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        self.workspace_grid.remove_window_by_surface(surface.wl_surface());
    }

    fn grab(&mut self, _surface: PopupSurface, _seat: smithay::reexports::wayland_server::protocol::wl_seat::WlSeat, _serial: smithay::utils::Serial) {
        // TODO: popup grabs
    }
}

impl XdgDecorationHandler for DewmState {
    fn new_decoration(&mut self, toplevel: ToplevelSurface) {
        // Tell the client to draw its own decorations (CSD).
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(
                smithay::reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode::ClientSide,
            );
        });
        toplevel.send_configure();
    }

    fn request_mode(&mut self, toplevel: ToplevelSurface, _mode: smithay::reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode) {
        use smithay::reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode;
        // Always honour CSD — never switch to SSD.
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(Mode::ClientSide);
        });
        toplevel.send_configure();
    }

    fn unset_mode(&mut self, toplevel: ToplevelSurface) {
        // Default is CSD, nothing special to do.
        toplevel.send_configure();
    }
}

impl SeatHandler for DewmState {
    type KeyboardFocus = WlSurface;
    type PointerFocus  = WlSurface;
    type TouchFocus    = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.seat_state
    }

    fn cursor_image(&mut self, _seat: &Seat<Self>, image: CursorImageStatus) {
        self.cursor_status = image;
    }

    fn focus_changed(&mut self, _seat: &Seat<Self>, _focused: Option<&WlSurface>) {
        // TODO: update window stacking order / visual focus ring
    }
}

impl PointerConstraintsHandler for DewmState {}

impl ShmHandler for DewmState {
    fn shm_state(&self) -> &ShmState { &self.shm_state }
}

impl BufferHandler for DewmState {
    fn buffer_destroyed(&mut self, _buffer: &WlBuffer) {}
}

impl SelectionHandler for DewmState {
    type SelectionUserData = ();
}

impl WaylandDndGrabHandler for DewmState {}

impl DataDeviceHandler for DewmState {
    fn data_device_state(&mut self) -> &mut DataDeviceState {
        &mut self.data_device_state
    }
}

impl OutputHandler for DewmState {}

delegate_dispatch2!(DewmState);

// ─── Per-client data ──────────────────────────────────────────────────────────

#[derive(Default)]
pub struct ClientState {
    pub compositor_state: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}
