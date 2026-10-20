//! Keybinding configuration and dispatch.
//!
//! # Bound keys
//!
//! | Binding                  | Action                                 |
//! |--------------------------|----------------------------------------|
//! | `Super + D`              | Launch Vicinae app launcher            |
//! | `Alt + Left / Right`     | Navigate workspace Left / Right        |
//! | `Alt + Up / Down`        | Navigate workspace Up / Down           |
//! | `Super + Ctrl + Arrow`   | Navigate workspace in direction        |
//! | `Super + Tab`            | Toggle workspace overview              |
//! | `Super + Return`         | Launch terminal (foot)                 |
//! | `Alt + Return`           | Launch terminal (foot)                 |
//! | `Super + Q` / `Alt + F4` | Close focused window                   |
//! | `Alt + F` / `Super + F`  | Toggle focused window fullscreen       |
//! | `Alt + M` / `Super + M`  | Toggle focused window maximized        |
//! | `Alt + Shift + Q`        | Quit compositor                        |

use smithay::input::keyboard::{Keysym, ModifiersState};

use crate::workspace::grid::Direction;

/// High-level action triggered by a keybinding.
#[derive(Debug, Clone)]
pub enum KeyAction {
    /// Spawn an external process by command name.
    SpawnProcess(String),
    /// Navigate the workspace grid.
    NavigateWorkspace(Direction),
    /// Toggle the overview.
    ToggleOverview,
    /// Close the currently focused window.
    CloseFocused,
    /// Toggle fullscreen mode for the focused window.
    ToggleFullscreen,
    /// Toggle maximized mode for the focused window.
    ToggleMaximize,
    /// Quit the compositor.
    Quit,
}

/// A single keybinding: modifier mask + keysym → action.
#[derive(Clone)]
pub struct Keybinding {
    pub mods:   ModifierMask,
    pub keysym: Keysym,
    pub action: KeyAction,
}

/// Simplified modifier mask.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ModifierMask {
    pub super_key: bool,
    pub ctrl:      bool,
    pub alt:       bool,
    pub shift:     bool,
}

impl ModifierMask {
    pub fn super_only() -> Self {
        Self { super_key: true, ctrl: false, alt: false, shift: false }
    }

    pub fn super_ctrl() -> Self {
        Self { super_key: true, ctrl: true, alt: false, shift: false }
    }

    pub fn alt_only() -> Self {
        Self { super_key: false, ctrl: false, alt: true, shift: false }
    }

    pub fn alt_ctrl() -> Self {
        Self { super_key: false, ctrl: true, alt: true, shift: false }
    }

    pub fn super_shift() -> Self {
        Self { super_key: true, ctrl: false, alt: false, shift: true }
    }

    pub fn alt_shift() -> Self {
        Self { super_key: false, ctrl: false, alt: true, shift: true }
    }

    pub fn matches(&self, mods: &ModifiersState) -> bool {
        self.super_key == mods.logo
            && self.ctrl  == mods.ctrl
            && self.alt   == mods.alt
            && self.shift == mods.shift
    }
}

/// Keybinding table.
pub struct KeybindingConfig {
    pub bindings: Vec<Keybinding>,
}

