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
    (if pkgs ? vicinae then pkgs.vicinae else if inputs ? vicinae then inputs.vicinae.packages.${pkgs.stdenv.system}.default else null)

            (rustPlatform.buildRustPackage {
          pname = "rustmcp";
          version = "1.0.0";
          src = inputs.rust-mcp;
          cargoLock.lockFile = "${inputs.rust-mcp}/Cargo.lock";
          nativeBuildInputs = [ makeWrapper ];
          postPatch = ''
            substituteInPlace src/main.rs \
              --replace-fail 'println!("Starting Rust MCP Server");' 'eprintln!("Starting Rust MCP Server");' \
              --replace-fail 'println!("Server running on stdio transport...");' 'eprintln!("Server running on stdio transport...");'
          '';
          postInstall = ''
            wrapProgram $out/bin/rustmcp \
              --set RUST_ANALYZER_PATH "${rust-analyzer}/bin/rust-analyzer" \
              --prefix PATH : ${lib.makeBinPath [ rust-analyzer ]}
          '';
        })
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
    if [ -t 1 ]; then
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
    fi
  '';

  # ─── Tests ───────────────────────────────────────────────────────────────────
  enterTest = ''
    echo "Running unit tests"
    cargo test
  '';
}
