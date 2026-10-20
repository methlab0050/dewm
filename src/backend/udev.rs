//! udev / DRM backend — native hardware rendering.
//!
//! This backend is used when running on a real GPU at a VT/TTY.
//! Launch with:
//!
//! ```
//! dewm --backend udev
//! ```
//!
//! Requires `seatd` (or `logind`) to be running for GPU/input device access.

use std::{collections::HashMap, sync::Arc, time::Duration};
use anyhow::{Context, Result};
use smithay::{
    backend::{
        allocator::{
            gbm::{GbmAllocator, GbmBufferFlags, GbmDevice},
            Fourcc,
        },
        drm::{DrmDevice, DrmDeviceFd, DrmEvent, GbmBufferedSurface},
        egl::{EGLContext, EGLDisplay},
        input::InputEvent,
        libinput::{LibinputInputBackend, LibinputSessionInterface},
        renderer::{
            damage::OutputDamageTracker,
            element::surface::WaylandSurfaceRenderElement,
            gles::GlesRenderer,
            Bind, ImportDma,
        },
        session::{
            libseat::LibSeatSession,
            Event as SessionEvent, Session,
        },
        udev::{all_gpus, primary_gpu},
    },
    desktop::Window,
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::{
        calloop::{
            generic::Generic,
            timer::{TimeoutAction, Timer},
            EventLoop, Interest, Mode as CalloopMode, PostAction,
        },
        drm::control::{connector, crtc, Device as _, ModeTypeFlags},
        input::{DeviceCapability, Libinput},
        rustix::fs::OFlags,
        wayland_server::Display,
    },
    utils::{DeviceFd, Transform},
    wayland::socket::ListeningSocketSource,
};

use crate::{
    rendering::elements::DewmRenderElement,
    state::{ClientState, DewmState},
};

struct DrmOutput {
    output: Output,
    surface: GbmBufferedSurface<GbmAllocator<DrmDeviceFd>, ()>,
    damage_tracker: OutputDamageTracker,
}