impl Default for KeybindingConfig {
    fn default() -> Self {
        use KeyAction::*;
        let b = |mods, keysym, action| Keybinding { mods, keysym, action };

        Self {
            bindings: vec![
                // ── App launcher & terminal ───────────────────────────────────
                b(ModifierMask::super_only(), Keysym::d,
                    SpawnProcess("vicinae".into())),
                b(ModifierMask::super_only(), Keysym::Return,
                    SpawnProcess("foot".into())),
                b(ModifierMask::alt_only(), Keysym::Return,
                    SpawnProcess("foot".into())),

                // ── Workspace navigation (keyboard) ───────────────────────────
                b(ModifierMask::alt_only(), Keysym::Left,
                    NavigateWorkspace(Direction::Left)),
                b(ModifierMask::alt_only(), Keysym::Right,
                    NavigateWorkspace(Direction::Right)),
                b(ModifierMask::alt_only(), Keysym::Up,
                    NavigateWorkspace(Direction::Up)),
                b(ModifierMask::alt_only(), Keysym::Down,
                    NavigateWorkspace(Direction::Down)),
                b(ModifierMask::super_ctrl(), Keysym::Left,
                    NavigateWorkspace(Direction::Left)),
                b(ModifierMask::super_ctrl(), Keysym::Right,
                    NavigateWorkspace(Direction::Right)),
                b(ModifierMask::super_ctrl(), Keysym::Up,
                    NavigateWorkspace(Direction::Up)),
                b(ModifierMask::super_ctrl(), Keysym::Down,
                    NavigateWorkspace(Direction::Down)),
                b(ModifierMask::alt_ctrl(), Keysym::Left,
                    NavigateWorkspace(Direction::Left)),
                b(ModifierMask::alt_ctrl(), Keysym::Right,
                    NavigateWorkspace(Direction::Right)),
                b(ModifierMask::alt_ctrl(), Keysym::Up,
                    NavigateWorkspace(Direction::Up)),
                b(ModifierMask::alt_ctrl(), Keysym::Down,
                    NavigateWorkspace(Direction::Down)),

                // ── Overview ─────────────────────────────────────────────────
                b(ModifierMask::super_only(), Keysym::Tab, ToggleOverview),
                b(ModifierMask::alt_only(),   Keysym::Tab, ToggleOverview),
                b(ModifierMask::alt_only(),   Keysym::o,   ToggleOverview),
                b(ModifierMask::super_only(), Keysym::o,   ToggleOverview),

                // ── Window management ─────────────────────────────────────────
                b(ModifierMask::super_only(), Keysym::q,  CloseFocused),
                b(ModifierMask::alt_only(),   Keysym::F4, CloseFocused),
                b(ModifierMask::alt_only(),   Keysym::f,  ToggleFullscreen),
                b(ModifierMask::super_only(), Keysym::f,  ToggleFullscreen),
                b(ModifierMask::alt_only(),   Keysym::F11, ToggleFullscreen),
                b(ModifierMask::alt_only(),   Keysym::m,  ToggleMaximize),
                b(ModifierMask::super_only(), Keysym::m,  ToggleMaximize),

                // ── Session ───────────────────────────────────────────────────
                b(ModifierMask::super_shift(), Keysym::q, Quit),
                b(ModifierMask::super_shift(), Keysym::Q, Quit),
                b(ModifierMask::alt_shift(),   Keysym::q, Quit),
                b(ModifierMask::alt_shift(),   Keysym::Q, Quit),
            ],
        }
    }
}

impl KeybindingConfig {
    /// Look up an action for the given key event.  Returns the first match.
    pub fn lookup(&self, mods: &ModifiersState, keysym: Keysym) -> Option<&KeyAction> {
        self.bindings.iter().find_map(|b| {
            if b.keysym == keysym && b.mods.matches(mods) {
                Some(&b.action)
            } else {
                None
            }
        })
    }
}

// ─── Process spawning ─────────────────────────────────────────────────────────

/// Spawn `cmd` as a detached child process using `fork` + `exec`.
/// The child process is immediately orphaned (parent does not wait).
pub fn spawn_process(cmd: &str) {
    use std::ffi::CString;

    let cmd_cstr = match CString::new(cmd) {
        Ok(s)  => s,
        Err(e) => {
            tracing::error!("spawn_process: invalid command string: {e}");
            return;
        }
    };

    unsafe {
        let pid = libc::fork();
        if pid < 0 {
            tracing::error!("fork() failed");
        } else if pid == 0 {
            // Child: exec the command.  On failure, exit immediately.
            libc::setsid(); // detach from compositor's session
            libc::execvp(
                cmd_cstr.as_ptr(),
                [cmd_cstr.as_ptr(), std::ptr::null()].as_ptr(),
            );
            libc::_exit(1);
        }
        // Parent: does not wait — child is orphaned and runs independently.
    }
}
