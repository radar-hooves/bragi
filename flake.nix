{
  description = "Bragi: the desktop player for your own Jellyfin or OpenSubsonic music server";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
          config = {
            allowUnfreePredicate = pkg: builtins.elem (pkgs.lib.getName pkg) [
              "pnpm"
            ];
            allowBroken = true;  # webkitgtk for Tauri on Linux
          };
        };

        pkg = builtins.fromJSON (builtins.readFile ./package.json);
        # The kit crates are git dependencies of one repository, so one hash
        # covers them all; `scripts/update-kit-hash.sh` sets it from Cargo.lock.
        kitHash = "sha256-9Mnft66pD36+xddsZAgnneSouXVPEd3/d6bh1qZdA1A=";

        # Rust toolchain with Tauri prerequisites
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
          # Release builds produce both macOS architectures from Apple Silicon,
          # so the devShell toolchain must carry the Intel target too.
          targets = pkgs.lib.optionals pkgs.stdenv.isDarwin [
            "aarch64-apple-darwin"
            "x86_64-apple-darwin"
          ];
        };

        # Node.js 22 for frontend
        nodeEnv = pkgs.nodejs_22;

        # Build the package with the newer toolchain too — nixpkgs' pinned rustc
        # can be too old for some deps.
        rustPlatform = pkgs.makeRustPlatform {
          cargo = rustToolchain;
          rustc = rustToolchain;
        };

        # Libraries the wrapped binary needs at runtime (system tray, Linux only).
        runtimeLibs = [ pkgs.libappindicator-gtk3 ];

        # ---------------------------------------------------------------------
        # Installable, importable package:
        #   nix build .#default
        #   nix profile install github:radar-hooves/bragi
        #   inputs.bragi.packages.${system}.default   (NixOS/home-manager module or overlay)
        #
        # Linux packages use WebKitGTK from nixpkgs. macOS uses the devShell
        # to build signed DMGs; no Darwin package output is exposed.
        #
        # @poodle64/design-tokens and @poodle64/ui come from public npmjs like
        # any other dependency. They were vendored as tarballs under vendor/
        # while they lived on private GitHub Packages, which 401s inside a
        # fixed-output derivation that has no credentials; they have published
        # publicly since design-system#19, so the workaround was holding this
        # app two minor versions behind for a constraint that no longer exists.
        # Refresh the pnpmDeps hash whenever pnpm-lock.yaml changes: set it to
        # pkgs.lib.fakeHash, run `nix build`, and paste the reported hash.
        # ---------------------------------------------------------------------
        bragiPackage = rustPlatform.buildRustPackage (finalAttrs: {
          pname = "bragi";
          version = pkg.version;
          src = ./.;

          cargoRoot = "src-tauri";
          buildAndTestSubdir = "src-tauri";
          cargoLock = {
            lockFile = ./src-tauri/Cargo.lock;
            # Registry checksums come from Cargo.lock; only the kit git deps need a hash.
            outputHashes = builtins.listToAttrs (map
              (c: { name = "${c.name}-${c.version}"; value = kitHash; })
              (builtins.filter
                (c: pkgs.lib.hasPrefix "git+https://github.com/radar-hooves/app-factory" (c.source or ""))
                (builtins.fromTOML (builtins.readFile ./src-tauri/Cargo.lock)).package));
          };

          pnpmDeps = pkgs.fetchPnpmDeps {
            inherit (finalAttrs) pname version src;
            fetcherVersion = 3;
            hash = "sha256-XIpWNxCocY38ysSLlGWiRmIxlIC896/qUrFRpfcUioQ=";
          };

          nativeBuildInputs = with pkgs; [
            cargo-tauri.hook
            nodejs_22
            pnpmConfigHook
            pnpm
            pkg-config
            cmake
            git
            makeWrapper
          ] ++ lib.optionals stdenv.isLinux [ wrapGAppsHook4 ];

          buildInputs = with pkgs;
            [ openssl ]
            ++ lib.optionals stdenv.isLinux [
              webkitgtk_4_1
              glib
              glib-networking
              libsecret
              libappindicator-gtk3
              alsa-lib
              librsvg
            ]
            ++ lib.optionals stdenv.isDarwin [ libiconv ];

          # Linux sandbox packages use plain cargo. macOS DMGs are signed
          # by the declared host runner, outside this package derivation.
          preBuild = ''
            substituteInPlace src-tauri/tauri.conf.json \
              --replace-fail '"runner": "../scripts/cargo-codesign.sh",' ""
          '';

          # mpv is the audio engine, spawned as a subprocess, so it MUST be on
          # PATH. The WebKit DMABUF workaround and GPU driver exposure are
          # Linux/NVIDIA-specific (otherwise WebKitGTK falls back to llvmpipe).
          # tauriInstallHook lays out a $out/bin/bragi executable on Linux but
          # only a $out/Applications/Bragi.app bundle on Darwin.
          postFixup = ''
            wrapProgram ${
              if pkgs.stdenv.isDarwin then
                "$out/Applications/Bragi.app/Contents/MacOS/bragi"
              else
                "$out/bin/bragi"
            } \
              --prefix PATH : ${
                pkgs.lib.makeBinPath ([ pkgs.mpv ] ++ pkgs.lib.optional pkgs.stdenv.isLinux pkgs.glib.bin)
              } \
              ${pkgs.lib.optionalString pkgs.stdenv.isLinux ''
                --prefix LD_LIBRARY_PATH : ${pkgs.lib.makeLibraryPath runtimeLibs}:/run/opengl-driver/lib \
                --set WEBKIT_DISABLE_DMABUF_RENDERER 1
              ''}
          '';

          doCheck = false; # no headless audio/display in the sandbox

          meta = with pkgs.lib; {
            description = "The desktop player for your own Jellyfin or OpenSubsonic music server, browsable even offline";
            homepage = "https://github.com/radar-hooves/bragi";
            license = licenses.mit;
            platforms = platforms.linux;
            mainProgram = "bragi";
          };
        });

      in {
        packages = pkgs.lib.optionalAttrs pkgs.stdenv.isLinux {
          default = bragiPackage;
          bragi = bragiPackage;
        };

        devShells.default = pkgs.mkShell {
          # Linux/WebKitGTK on NVIDIA+Wayland: the DMABUF renderer crashes the
          # app with "Error 71 (Protocol error) dispatching to Wayland display"
          # (seen on WebKitGTK 2.50.x), so disable it. Remove this once
          # upstream fixes it. WebKit then composites via the non-DMABUF GL path,
          # which uses the real GPU *provided* the NVIDIA driver is reachable
          # (see LD_LIBRARY_PATH below). Without the driver on the path it
          # silently falls back to llvmpipe software rendering — that was the
          # cause of the UI slowness.
          WEBKIT_DISABLE_DMABUF_RENDERER = pkgs.lib.optionalString pkgs.stdenv.isLinux "1";

          # Make appindicator + the NVIDIA GL driver available at runtime.
          # /run/opengl-driver/lib is where NixOS exposes the real GPU driver
          # (libGLX_nvidia, libEGL_nvidia, libnvidia-glcore). Without this,
          # WebKitGTK cannot find them and renders on the CPU via llvmpipe.
          LD_LIBRARY_PATH = pkgs.lib.optionalString pkgs.stdenv.isLinux
            (pkgs.lib.makeLibraryPath (with pkgs; [
              libappindicator-gtk3
            ]) + ":/run/opengl-driver/lib");

          packages = with pkgs; [
            # Rust / Tauri
            rustToolchain
            cargo
            rustc
            rust-analyzer

            # Tauri dependencies (platform-specific)
            openssl
            pkg-config
          ] ++ lib.optionals stdenv.isLinux [
            # Linux-only Tauri dependencies
            webkitgtk_4_1
            libappindicator-gtk3
            librsvg
            alsa-lib
          ] ++ lib.optionals stdenv.isDarwin [
            # macOS: applesoft libraries (via Xcode) are used automatically
            libiconv
          ] ++ [
            # Frontend
            nodeEnv
            pnpm

            # Audio playback engine (spawned as a subprocess, controlled over JSON IPC)
            mpv

            # Build tools
            cmake

            # Release publishing (CI runs `gh release upload` inside this
            # devShell; the self-hosted runner daemons have a bare PATH)
            gh

            # Useful utilities (Linux-only)
          ] ++ lib.optionals stdenv.isLinux [
            glib
            libsecret
          ];

          shellHook = ''
            echo "🎵 Bragi Development Environment"
            echo "================================"
            echo "  Rust: $(rustc --version)"
            echo "  Node: $(node --version)"
            echo "  pnpm: $(pnpm --version)"
            echo ""
            echo "Commands:"
            echo "  pnpm dev        - Start Vite dev server"
            echo "  pnpm tauri dev  - Start full Tauri app"
          '';
        };
      });
}
