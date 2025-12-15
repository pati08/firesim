{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };
  outputs = { self, nixpkgs, rust-overlay, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let 
        overlays = [
          (import rust-overlay)           
          (self: super: {
            wasm-bindgen-cli =
              super.callPackage
              ({ buildWasmBindgenCli
              , fetchCrate
              , rustPlatform
              , lib
              ,
            }:
            buildWasmBindgenCli rec {
              src = fetchCrate {
                pname = "wasm-bindgen-cli";
                version = "0.2.106";
                hash = "sha256-M6WuGl7EruNopHZbqBpucu4RWz44/MSdv6f0zkYw+44=";
              };
              cargoDeps =
                rustPlatform.fetchCargoVendor
                {
                  inherit src;
                  inherit (src) pname version;
                  hash = "sha256-ElDatyOwdKwHg3bNH/1pcxKI7LXkhsotlDPQjiLHBwA=";
                };
              })
              { };
            })
          ];
          pkgs = import nixpkgs {
            inherit system overlays;
          };
      in {
        devShells.default = pkgs.mkShell rec {
          nativeBuildInputs = with pkgs; [
            pkg-config
          ];
          buildInputs = with pkgs; [
            udev
            xorg.libX11 xorg.libXcursor xorg.libXi xorg.libXrandr
            libxkbcommon wayland
            wasm-bindgen-cli
            simple-http-server
            wasm-pack
            (rust-bin.nightly.latest.default.override {
              extensions = [ "rust-src" "rust-analyzer" "rustfmt" ];
              targets = [ "wasm32-unknown-unknown" ];
            })
          ];
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath buildInputs;
        };
      }
      );
    }
