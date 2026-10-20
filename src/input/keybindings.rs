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
    /// Move the focused window to an adjacent workspace without following.
    MoveWindow(Direction),
    /// Move the focused window to an adjacent workspace and follow it.
    MoveWindowAndFollow(Direction),
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

    pub fn super_ctrl_shift() -> Self {
        Self { super_key: true, ctrl: true, alt: false, shift: true }
    }

    pub fn alt_ctrl_shift() -> Self {
        Self { super_key: false, ctrl: true, alt: true, shift: true }
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
                    SpawnProcess("vicinae toggle".into())),
                b(ModifierMask::alt_only(), Keysym::d,
                    SpawnProcess("vicinae toggle".into())),
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

                // ── Move window across workspaces (send & stay) ──────────────
                b(ModifierMask::super_shift(), Keysym::Left,
                    MoveWindow(Direction::Left)),
                b(ModifierMask::super_shift(), Keysym::Right,
                    MoveWindow(Direction::Right)),
                b(ModifierMask::super_shift(), Keysym::Up,
                    MoveWindow(Direction::Up)),
                b(ModifierMask::super_shift(), Keysym::Down,
                    MoveWindow(Direction::Down)),
                b(ModifierMask::alt_shift(), Keysym::Left,
                    MoveWindow(Direction::Left)),
                b(ModifierMask::alt_shift(), Keysym::Right,
                    MoveWindow(Direction::Right)),
                b(ModifierMask::alt_shift(), Keysym::Up,
                    MoveWindow(Direction::Up)),
                b(ModifierMask::alt_shift(), Keysym::Down,
                    MoveWindow(Direction::Down)),

                // ── Move window across workspaces (send & follow) ─────────────
                b(ModifierMask::super_ctrl_shift(), Keysym::Left,
                    MoveWindowAndFollow(Direction::Left)),
                b(ModifierMask::super_ctrl_shift(), Keysym::Right,
                    MoveWindowAndFollow(Direction::Right)),
                b(ModifierMask::super_ctrl_shift(), Keysym::Up,
                    MoveWindowAndFollow(Direction::Up)),
                b(ModifierMask::super_ctrl_shift(), Keysym::Down,
                    MoveWindowAndFollow(Direction::Down)),
                b(ModifierMask::alt_ctrl_shift(), Keysym::Left,
                    MoveWindowAndFollow(Direction::Left)),
                b(ModifierMask::alt_ctrl_shift(), Keysym::Right,
                    MoveWindowAndFollow(Direction::Right)),
                b(ModifierMask::alt_ctrl_shift(), Keysym::Up,
                    MoveWindowAndFollow(Direction::Up)),
                b(ModifierMask::alt_ctrl_shift(), Keysym::Down,
                    MoveWindowAndFollow(Direction::Down)),

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

    let parts: Vec<CString> = cmd
        .split_whitespace()
        .filter_map(|s| CString::new(s).ok())
        .collect();

    if parts.is_empty() {
        tracing::error!("spawn_process: empty command string");
        return;
    }

    unsafe {
        let pid = libc::fork();
        if pid < 0 {
            tracing::error!("fork() failed");
        } else if pid == 0 {
            // Child: exec the command.  On failure, exit immediately.
            libc::setsid(); // detach from compositor's session
            let mut argv: Vec<*const libc::c_char> = parts.iter().map(|s| s.as_ptr()).collect();
            argv.push(std::ptr::null());

            libc::execvp(argv[0], argv.as_ptr());
            libc::_exit(1);
        }
        // Parent: does not wait — child is orphaned and runs independently.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smithay::input::keyboard::ModifiersState;

    #[test]
    fn test_launcher_keybinding() {
        let config = KeybindingConfig::default();
        let super_mods = ModifiersState {
            logo: true,
            ..Default::default()
        };
        let action = config.lookup(&super_mods, Keysym::d);
        match action {
            Some(KeyAction::SpawnProcess(cmd)) => assert_eq!(cmd, "vicinae toggle"),
            other => panic!("Expected SpawnProcess('vicinae toggle'), got {:?}", other),
        }

        let alt_mods = ModifiersState {
            alt: true,
            ..Default::default()
        };
        let action = config.lookup(&alt_mods, Keysym::d);
        match action {
            Some(KeyAction::SpawnProcess(cmd)) => assert_eq!(cmd, "vicinae toggle"),
            other => panic!("Expected SpawnProcess('vicinae toggle'), got {:?}", other),
        }
    }

    #[test]
    fn test_move_window_bindings() {
        let config = KeybindingConfig::default();
        // Super + Shift + Right -> MoveWindow(Right)
        let super_shift = ModifiersState {
            logo: true,
            shift: true,
            ..Default::default()
        };
        match config.lookup(&super_shift, Keysym::Right) {
            Some(KeyAction::MoveWindow(Direction::Right)) => {}
            other => panic!("Expected MoveWindow(Right), got {:?}", other),
        }

        // Super + Ctrl + Shift + Down -> MoveWindowAndFollow(Down)
        let super_ctrl_shift = ModifiersState {
            logo: true,
            ctrl: true,
            shift: true,
            ..Default::default()
        };
        match config.lookup(&super_ctrl_shift, Keysym::Down) {
            Some(KeyAction::MoveWindowAndFollow(Direction::Down)) => {}
            other => panic!("Expected MoveWindowAndFollow(Down), got {:?}", other),
        }

        // Alt + Shift + Left -> MoveWindow(Left)
        let alt_shift = ModifiersState {
            alt: true,
            shift: true,
            ..Default::default()
        };
        match config.lookup(&alt_shift, Keysym::Left) {
            Some(KeyAction::MoveWindow(Direction::Left)) => {}
            other => panic!("Expected MoveWindow(Left), got {:?}", other),
        }

        // Alt + Ctrl + Shift + Up -> MoveWindowAndFollow(Up)
        let alt_ctrl_shift = ModifiersState {
            ctrl: true,
            alt: true,
            shift: true,
            ..Default::default()
        };
        match config.lookup(&alt_ctrl_shift, Keysym::Up) {
            Some(KeyAction::MoveWindowAndFollow(Direction::Up)) => {}
            other => panic!("Expected MoveWindowAndFollow(Up), got {:?}", other),
        }
    }
}
