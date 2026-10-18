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

use anyhow::Result;

pub fn run() -> Result<()> {
    // The udev backend follows the same structure as the Winit backend but uses:
    //
    //   smithay::backend::drm       — KMS/DRM output
    //   smithay::backend::udev      — device hotplug via udev
    //   smithay::backend::libinput  — real input device access
    //   smithay::backend::session   — libseat session for VT switching
    //
    // Implementation is deferred until the Winit backend is stable.
    // The overall structure will mirror winit.rs with:
    //   1. LibSeatSession::new()
    //   2. UdevBackend::new(session)  [for GPU device discovery]
    //   3. Libinput device enumeration
    //   4. DRM device + connector detection
    //   5. GbmDevice + GlesRenderer initialisation
    //   6. Same calloop event loop as winit.rs

    tracing::warn!("udev/DRM backend is not yet implemented. Use --backend winit for development.");
    Err(anyhow::anyhow!("udev backend: not yet implemented"))
}
