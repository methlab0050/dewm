//! Input event routing for all backends.
//!
//! Both the Winit and udev backends funnel raw `InputEvent`s through
//! `DewmState::process_input_event`.  This module handles:
//!   - Keyboard key presses → keybinding lookup + focus forwarding
//!   - Pointer motion / buttons → window focus + overview pointer events
//!   - Touchpad gesture events → GestureTracker

use smithay::{
    backend::input::{
        Event, InputEvent, KeyState,
        KeyboardKeyEvent, PointerButtonEvent, PointerMotionEvent,
        AbsolutePositionEvent,
    },
    input::{
        keyboard::{FilterResult, KeyboardHandle},
        pointer::{ButtonEvent, MotionEvent},
    },
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::SERIAL_COUNTER,
};

use crate::{
    input::keybindings::{spawn_process, KeyAction},
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

            // ── Pointer motion ────────────────────────────────────────────────
            InputEvent::PointerMotion { event } => {
                // TODO: update cursor position, update focus under pointer
            }
            InputEvent::PointerMotionAbsolute { event } => {
                // TODO: absolute motion (e.g. from Winit mouse moves)
            }

            // ── Pointer buttons ───────────────────────────────────────────────
            InputEvent::PointerButton { event } => {
                // TODO: focus window under pointer, handle overview clicks/drags
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
        let time   = event.time_msec();
        let state  = event.state();

        let keyboard = self.seat.get_keyboard().unwrap();

        keyboard.input::<(), _>(
            self,
            event.key_code(),
            state,
            serial,
            time,
            |state, modifiers, handle| {
                if matches!(state_to_key_state(state), KeyState::Released) {
                    return FilterResult::Forward;
                }

                let keysym = handle.modified_sym();

                if let Some(action) = state.keybindings.lookup(modifiers, keysym).cloned() {
                    match action {
                        KeyAction::SpawnProcess(cmd) => {
                            spawn_process(&cmd);
                        }
                        KeyAction::NavigateWorkspace(dir) => {
                            if let Some(new_cell) = state.workspace_grid.navigate(dir) {
                                tracing::info!(?new_cell, "Keyboard workspace navigate");
                            }
                        }
                        KeyAction::ToggleOverview => {
                            state.overview_state.toggle();
                        }
                        KeyAction::CloseFocused => {
                            // TODO: send close request to focused window
                        }
                        KeyAction::Quit => {
                            state.loop_signal.stop();
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

fn state_to_key_state(
    state: smithay::backend::input::KeyState,
) -> smithay::backend::input::KeyState {
    state
}
