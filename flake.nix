{
  description = "pendulum";

  inputs = {
    fenix = {
      url = "github:nix-community/fenix/3116ee073ab3931c78328ca126224833c95e6227";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
    fenix,
  }:
    flake-utils.lib.eachDefaultSystem (system: let
      overlays = [fenix.overlays.default];
      pkgs = import nixpkgs {inherit overlays system;};

      rustPkg = fenix.packages.${system}.latest.withComponents [
        "rust-src"
        "rustc-dev"
        "llvm-tools-preview"
        "cargo"
        "clippy"
        "rustc"
        "rustfmt"
        "rust-analyzer"
      ];
      rustPlatform = pkgs.makeRustPlatform {
        cargo = rustPkg;
        rustc = rustPkg;
      };
      buildInputs = with pkgs; [
        xorg.libX11
        xorg.libXcursor
        xorg.libXrandr
        xorg.libXi
        vulkan-loader
        vulkan-tools
        wayland
        libxkbcommon
      ];
    in rec {
      pendulum = rustPlatform.buildRustPackage {
        pname = "pendulum";
        version = "0.0.0";
        src = ./.;
        cargoHash = "";
      };
      apps.default = {
        type = "app";
        program = "${pendulum}/bin/runner";
      };
      devShell = with pkgs;
        mkShell {
          nativeBuildInputs = [rustPkg];
          LD_LIBRARY_PATH = "${lib.makeLibraryPath buildInputs}";
        };
    });
}
