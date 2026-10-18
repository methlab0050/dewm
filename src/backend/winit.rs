//! Winit backend — runs dewm as a window inside an existing compositor.
//!
//! This is the primary backend for day-to-day development.  Launch with:
//!
//! ```
//! RUST_LOG=dewm=debug cargo run -- --backend winit
//! ```
//!
//! Then open Wayland clients inside the nested session:
//! ```
//! WAYLAND_DISPLAY=wayland-1 foot
//! ```

use anyhow::Result;
use smithay::{
    backend::{
        renderer::{
            damage::OutputDamageTracker,
            element::surface::WaylandSurfaceRenderElement,
            gles::GlesRenderer,
        },
        winit::{self, WinitEvent, WinitGraphicsBackend},
    },
    desktop::space::SpaceRenderElements,
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::{
        calloop::{
            EventLoop,
            timer::{Timer, TimeoutAction},
        },
        wayland_server::Display,
    },
    utils::{Rectangle, Transform},
};

use crate::state::{ClientState, DewmState};

pub fn run() -> Result<()> {
    tracing::info!("Starting dewm with Winit backend (nested mode)");

    // ── Wayland display ───────────────────────────────────────────────────────
    let mut display = Display::<DewmState>::new()?;
    let mut event_loop = EventLoop::<DewmState>::try_new()?;

    // ── Winit window + OpenGL renderer ───────────────────────────────────────
    let (mut backend, winit_evt_loop) = winit::init::<GlesRenderer>()?;

    // ── Output ───────────────────────────────────────────────────────────────
    let output = Output::new(
        "winit".into(),
        PhysicalProperties {
            size:     (0, 0).into(),
            subpixel: Subpixel::Unknown,
            make:     "dewm".into(),
            model:    "winit".into(),
        },
    );
    let mode = Mode {
        size:    backend.window_size(),
        refresh: 60_000,
    };
    output.change_current_state(Some(mode), Some(Transform::Normal), None, Some((0, 0).into()));
    output.set_preferred(mode);

    // ── Compositor state ──────────────────────────────────────────────────────
    let mut state = DewmState::new(&mut display, &event_loop);
    state.space.map_output(&output, (0, 0));

    // Register the Wayland socket so clients can connect.
    let socket_name = display.add_socket_auto()?;
    tracing::info!(?socket_name, "Wayland display socket ready");
    std::env::set_var("WAYLAND_DISPLAY", socket_name.to_string_lossy().as_ref());

    // ── Damage tracker ────────────────────────────────────────────────────────
    let mut damage_tracker =
        OutputDamageTracker::from_output(&output);

    // ── Event loop ────────────────────────────────────────────────────────────
    let loop_handle = event_loop.handle();

    // Insert Winit event source.
    loop_handle.insert_source(winit_evt_loop, move |event, _, state| {
        match event {
            WinitEvent::Resized { size, .. } => {
                // TODO: update output mode and damage tracker
                let _ = size;
            }
            WinitEvent::Input(input_event) => {
                state.process_input_event(input_event, (1920, 1080));
            }
            WinitEvent::Refresh => {
                // Redraw request from the Winit window.
            }
            WinitEvent::Focus(_) | WinitEvent::CloseRequested => {}
        }
    })?;

    // Insert Wayland client source.
    let display_handle = display.handle();
    loop_handle
        .insert_source(
            calloop_wayland_source::WaylandSource::new(display_handle, display),
            |_, _, state| Ok(()),
        )?;

    // Main loop — 60 fps render timer.
    let timer = Timer::immediate();
    loop_handle.insert_source(timer, move |_, _, state| {
        // TODO: render frame
        TimeoutAction::ToDuration(std::time::Duration::from_millis(16))
    })?;

    tracing::info!("Entering event loop");
    event_loop.run(None, &mut state, |_| {})?;

    Ok(())
}
