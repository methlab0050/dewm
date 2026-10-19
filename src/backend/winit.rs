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

use std::sync::Arc;
use anyhow::Result;
use smithay::{
    backend::{
        renderer::{
            damage::OutputDamageTracker,
            element::surface::WaylandSurfaceRenderElement,
            gles::GlesRenderer,
        },
        winit::{self, WinitEvent},
    },
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::{
        calloop::{
            generic::Generic,
            EventLoop, Interest, Mode as CalloopMode, PostAction,
        },
        wayland_server::Display,
    },
    utils::{Rectangle, Transform},
    wayland::socket::ListeningSocketSource,
};

use crate::state::{ClientState, DewmState};

pub fn run() -> Result<()> {
    tracing::info!("Starting dewm with Winit backend (nested mode)");

    // ── Wayland display ───────────────────────────────────────────────────────
    let mut display = Display::<DewmState>::new()?;
    let mut event_loop = EventLoop::<DewmState>::try_new()?;

    // ── Winit window + OpenGL renderer ───────────────────────────────────────
    let (mut backend, winit_evt_loop) = winit::init::<GlesRenderer>()
        .map_err(|e| anyhow::anyhow!("Failed to initialize Winit backend: {e}"))?;

    backend.window().set_title("dewm");

    // ── Output ───────────────────────────────────────────────────────────────
    let output = Output::new(
        "winit".into(),
        PhysicalProperties {
            size:          (0, 0).into(),
            subpixel:      Subpixel::Unknown,
            make:          "dewm".into(),
            model:         "winit".into(),
            serial_number: "Unknown".into(),
        },
    );
    let mode = Mode {
        size:    backend.window_size(),
        refresh: 60_000,
    };

    // ── Compositor state ──────────────────────────────────────────────────────
    let mut state = DewmState::new(&mut display, &event_loop);

    let _global = output.create_global::<DewmState>(&state.display_handle);
    output.change_current_state(Some(mode), Some(Transform::Flipped180), None, Some((0, 0).into()));
    output.set_preferred(mode);

    state.space.map_output(&output, (0, 0));

    // ── Damage tracker ────────────────────────────────────────────────────────
    let mut damage_tracker = OutputDamageTracker::from_output(&output);

    // ── Event loop ────────────────────────────────────────────────────────────
    let loop_handle = event_loop.handle();

    // Register listening socket so clients can connect.
    let socket_source = ListeningSocketSource::new_auto()?;
    let socket_name = socket_source.socket_name().to_string_lossy().into_owned();
    tracing::info!(?socket_name, "Wayland display socket ready");
    std::env::set_var("WAYLAND_DISPLAY", &socket_name);

    loop_handle
        .insert_source(socket_source, |client_stream, _, state| {
            if let Err(err) = state
                .display_handle
                .insert_client(client_stream, Arc::new(ClientState::default()))
            {
                tracing::warn!("Error adding wayland client: {err}");
            }
        })
        .map_err(|e| anyhow::anyhow!("Failed to insert wayland socket source: {e}"))?;

    // Insert Wayland client dispatcher.
    loop_handle
        .insert_source(
            Generic::new(display, Interest::READ, CalloopMode::Level),
            |_, display, state| {
                unsafe {
                    display.get_mut().dispatch_clients(state).unwrap();
                }
                Ok(PostAction::Continue)
            },
        )
        .map_err(|e| anyhow::anyhow!("Failed to insert wayland server source: {e}"))?;

    backend.window().request_redraw();

    // Insert Winit event source.
    loop_handle
        .insert_source(winit_evt_loop, move |event, _, state| {
            match event {
                WinitEvent::Resized { size, .. } => {
                    output.change_current_state(
                        Some(Mode {
                            size,
                            refresh: 60_000,
                        }),
                        None,
                        None,
                        None,
                    );
                }
                WinitEvent::Input(input_event) => {
                    let size = backend.window_size();
                    state.process_input_event(input_event, (size.w as u32, size.h as u32));
                    backend.window().request_redraw();
                }
                WinitEvent::Redraw => {
                    let now = std::time::Instant::now();
                    let dt = now.duration_since(state.last_frame);
                    state.last_frame = now;

                    let anim_updated = state.pan_animation.update(dt);
                    if anim_updated || state.pan_animation.is_active() {
                        let cam = state.pan_animation.current_camera.to_i32_round();
                        state.space.map_output(&output, cam);
                        backend.window().request_redraw();
                    }

                    let size = backend.window_size();
                    let damage = Rectangle::from_size(size);

                    {
                        match backend.bind() {
                            Ok((renderer, mut framebuffer)) => {
                                if let Err(err) = smithay::desktop::space::render_output::<
                                    _,
                                    WaylandSurfaceRenderElement<GlesRenderer>,
                                    _,
                                    _,
                                >(
                                    &output,
                                    renderer,
                                    &mut framebuffer,
                                    1.0,
                                    0,
                                    [&state.space],
                                    &[],
                                    &mut damage_tracker,
                                    [0.118, 0.118, 0.180, 1.0], // Catppuccin Mocha base #1e1e2e
                                ) {
                                    tracing::warn!("Error rendering frame: {err}");
                                }
                            }
                            Err(err) => {
                                tracing::warn!("Error binding backend for redraw: {err}");
                            }
                        }
                    }

                    if let Err(err) = backend.submit(Some(&[damage])) {
                        tracing::warn!("Error submitting buffer: {err}");
                    }

                    state.space.elements().for_each(|window| {
                        window.send_frame(
                            &output,
                            state.start_time.elapsed(),
                            Some(std::time::Duration::ZERO),
                            |_, _| Some(output.clone()),
                        );
                    });

                    state.space.refresh();
                    state.popups.cleanup();
                    let _ = state.display_handle.flush_clients();

                    // Ask for redraw to schedule next frame.
                    backend.window().request_redraw();
                }
                WinitEvent::CloseRequested => {
                    state.loop_signal.stop();
                }
                WinitEvent::Focus(focused) => {
                    if focused {
                        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
                        if let Some(keyboard) = state.seat.get_keyboard() {
                            if let Some(window) = state.space.elements().last().cloned() {
                                if let Some(toplevel) = window.toplevel() {
                                    keyboard.set_focus(state, Some(toplevel.wl_surface().clone()), serial);
                                }
                            }
                        }
                    }
                }
            }
        })
        .map_err(|e| anyhow::anyhow!("Failed to insert winit event source: {e}"))?;

    tracing::info!("Entering event loop");
    event_loop.run(None, &mut state, |_| {})?;

    Ok(())
}
