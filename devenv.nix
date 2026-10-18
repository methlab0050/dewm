{ pkgs, lib, config, inputs, ... }:

{
  # ─── Packages ────────────────────────────────────────────────────────────────
  packages = with pkgs; [
    # Build toolchain
    rustup
    pkg-config

    # Wayland display server protocol libraries
    wayland
    wayland-protocols
    wayland-utils

    # Input
    libinput
    libxkbcommon

    # OpenGL / rendering
    mesa
    libGL
    egl-wayland

    # Session management (for DRM/KMS udev backend)
    seatd
    udev

    # X11 compatibility layer
    xwayland

    # Test clients for the nested session
    foot
    git
  ];

  # ─── Environment ─────────────────────────────────────────────────────────────

  # Expose system GL/Wayland libs to the Rust linker
  env.LD_LIBRARY_PATH = lib.makeLibraryPath (with pkgs; [
    wayland
    libxkbcommon
    mesa
    libGL
    libinput
    udev
    seatd
  ]);

  # PKG_CONFIG path so pkg-config can find .pc files when building Smithay
  env.PKG_CONFIG_PATH = lib.makeSearchPathOutput "dev" "lib/pkgconfig" (with pkgs; [
    wayland
    wayland-protocols
    libxkbcommon
    libinput
    mesa
    udev
  ]);

  # ─── Rust toolchain ──────────────────────────────────────────────────────────
  languages.rust.enable  = true;
  # languages.rust.channel = "stable";

  # ─── Shell greeting ──────────────────────────────────────────────────────────
  enterShell = ''
    echo ""
    echo "  ╔═══════════════════════════════════════════════╗"
    echo "  ║   dewm dev environment ready                  ║"
    echo "  ╠═══════════════════════════════════════════════╣"
    echo "  ║  Build:         cargo build                   ║"
    echo "  ║  Run (nested):  cargo run -- --backend winit  ║"
    echo "  ║  Run (native):  cargo run -- --backend udev   ║"
    echo "  ║  Lint:          cargo clippy                  ║"
    echo "  ╚═══════════════════════════════════════════════╝"
    echo ""
  '';

  # ─── Tests ───────────────────────────────────────────────────────────────────
  enterTest = ''
    echo "Running unit tests"
    cargo test
  '';
}
