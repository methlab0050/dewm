//! dewm — Wayland compositor entry point.
//!
//! Parses CLI arguments, initialises tracing, then delegates to the
//! chosen backend (Winit for nested development, udev/DRM for native).

mod backend;
mod gesture;
mod input;
mod overview;
mod rendering;
mod state;
mod workspace;
mod xwayland;

use anyhow::Result;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// Run as a Wayland window inside an existing compositor (development).
    Winit,
    /// Run natively on DRM/KMS hardware (production).
    Udev,
}

fn main() -> Result<()> {
    // Initialise structured logging.  Set RUST_LOG=dewm=debug to increase verbosity.
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "dewm=info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let backend = parse_backend_arg();
    tracing::info!(?backend, "Starting dewm");

    match backend {
        Backend::Winit => backend::winit::run()?,
        Backend::Udev  => backend::udev::run()?,
    }

    Ok(())
}

fn parse_backend_arg() -> Backend {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--backend" {
            match args.next().as_deref() {
                Some("winit") => return Backend::Winit,
                Some("udev")  => return Backend::Udev,
                other => {
                    eprintln!("Unknown backend: {:?}. Use 'winit' or 'udev'.", other);
                    std::process::exit(1);
                }
            }
        }
    }
    // Default to Winit for developer ergonomics.
    tracing::info!("No --backend specified, defaulting to 'winit' (nested mode)");
    Backend::Winit
}
