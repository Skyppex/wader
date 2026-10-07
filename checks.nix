{
  src,
  naersk,
  pkgs,
}: {
  cargo-check = naersk.buildPackage {
    inherit src;
    mode = "check";
  };

  cargo-test = naersk.buildPackage {
    inherit src;
    mode = "test";
  };

  cargo-clippy = naersk.buildPackage {
    inherit src;
    mode = "clippy";
  };

  # Without `--all`, which would also check rill's source.
  cargo-fmt =
    pkgs.runCommand "cargo-fmt-check" {
      buildInputs = [pkgs.rustfmt pkgs.cargo];
    } ''
      cp -r ${src} ./source
      chmod -R +w ./source
      cd ./source
      cargo fmt -- --check
      touch $out
    '';
}
