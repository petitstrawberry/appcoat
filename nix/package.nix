# Artifact may be produced by Cargo, CMake, a custom tool, or a separate Scarlet toolchain.
# `src` supplies the manifest and resources. Nix never invokes the manifest build.command.
{ pkgs, appcoat, src, artifact, platform, slug, manifest ? "appcoat.toml", target ? null }:
assert builtins.elem platform [ "macos" "linux" "scarlet" ];
assert (platform == "scarlet") == (target != null);
let name = if platform == "linux" then slug else "${slug}.app";
in pkgs.runCommand "${slug}-${platform}" { nativeBuildInputs = [ appcoat ]; } ''
  mkdir -p "$out"
  appcoat package \
    --manifest ${pkgs.lib.escapeShellArg "${src}/${manifest}"} \
    --platform ${pkgs.lib.escapeShellArg platform} \
    --artifact ${pkgs.lib.escapeShellArg (toString artifact)} \
    --output "$out/"${pkgs.lib.escapeShellArg name} \
    ${pkgs.lib.optionalString (target != null) "--target ${pkgs.lib.escapeShellArg target}"}
''