pub fn run() -> Result<()> {
    tracing::info!("Starting dewm with native DRM/KMS udev backend");

    // ── Session (libseat / seatd) ──────────────────────────────────────────────
    let (mut session, notifier) = LibSeatSession::new().map_err(|err| {
        anyhow::anyhow!(
            "Could not connect to seatd/logind ({err}). \
             Ensure seatd or logind is active and dewm is run from a Linux TTY."
        )
    })?;

    let seat_name = session.seat();
    tracing::info!(?seat_name, "libseat session initialized");

    // ── Wayland display & event loop ──────────────────────────────────────────
    let mut display = Display::<DewmState>::new()?;
    let mut event_loop = EventLoop::<DewmState>::try_new()?;
    let loop_handle = event_loop.handle();

    // ── Primary GPU discovery ─────────────────────────────────────────────────
    let primary_gpu_path = primary_gpu(&seat_name)
        .ok()
        .flatten()
        .or_else(|| all_gpus(&seat_name).ok()?.into_iter().next())
        .context("No GPU found on this seat. Ensure DRM drivers are loaded.")?;

    tracing::info!(gpu = ?primary_gpu_path, "Using primary GPU for DRM display");

    let fd = session
        .open(
            &primary_gpu_path,
            OFlags::RDWR | OFlags::CLOEXEC | OFlags::NONBLOCK | OFlags::NOCTTY,
        )
        .context("Failed to open DRM device via libseat session")?;

    let drm_fd = DrmDeviceFd::new(DeviceFd::from(fd));
    let (mut drm_device, drm_notifier) = DrmDevice::new(drm_fd.clone(), true)
        .context("Failed to initialize DrmDevice")?;
    let gbm_device = GbmDevice::new(drm_fd)
        .context("Failed to initialize GbmDevice")?;

    // ── EGL & OpenGL ES Renderer ──────────────────────────────────────────────
    let egl_display = unsafe { EGLDisplay::new(gbm_device.clone()) }
        .context("Failed to create EGLDisplay from GbmDevice")?;
    let egl_context = EGLContext::new(&egl_display)
        .context("Failed to create EGLContext")?;
    let mut renderer = unsafe { GlesRenderer::new(egl_context) }
        .context("Failed to create GlesRenderer")?;

    let allocator = GbmAllocator::new(
        gbm_device,
        GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT,
    );

    // ── Compositor State ──────────────────────────────────────────────────────
    let mut state = DewmState::new(&mut display, &event_loop);

    // ── Connector & Display Detection ─────────────────────────────────────────
    let res_handles = drm_device
        .resource_handles()
        .context("Failed to query DRM resource handles")?;

    let mut outputs: HashMap<crtc::Handle, DrmOutput> = HashMap::new();
    let mut primary_screen_size: (u32, u32) = (1920, 1080);

    for &conn_handle in res_handles.connectors() {
        let conn_info = match drm_device.get_connector(conn_handle, false) {
            Ok(info) => info,
            Err(e) => {
                tracing::warn!(?conn_handle, "Failed to get connector info: {e}");
                continue;
            }
        };

        if conn_info.state() != connector::State::Connected {
            continue;
        }

        let Some(&drm_mode) = conn_info
            .modes()
            .iter()
            .find(|m| m.mode_type().contains(ModeTypeFlags::PREFERRED))
            .or_else(|| conn_info.modes().first())
        else {
            tracing::warn!(?conn_handle, "Connected connector has no valid video modes");
            continue;
        };

        let crtc_handle = match res_handles.crtcs().first().copied() {
            Some(c) => c,
            None => {
                tracing::warn!("No CRTC available to drive connector");
                break;
            }
        };

        let drm_surface = match drm_device.create_surface(crtc_handle, drm_mode, &[conn_handle]) {
            Ok(s) => s,
            Err(err) => {
                tracing::warn!(?crtc_handle, "Failed to create DrmSurface: {err}");
                continue;
            }
        };

        let gbm_surface = match GbmBufferedSurface::new(
            drm_surface,
            allocator.clone(),
            &[Fourcc::Argb8888, Fourcc::Xrgb8888],
            renderer.dmabuf_formats(),
        ) {
            Ok(s) => s,
            Err(err) => {
                tracing::warn!("Failed to create GbmBufferedSurface: {err}");
                continue;
            }
        };

        let w = drm_mode.size().0 as i32;
        let h = drm_mode.size().1 as i32;
        primary_screen_size = (w as u32, h as u32);

        let output = Output::new(
            format!("DRM-{}", outputs.len() + 1),
            PhysicalProperties {
                size: (0, 0).into(),
                subpixel: Subpixel::Unknown,
                make: "dewm-drm".into(),
                model: "KMS Output".into(),
                serial_number: "Unknown".into(),
            },
        );

        let wl_mode = Mode {
            size: (w, h).into(),
            refresh: (drm_mode.vrefresh() * 1000) as i32,
        };

        let _global = output.create_global::<DewmState>(&state.display_handle);
        output.change_current_state(Some(wl_mode), Some(Transform::Normal), None, Some((0, 0).into()));
        output.set_preferred(wl_mode);

        state.space.map_output(&output, (0, 0));
        let damage_tracker = OutputDamageTracker::from_output(&output);

        tracing::info!(name = output.name(), mode = ?wl_mode, "Initialized DRM output");

        outputs.insert(
            crtc_handle,
            DrmOutput {
                output,
                surface: gbm_surface,
                damage_tracker,
            },
        );
        break; // Bind primary monitor
    }

    if outputs.is_empty() {
        tracing::warn!("No connected DRM monitors detected; compositor will wait for display hotplug");
    }

    // ── Libinput (Hardware input devices) ─────────────────────────────────────
    let mut libinput_context = Libinput::new_with_udev::<LibinputSessionInterface<LibSeatSession>>(
        session.clone().into(),
    );

    if libinput_context.udev_assign_seat(&seat_name).is_err() {
        tracing::warn!("Failed to assign seat to libinput");
    }

    let libinput_backend = LibinputInputBackend::new(libinput_context);

    loop_handle
        .insert_source(libinput_backend, move |event, _, state| {
            if let InputEvent::DeviceAdded { mut device } = event {
                // Enable tap-to-click on touchpads
                if device.config_tap_finger_count() > 0 {
                    let _ = device.config_tap_set_enabled(true);
                }
                // Enable natural scrolling
                if device.has_capability(DeviceCapability::Gesture) {
                    let _ = device.config_scroll_set_natural_scroll_enabled(true);
                }
                tracing::info!(name = %device.name(), "Input device added");
            } else {
                state.process_input_event(event, primary_screen_size);
            }
        })
        .map_err(|e| anyhow::anyhow!("Failed to register libinput source: {e}"))?;

    // ── Session Event Notifier (VT Switch handling) ───────────────────────────
    loop_handle
        .insert_source(notifier, move |event, _, _state| match event {
            SessionEvent::PauseSession => {
                tracing::info!("Session paused (VT switched away)");
            }
            SessionEvent::ActivateSession => {
                tracing::info!("Session activated (VT switched back)");
            }
        })
        .map_err(|e| anyhow::anyhow!("Failed to register session notifier: {e}"))?;

    // ── DRM Event Notifier (vblank page flips) ─────────────────────────────────
    loop_handle
        .insert_source(drm_notifier, move |event, _metadata, _state| match event {
            DrmEvent::VBlank(crtc) => {
                tracing::trace!(?crtc, "VBlank received");
            }
            DrmEvent::Error(err) => {
                tracing::error!(?err, "DRM event error");
            }
        })
        .map_err(|e| anyhow::anyhow!("Failed to register DRM event notifier: {e}"))?;

    // ── Wayland display socket & client dispatcher ────────────────────────────
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
                tracing::warn!("Error adding Wayland client: {err}");
            }
        })
        .map_err(|e| anyhow::anyhow!("Failed to insert Wayland socket source: {e}"))?;

    loop_handle
        .insert_source(
            Generic::new(display, Interest::READ, CalloopMode::Level),
            |_, display, state| {
                unsafe {
                    if let Err(err) = display.get_mut().dispatch_clients(state) {
                        tracing::error!("Error dispatching Wayland clients: {err}");
                    }
                }
                Ok(PostAction::Continue)
            },
        )
        .map_err(|e| anyhow::anyhow!("Failed to insert Wayland server source: {e}"))?;

    // ── Xwayland ──────────────────────────────────────────────────────────────
    if let Err(err) = state.start_xwayland() {
        tracing::warn!("Could not start Xwayland: {err}");
    }

    // ── Periodic Frame Redraw Timer (60Hz / 16ms VSync cadence) ───────────────
    let frame_timer = Timer::immediate();
    loop_handle
        .insert_source(frame_timer, move |_instant, _, state| {
            for drm_output in outputs.values_mut() {
                let _ = render_drm_frame(
                    state,
                    &drm_output.output,
                    &mut drm_output.surface,
                    &mut drm_output.damage_tracker,
                    &mut renderer,
                );
            }
            TimeoutAction::ToDuration(Duration::from_millis(16))
        })
        .map_err(|e| anyhow::anyhow!("Failed to insert frame timer: {e}"))?;

    tracing::info!("Entering native DRM compositor event loop");
    event_loop.run(None, &mut state, |_| {})?;

    tracing::info!("dewm udev session exited cleanly");
    Ok(())
}

