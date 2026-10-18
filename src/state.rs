//! Global compositor state.
//!
//! [`DewmState`] is the single struct that owns all compositor sub-systems and
//! implements every Smithay handler trait required to run a Wayland compositor.
//! Smithay's `delegate_*!` macros wire those trait impls to the Wayland protocol
//! dispatch machinery automatically.

use smithay::{
    delegate_compositor, delegate_data_device, delegate_output, delegate_seat,
    delegate_shm, delegate_xdg_decoration, delegate_xdg_shell,
    desktop::{Space, Window},
    input::{Seat, SeatHandler, SeatState, pointer::CursorImageStatus},
    reexports::{
        calloop::{EventLoop, LoopSignal},
        wayland_server::{
            backend::{ClientData, ClientId, DisconnectReason},
            Display, DisplayHandle, protocol::wl_surface::WlSurface,
        },
    },
    wayland::{
        compositor::{CompositorClientState, CompositorHandler, CompositorState},
        output::OutputManagerState,
        selection::data_device::{
            ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
        },
        shell::xdg::{
            decoration::XdgDecorationHandler, XdgShellHandler, XdgShellState,
            ToplevelSurface,
        },
        shm::{ShmHandler, ShmState},
    },
    utils::{Logical, Point},
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
        let mut seat_state       = SeatState::new();
        let seat                 = seat_state.new_wl_seat(&dh, "dewm-seat0");

        Self {
            display_handle: dh,
            compositor_state,
            xdg_shell_state,
            shm_state,
            output_manager_state,
            data_device_state,
            seat_state,
            seat,
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
        smithay::desktop::on_commit_buffer_handler::<Self>(surface);
    }
}
delegate_compositor!(DewmState);

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

    fn new_popup(&mut self, _surface: smithay::wayland::shell::xdg::PopupSurface, _positioner: smithay::wayland::shell::xdg::PositionerState) {
        // TODO: popup handling
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        self.workspace_grid.remove_window_by_surface(surface.wl_surface());
    }

    fn grab(&mut self, _surface: smithay::wayland::shell::xdg::PopupSurface, _seat: smithay::reexports::wayland_server::protocol::wl_seat::WlSeat, _serial: smithay::utils::Serial) {
        // TODO
    }
}
delegate_xdg_shell!(DewmState);

impl XdgDecorationHandler for DewmState {
    fn new_decoration(&mut self, toplevel: ToplevelSurface) {
        use smithay::wayland::shell::xdg::decoration::XdgToplevelDecorationState;
        // Tell the client to draw its own decorations (CSD).
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(
                smithay::reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode::ClientSide,
            );
        });
        toplevel.send_configure();
    }

    fn request_mode(&mut self, toplevel: ToplevelSurface, mode: smithay::reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode) {
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
delegate_xdg_decoration!(DewmState);

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
delegate_seat!(DewmState);

impl ShmHandler for DewmState {
    fn shm_state(&self) -> &ShmState { &self.shm_state }
}
delegate_shm!(DewmState);

impl DataDeviceHandler for DewmState {
    fn data_device_state(&self) -> &DataDeviceState { &self.data_device_state }
}
impl ClientDndGrabHandler for DewmState {}
impl ServerDndGrabHandler for DewmState {}
delegate_data_device!(DewmState);

delegate_output!(DewmState);

// ─── Per-client data ──────────────────────────────────────────────────────────

#[derive(Default)]
pub struct ClientState {
    pub compositor_state: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}
