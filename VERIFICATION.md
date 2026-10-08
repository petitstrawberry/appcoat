# Verified prototype — 2026-10-08

Host: Apple Silicon macOS (`aarch64-apple-darwin`).

## Reusable crates and toolchain boundary

The Cargo workspace contains the library crates `appcoat_build` and
`appcoat_runtime`, the CLI binary package `appcoat_cli` (binary `appcoat`), and
the example binary `appcoat_resource_reader`. The runtime has no dependencies;
the build library uses serde/toml. The host workspace has no Scarlet packages,
compiler selection, target-specific dependencies, or git dependency sources.
The flake lock contains only nixpkgs and the root node.

The complete workspace compiled with upstream Rust 1.95.0:
`rustc 1.95.0 (59807616e 2026-04-14)`, LLVM 21.1.8, host
`aarch64-apple-darwin`. `rustc --print target-list` returned no Scarlet target.
An independent, temporary Cargo consumer used local path dependencies on both
public crates, read the resource with `Resources::from_env`, and assembled a
working macOS app through `appcoat_build::package`.

## Host checks that passed

- `./scripts/dev ./scripts/check`: format, clippy with warnings denied, 9 Rust
  unit tests (8 build + 1 runtime), all workspace binaries, and 5 end-to-end tests.
- `nix flake show`: outputs evaluated for four host systems.
- `nix build .#appcoat .#resource-reader .#custom-c`: built upstream host CLI,
  Rust sample and a separate C derivation, then packaged both application outputs.
- `nix flake check`: all checks for aarch64-darwin passed, including real Rust/C
  package launches and reading their bundled resources. Other host systems were
  omitted as incompatible with this Mac.
- `/usr/bin/plutil -lint` accepted both generated macOS Info.plists.
- Direct execution from `/tmp` read the Rust app's bundled resource; `appcoat run`
  read it too and preserved cwd plus arguments containing spaces and Japanese.
- Relocated packages and a custom Cargo output directory passed the end-to-end
  tests. The C example's `sh` + `cc` build read its resource through the launcher.
- Unit tests covered resource traversal/symlink escape, duplicate resources,
  staging cleanup/no overwrite, staging recursion prevention, receipt escapes,
  incompatible host launch, native Scarlet headers and interpreter bounds.

The locally produced verification artifacts were:

- `dist/resource-reader.app`
- `dist/custom-c.app`
- `dist/scarlet-reader.app` (AArch64)
- `dist/riscv64/scarlet-reader.app` (RISC-V64)

Useful local evidence, deliberately ignored by Git: `target/check.log`,
`target/nix-check.log`, `target/nix-build.log`, `target/nix-outputs.txt`,
`target/cargo-metadata.json` and `target/downstream-api.log`. Generated artifacts,
local validation snapshots and dependency caches are excluded from the published
source tree.

## Scarlet cross-build and SDK acceptance

Only the standalone `examples/scarlet/build.sh` adapter selected the explicitly
provided Scarlet compiler (Rust 1.94.0-nightly, LLVM 21.1.8). It compiled the std-only runtime and consumer for both targets;
the default Nix/Cargo graph never selected it.

The packaged executables had ELFOSABI_SCARLET 83 and machine 183 (AArch64) or
243 (RISC-V64). The inspected SDK source was copied into an ignored local
validation directory and built as a host tool with upstream Rust. Its
`cargo-scarlet app build --source <prebuilt>`
accepted and copied both generated apps into fresh validation destinations.
The reference SDK and consumer checkouts were not modified or included in this
repository.

## Remaining limits

Neither Scarlet binary has been booted or executed on Scarlet. Linux layout is
unit-tested and its flake outputs evaluated, but Linux executables, desktop
launch and Linux resource discovery were not run on this Mac. x86_64 macOS was
not built. Native header validation is not a complete executable/dependency audit.
No image build, GUI integration, shared Linux prefix installation, dependency
closure, signing, notarization, release or crate registry publication was performed.

The C example currently needs `appcoat run` or an explicit root. The Scarlet
example defaults to its documented `/applications/scarlet-reader.app/resources`
deployment path until an OS launcher supplies a root. These are explicit example
policies rather than an implemented cross-platform launcher integration.
