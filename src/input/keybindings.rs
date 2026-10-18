//! Keybinding configuration and dispatch.
//!
//! # Bound keys
//!
//! | Binding                  | Action                                 |
//! |--------------------------|----------------------------------------|
//! | `Super + D`              | Launch Vicinae app launcher            |
//! | `Super + Ctrl + Left`    | Navigate workspace Left                |
//! | `Super + Ctrl + Right`   | Navigate workspace Right               |
//! | `Super + Ctrl + Up`      | Navigate workspace Up                  |
//! | `Super + Ctrl + Down`    | Navigate workspace Down                |
//! | `Super + Tab`            | Toggle workspace overview              |
//! | `Super + Q`              | Close focused window                   |
//! | `Alt + F4`               | Close focused window                   |

use smithay::input::keyboard::{KeyboardHandler, Keysym, ModifiersState};

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
                // ── App launcher ──────────────────────────────────────────────
                b(ModifierMask::super_only(), Keysym::d,
                    SpawnProcess("vicinae".into())),

                // ── Workspace navigation (keyboard fallback) ──────────────────
                b(ModifierMask::super_ctrl(), Keysym::Left,
                    NavigateWorkspace(Direction::Left)),
                b(ModifierMask::super_ctrl(), Keysym::Right,
                    NavigateWorkspace(Direction::Right)),
                b(ModifierMask::super_ctrl(), Keysym::Up,
                    NavigateWorkspace(Direction::Up)),
                b(ModifierMask::super_ctrl(), Keysym::Down,
                    NavigateWorkspace(Direction::Down)),

                // ── Overview ─────────────────────────────────────────────────
                b(ModifierMask::super_only(), Keysym::Tab,
                    ToggleOverview),

                // ── Window management ─────────────────────────────────────────
                b(ModifierMask::super_only(), Keysym::q,  CloseFocused),
                b(ModifierMask::alt_only(),   Keysym::F4, CloseFocused),
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
