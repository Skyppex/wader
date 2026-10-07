# The source to build: wader with rill's source inside it, and the path
# dependency on `..` pointed there instead.
{
  pkgs,
  wader,
  rill,
}:
pkgs.runCommand "wader-src" {} ''
  cp -r ${wader} $out
  chmod -R +w $out
  cp -r ${rill} $out/rill
  substituteInPlace $out/Cargo.toml --replace-fail 'path = ".."' 'path = "rill"'
''
