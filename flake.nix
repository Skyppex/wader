{
  description = "wader is a language server for rill";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    naersk.url = "github:nix-community/naersk";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    # wader depends on the rill crate by path (`..`). Only its source is
    # needed; for a local rill, pass `--override-input rill git+file:..`.
    rill = {
      url = "github:skyppex/rill";
      flake = false;
    };
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
    naersk,
    fenix,
    rill,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (system: let
      pkgs = import nixpkgs {inherit system;};
      fenixLib = fenix.packages.${system};
      toolchain = with fenixLib;
        combine [
          (stable.withComponents [
            "rustc"
            "cargo"
            "rustfmt"
            "clippy"
            "rust-src"
            "rust-docs"
            "rust-std"
            "rust-analyzer"
          ])
        ];

      naerskLib = (pkgs.callPackage naersk {}).override {
        cargo = toolchain;
        rustc = toolchain;
      };

      src = import ./source.nix {
        inherit pkgs rill;
        wader = self;
      };

      waderPackage = {release}:
        import ./default.nix {
          inherit src;
          naersk = naerskLib;
          inherit pkgs;
          inherit release;
        };

      checks = import ./checks.nix {
        inherit src;
        naersk = naerskLib;
        pkgs = pkgs;
      };

      apps = import ./apps.nix {
        pkgs = pkgs;
      };
    in {
      packages = rec {
        default = debug;
        debug = waderPackage {release = false;};
        release = waderPackage {release = true;};
      };

      devShells.default = pkgs.mkShell {
        packages = with pkgs; [
          toolchain
          nixd
          alejandra
        ];
        env.RUST_SRC_PATH = "${toolchain}/lib/rustlib/src/rust/library";
      };

      checks = checks;

      apps = apps;

      formatter = pkgs.writeShellApplication {
        name = "fmt";
        runtimeInputs = [pkgs.rustfmt pkgs.cargo];
        text = "cargo fmt";
      };
    });
}
