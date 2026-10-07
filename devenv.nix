{pkgs, ...}: {
  # https://devenv.sh/packages/
  packages = with pkgs; [
    alejandra
  ];

  # https://devenv.sh/languages/
  languages.nix.enable = true;
}
