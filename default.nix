{
  src,
  naersk,
  pkgs,
  release ? false,
}:
naersk.buildPackage {
  name = "wader";
  inherit src;
  doCheck = false;

  # naersk's own switch: it defaults to a release build, so passing
  # `--release` in `cargoBuildFlags` alone cannot turn one off.
  inherit release;
}