fn render_drm_frame(
    state: &DewmState,
    output: &Output,
    surface: &mut GbmBufferedSurface<GbmAllocator<DrmDeviceFd>, ()>,
    damage_tracker: &mut OutputDamageTracker,
    renderer: &mut GlesRenderer,
) -> Result<()> {
    let (mut dmabuf, age) = surface.next_buffer().context("Failed to acquire next DRM buffer")?;
    let mut target = renderer.bind(&mut dmabuf).map_err(|e| anyhow::anyhow!("Failed to bind Dmabuf: {e}"))?;
    let clear_color = [0.118, 0.118, 0.180, 1.0]; // Catppuccin Mocha base #1e1e2e

    if state.overview_state.is_visible() {
        let elements = crate::overview::renderer::overview_render_elements(
            renderer,
            state,
            output,
        );
        damage_tracker
            .render_output(
                renderer,
                &mut target,
                age as usize,
                &elements,
                clear_color,
            )
            .map_err(|e| anyhow::anyhow!("Failed to render overview frame: {e}"))?;
    } else {
        let res = smithay::desktop::space::space_render_elements::<
            GlesRenderer,
            Window,
            _,
        >(renderer, [&state.space], output, 1.0);

        match res {
            Ok(space_elements) => {
                let elements: Vec<DewmRenderElement<GlesRenderer, WaylandSurfaceRenderElement<GlesRenderer>>> =
                    space_elements.into_iter().map(DewmRenderElement::Space).collect();

                damage_tracker
                    .render_output(
                        renderer,
                        &mut target,
                        age as usize,
                        &elements,
                        clear_color,
                    )
                    .map_err(|e| anyhow::anyhow!("Failed to render space frame: {e}"))?;
            }
            Err(err) => {
                tracing::warn!("Error extracting space render elements: {err}");
            }
        }
    }

    surface.queue_buffer(None, None, ()).context("Failed to queue buffer for page flip")?;
    Ok(())
}
