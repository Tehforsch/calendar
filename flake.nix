{
  description = "Calendar - A keyboard-first terminal calendar";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs = {
        nixpkgs.follows = "nixpkgs";
      };
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" ];
        };

        rustPlatform = pkgs.makeRustPlatform {
          cargo = rustToolchain;
          rustc = rustToolchain;
        };

        calendar = rustPlatform.buildRustPackage {
          pname = "calendar";
          version = "0.1.0";
          src = ./.;

          cargoLock = {
            lockFile = ./Cargo.lock;
          };

          meta = with pkgs.lib; {
            description = "A keyboard-first terminal calendar";
            homepage = "https://github.com/tehforsch/calendar";
            maintainers = [ ];
            mainProgram = "calendar";
          };
        };
      in
      {
        packages = {
          default = calendar;
          calendar = calendar;
        };

        apps = {
          default = flake-utils.lib.mkApp {
            drv = calendar;
          };
          calendar = flake-utils.lib.mkApp {
            drv = calendar;
          };
        };

        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            rustToolchain
            cargo-watch
            cargo-edit
          ];

          RUST_SRC_PATH = rustToolchain + "/lib/rustlib/src/rust/library";
        };
      });
}
