//! Input event routing for all backends.
//!
//! Both the Winit and udev backends funnel raw `InputEvent`s through
//! `DewmState::process_input_event`.  This module handles:
//!   - Keyboard key presses → keybinding lookup + focus forwarding
//!   - Pointer motion / buttons → window focus + overview pointer events
//!   - Touchpad gesture events → GestureTracker

use smithay::{
    backend::input::{
        AbsolutePositionEvent, Axis, AxisSource, ButtonState, Event, InputEvent,
        KeyState, KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent, PointerMotionEvent,
    },
    input::{
        keyboard::FilterResult,
        pointer::{
            AxisFrame, ButtonEvent, Focus, GrabStartData, MotionEvent,
        },
    },
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Point, Rectangle, SERIAL_COUNTER},
};

use crate::{
    input::{
        grabs::{MoveSurfaceGrab, ResizeEdge, ResizeSurfaceGrab},
        keybindings::{spawn_process, KeyAction},
    },
    state::DewmState,
};

impl DewmState {
    /// Route a raw backend `InputEvent` to the appropriate handler.
    pub fn process_input_event<B: smithay::backend::input::InputBackend>(
        &mut self,
        event: InputEvent<B>,
        screen_size: (u32, u32),
    ) {
        match event {
            // ── Keyboard ─────────────────────────────────────────────────────
            InputEvent::Keyboard { event } => {
                self.handle_keyboard_event::<B>(event);
            }

            // ── Pointer motion (relative) ─────────────────────────────────────
            InputEvent::PointerMotion { event } => {
                let serial = SERIAL_COUNTER.next_serial();
                let Some(pointer) = self.seat.get_pointer() else { return; };
                let current_pos = pointer.current_location();
                let delta = event.delta();
                let pos = current_pos + Point::from((delta.x, delta.y));

                let under = if self.overview_state.is_visible() {
                    let output_info = self.space.outputs().next().and_then(|o| {
                        self.space.output_geometry(o).map(|geo| (geo.loc.to_f64(), (geo.size.w, geo.size.h)))
                    });
                    if let Some((geo_loc, out_size)) = output_info {
                        let out_pos = pos - geo_loc;
                        let cursor_pos = (out_pos.x as f32, out_pos.y as f32);
                        let active = self.workspace_grid.active();
                        if self.overview_dragger.is_dragging() {
                            self.overview_state.hovered_cell = crate::overview::drag::hit_test_cell(
                                cursor_pos,
                                &self.workspace_grid,
                                active,
                                out_size,
                                self.overview_state.camera_pos,
                                self.overview_state.animation_progress,
                            );
                        } else {
                            self.overview_state.hovered_cell = None;
                        }
                        self.overview_dragger.update(cursor_pos);
                    }
                    None
                } else {
                    self.surface_under(pos)
                };

                pointer.motion(
                    self,
                    under,
                    &MotionEvent {
                        location: pos,
                        serial,
                        time: event.time(),
                    },
                );
                pointer.frame(self);
            }

            // ── Pointer motion (absolute) ─────────────────────────────────────
            InputEvent::PointerMotionAbsolute { event } => {
                let Some(output) = self.space.outputs().next().cloned() else { return; };
                let Some(output_geo) = self.space.output_geometry(&output) else { return; };
                let pos = event.position_transformed(output_geo.size) + output_geo.loc.to_f64();
                let serial = SERIAL_COUNTER.next_serial();
                let Some(pointer) = self.seat.get_pointer() else { return; };

                let under = if self.overview_state.is_visible() {
                    let out_pos = pos - output_geo.loc.to_f64();
                    let cursor_pos = (out_pos.x as f32, out_pos.y as f32);
                    let out_size = (output_geo.size.w, output_geo.size.h);
                    let active = self.workspace_grid.active();
                    if self.overview_dragger.is_dragging() {
                        self.overview_state.hovered_cell = crate::overview::drag::hit_test_cell(
                            cursor_pos,
                            &self.workspace_grid,
                            active,
                            out_size,
                            self.overview_state.camera_pos,
                            self.overview_state.animation_progress,
                        );
                    } else {
                        self.overview_state.hovered_cell = None;
                    }
                    self.overview_dragger.update(cursor_pos);
                    None
                } else {
                    self.surface_under(pos)
                };

                pointer.motion(
                    self,
                    under,
                    &MotionEvent {
                        location: pos,
                        serial,
                        time: event.time(),
                    },
                );
                pointer.frame(self);
            }

            // ── Pointer buttons ───────────────────────────────────────────────
            InputEvent::PointerButton { event } => {
                let Some(pointer) = self.seat.get_pointer() else { return; };
                let Some(keyboard) = self.seat.get_keyboard() else { return; };

                let serial = SERIAL_COUNTER.next_serial();
                let button = event.button_code();
                let button_state = event.state();

                const BTN_LEFT: u32 = 0x110;
                const BTN_RIGHT: u32 = 0x111;

                if self.overview_state.is_visible() {
                    let loc = pointer.current_location();
                    let output_info = self.space.outputs().next().and_then(|o| {
                        self.space.output_geometry(o).map(|geo| (geo.loc.to_f64(), (geo.size.w, geo.size.h)))
                    });

                    if let Some((geo_loc, out_size)) = output_info {
                        let out_pos = loc - geo_loc;
                        let cursor_pos = (out_pos.x as f32, out_pos.y as f32);
                        let active = self.workspace_grid.active();

                        if button == BTN_LEFT {
                            if button_state == ButtonState::Pressed {
                                self.overview_dragger.try_begin(
                                    cursor_pos,
                                    &self.workspace_grid,
                                    &self.space,
                                    active,
                                    out_size,
                                    self.overview_state.camera_pos,
                                    self.overview_state.animation_progress,
                                );
                            } else if button_state == ButtonState::Released {
                                if let Some((window, from, to, is_dragging)) =
                                    self.overview_dragger.finish(
                                        cursor_pos,
                                        &self.workspace_grid,
                                        active,
                                        out_size,
                                        self.overview_state.camera_pos,
                                        self.overview_state.animation_progress,
                                    )
                                {
                                    if is_dragging {
                                        if from != to {
                                            tracing::info!(?from, ?to, "Overview: moved window");
                                            let old_origin = self.workspace_origin(from);
                                            let new_origin = self.workspace_origin(to);
                                            let cur_loc = self.space.element_location(&window).unwrap_or(old_origin);
                                            let rel_pos = cur_loc - old_origin;
                                            self.workspace_grid.move_window_to(&window, to);
                                            self.space.map_element(window, new_origin + rel_pos, true);
                                        }
                                    } else {
                                        // Tap without dragging! Focus window, teleport to its workspace, and exit overview.
                                        tracing::info!(?from, "Overview: tapped window -> focusing & zooming into workspace");
                                        self.space.raise_element(&window, true);
                                        self.teleport_to_workspace(from);
                                        if let Some(keyboard) = self.seat.get_keyboard() {
                                            let serial = SERIAL_COUNTER.next_serial();
                                            if let Some(top) = window.toplevel() {
                                                keyboard.set_focus(self, Some(top.wl_surface().clone()), serial);
                                            }
                                        }
                                        self.exit_overview();
                                    }
                                } else if let Some(target) =
                                    crate::overview::drag::hit_test_cell(
                                        cursor_pos,
                                        &self.workspace_grid,
                                        active,
                                        out_size,
                                        self.overview_state.camera_pos,
                                        self.overview_state.animation_progress,
                                    )
                                {
                                    self.teleport_to_workspace(target);
                                    self.exit_overview();
                                }
                            }
                        } else if button == BTN_RIGHT && button_state == ButtonState::Pressed {
                            self.overview_dragger.cancel();
                        }
                    }
                    return;
                }

                if button_state == ButtonState::Pressed && !pointer.is_grabbed() {
                    let loc = pointer.current_location();
                    let modifiers = keyboard.modifier_state();

                    if let Some((window, _)) = self.space.element_under(loc).map(|(w, l)| (w.clone(), l)) {
                        tracing::debug!(?loc, "Clicked on window, raising and setting keyboard focus");
                        self.space.raise_element(&window, true);
                        if let Some(toplevel) = window.toplevel() {
                            keyboard.set_focus(
                                self,
                                Some(toplevel.wl_surface().clone()),
                                serial,
                            );
                        }
                        self.space.elements().for_each(|w| {
                            w.set_activated(w == &window);
                            if let Some(t) = w.toplevel() {
                                t.send_pending_configure();
                            }
                        });

                        // Super + Left Click / Alt + Left Click = Interactive Move Grab
                        // Super + Right Click / Alt + Right Click = Interactive Resize Grab
                        if modifiers.logo || modifiers.alt {
                            let initial_window_location = self.space.element_location(&window).unwrap();
                            let start_data = GrabStartData {
                                focus: None,
                                button,
                                location: loc,
                            };

                            if button == BTN_LEFT {
                                let grab = MoveSurfaceGrab {
                                    start_data,
                                    window,
                                    initial_window_location,
                                };
                                pointer.set_grab(self, grab, serial, Focus::Clear);
                                return;
                            } else if button == BTN_RIGHT {
                                let initial_window_size = window.geometry().size;
                                let grab = ResizeSurfaceGrab::start(
                                    start_data,
                                    window,
                                    ResizeEdge::BOTTOM_RIGHT,
                                    Rectangle::new(initial_window_location, initial_window_size),
                                );
                                pointer.set_grab(self, grab, serial, Focus::Clear);
                                return;
                            }
                        }
                    } else {
                        tracing::debug!(?loc, "Clicked empty desktop, clearing focus");
                        // Clicked empty desktop
                        self.space.elements().for_each(|w| {
                            w.set_activated(false);
                            if let Some(t) = w.toplevel() {
                                t.send_pending_configure();
                            }
                        });
                        keyboard.set_focus(self, Option::<WlSurface>::None, serial);
                    }
                }

                pointer.button(
                    self,
                    &ButtonEvent {
                        button,
                        state: button_state,
                        serial,
                        time: event.time(),
                    },
                );
                pointer.frame(self);
            }

            // ── Pointer scrolling / axis ──────────────────────────────────────
            InputEvent::PointerAxis { event } => {
                let source = event.source();

                let horizontal_amount = event
                    .amount(Axis::Horizontal)
                    .unwrap_or_else(|| event.amount_v120(Axis::Horizontal).unwrap_or(0.0) * 15.0 / 120.);
                let vertical_amount = event
                    .amount(Axis::Vertical)
                    .unwrap_or_else(|| event.amount_v120(Axis::Vertical).unwrap_or(0.0) * 15.0 / 120.);

                // If in overview mode, two-finger swipe or mouse wheel snaps across workspaces!
                if self.overview_state.is_visible() {
                    let out_size = self
                        .space
                        .outputs()
                        .next()
                        .and_then(|o| self.space.output_geometry(o))
                        .map(|geo| (geo.size.w, geo.size.h))
                        .unwrap_or((1280, 800));

                    let is_discrete = matches!(source, AxisSource::Wheel)
                        || event.amount_v120(Axis::Horizontal).is_some()
                        || event.amount_v120(Axis::Vertical).is_some();

                    self.overview_state.snap_scroll(
                        Point::from((horizontal_amount as f32, vertical_amount as f32)),
                        is_discrete,
                        &self.workspace_grid,
                        out_size,
                    );
                    return;
                }
                let horizontal_amount_discrete = event.amount_v120(Axis::Horizontal);
                let vertical_amount_discrete = event.amount_v120(Axis::Vertical);

                let mut frame = AxisFrame::new(event.time()).source(source);
                if horizontal_amount != 0.0 {
                    frame = frame.value(Axis::Horizontal, horizontal_amount);
                    if let Some(discrete) = horizontal_amount_discrete {
                        frame = frame.v120(Axis::Horizontal, discrete as i32);
                    }
                }
                if vertical_amount != 0.0 {
                    frame = frame.value(Axis::Vertical, vertical_amount);
                    if let Some(discrete) = vertical_amount_discrete {
                        frame = frame.v120(Axis::Vertical, discrete as i32);
                    }
                }

                if source == AxisSource::Finger {
                    if event.amount(Axis::Horizontal) == Some(0.0) {
                        frame = frame.stop(Axis::Horizontal);
                    }
                    if event.amount(Axis::Vertical) == Some(0.0) {
                        frame = frame.stop(Axis::Vertical);
                    }
                }

                if let Some(pointer) = self.seat.get_pointer() {
                    pointer.axis(self, frame);
                    pointer.frame(self);
                }
            }

            // ── Gesture events ────────────────────────────────────────────────
            InputEvent::GestureSwipeBegin { event } => {
                self.handle_gesture_swipe_begin::<B>(event);
            }
            InputEvent::GestureSwipeUpdate { event } => {
                self.handle_gesture_swipe_update::<B>(event, screen_size);
            }
            InputEvent::GestureSwipeEnd { event } => {
                self.handle_gesture_swipe_end::<B>(event);
            }

            // Unhandled events.
            _ => {}
        }
    }

    // ── Keyboard ──────────────────────────────────────────────────────────────

    fn handle_keyboard_event<B: smithay::backend::input::InputBackend>(
        &mut self,
        event: B::KeyboardKeyEvent,
    ) where
        B::KeyboardKeyEvent: KeyboardKeyEvent<B>,
    {
        let serial = SERIAL_COUNTER.next_serial();
        let time   = event.time();
        let key_state = event.state();

        let keyboard = self.seat.get_keyboard().unwrap();

        keyboard.input::<(), _>(
            self,
            event.key_code(),
            key_state,
            serial,
            time,
            |compositor, modifiers, handle| {
                if matches!(key_state, KeyState::Released) {
                    return FilterResult::Forward;
                }

                let keysym = handle.modified_sym();

                if compositor.overview_state.is_visible() {
                    if keysym == smithay::input::keyboard::Keysym::Escape {
                        compositor.exit_overview();
                        return FilterResult::Intercept(());
                    } else if keysym == smithay::input::keyboard::Keysym::Return
                        || keysym == smithay::input::keyboard::Keysym::KP_Enter
                        || keysym == smithay::input::keyboard::Keysym::space
                    {
                        let target = compositor.overview_state.focused_cell;
                        compositor.teleport_to_workspace(target);
                        compositor.exit_overview();
                        return FilterResult::Intercept(());
                    }
                }

                if let Some(action) = compositor.keybindings.lookup(modifiers, keysym).cloned() {
                    match action {
                        KeyAction::SpawnProcess(cmd) => {
                            spawn_process(&cmd);
                        }
                        KeyAction::NavigateWorkspace(dir) => {
                            compositor.switch_workspace(dir);
                        }
                        KeyAction::ToggleOverview => {
                            compositor.toggle_overview();
                        }
                        KeyAction::CloseFocused => {
                            if let Some(surface) = compositor.seat.get_keyboard().and_then(|k| k.current_focus()) {
                                if let Some(window) = compositor.space.elements().find(|w| {
                                    w.toplevel().map(|t| t.wl_surface() == &surface).unwrap_or(false)
                                }) {
                                    if let Some(toplevel) = window.toplevel() {
                                        toplevel.send_close();
                                    }
                                }
                            }
                        }
                        KeyAction::ToggleFullscreen => {
                            let toplevel = compositor
                                .seat
                                .get_keyboard()
                                .and_then(|k| k.current_focus())
                                .and_then(|surface| {
                                    compositor
                                        .space
                                        .elements()
                                        .find(|w| {
                                            w.toplevel().map(|t| t.wl_surface() == &surface).unwrap_or(false)
                                        })
                                        .and_then(|w| w.toplevel().cloned())
                                });

                            if let Some(toplevel) = toplevel {
                                compositor.toggle_fullscreen(&toplevel);
                            }
                        }
                        KeyAction::ToggleMaximize => {
                            let toplevel = compositor
                                .seat
                                .get_keyboard()
                                .and_then(|k| k.current_focus())
                                .and_then(|surface| {
                                    compositor
                                        .space
                                        .elements()
                                        .find(|w| {
                                            w.toplevel().map(|t| t.wl_surface() == &surface).unwrap_or(false)
                                        })
                                        .and_then(|w| w.toplevel().cloned())
                                });

                            if let Some(toplevel) = toplevel {
                                compositor.toggle_maximize(&toplevel);
                            }
                        }
                        KeyAction::Quit => {
                            compositor.loop_signal.stop();
                        }
                    }
                    FilterResult::Intercept(())
                } else {
                    FilterResult::Forward
                }
            },
        );
    }
}

