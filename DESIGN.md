# Prototype boundary

The first acceptance criterion is toolchain isolation. The host CLI and macOS/
Linux runtime must compile using upstream Rust; a Scarlet compiler is selected
only by a Scarlet consumer build. The prototype demonstrates that boundary with
actual binaries, not just target cfgs.

## Evidence from the local projects

Inspected on 2026-10-08: the existing Scarlet SDK app packaging implementation,
Scarlet's native application documentation, and a reference consumer's Scarlet
install script, metadata build script, macOS bundler and Nix development shell.
The findings below describe their contracts without shipping their source code
or local checkout information.

The SDK already separates application assembly from image layers. Its app recipes
support Cargo, scripts and prebuilt artifacts. Native app validation is an
artifact contract: portable paths, one desktop entry, native ELF ABI/architecture,
permissions, and the interpreter. The reference consumer uses a script recipe,
keeps its own Scarlet binary audit, derives identity from the recipe in build.rs
and has a
separate macOS bundler that reads Cargo JSON. Its development shell currently
selects the Scarlet compiler even for the host; appcoat does not copy that coupling.

The reference checkouts were inspected read-only and were not modified for this
prototype. Appcoat ships its own implementation and examples.

## Chosen shape

- `appcoat_build`: host metadata/assembly and explicit command adapter, upstream
  Rust with only serde/toml dependencies.
- `appcoat_cli`: `package`, `run` and `resource-root`; upstream Rust.
- `appcoat_runtime`: optional std-only logical resource access. No GUI, SDK or
  Scarlet dependency; explicit-root use also compiles with the fork.
- Nix: a pinned upstream development shell, host derivations, and an exported
  helper receiving a consumer-produced artifact. Consumer toolchains remain inputs
  to the consumer derivation.

Making Nix mandatory could represent every build as a derivation and centralize
dependency closure. It would also make plain Cargo/CMake/custom-command consumers
adopt Nix and tie the public contract to its evaluator/store. The existing SDK
does not require that boundary for script/prebuilt apps. The implemented artifact
contract is sufficient for both local commands and Nix derivations, so mandatory
Nix is an unnecessary restriction at this stage.

## Isolation pitfalls

Cfg gating code alone does not isolate dependency resolution or toolchains.
Target-specific Cargo dependencies still participate in the workspace lockfile
and can require fetching Scarlet sources during a native build. A common flake
input or rust-toolchain file selecting the fork would couple evaluation and all
host commands before runtime cfgs matter. `check-cfg` only controls diagnostics;
it does not teach upstream rustc an unknown target or supply a target std library.
Upstream Rust 1.95.0's target list on this Mac contains no Scarlet target.

Accordingly, the default workspace has no Scarlet target-specific dependencies,
no unknown-target tables, no fork sources in Cargo.lock, no fork-selection config,
and no Scarlet Nix input. The separate Scarlet example invokes its compiler
explicitly rather than contaminating this graph. This also avoids mixing compiler
outputs in Cargo target directories. Applications needing Scarlet-only Cargo
dependencies should keep that consumer workspace/lock separate from host appcoat
tooling until a deliberate isolated dependency strategy is implemented.

## Concrete Scarlet adapters

1. Existing audited build script or SDK build: pass the finished native executable
   to host `appcoat package --artifact`. This preserves the consumer's existing
   audited build.
2. Explicit build command: the included standalone rustc example demonstrates a
   selected fork and separate output for each invocation. Only consumer/runtime
   binaries use it; both supported architectures have been built on this Mac.
3. Nix consumer cross derivation: select a Scarlet compiler in that derivation and
   pass its executable to `mkArtifactPackage`. The host packager stays upstream.
4. SDK image integration: consume the finished `.app` as an existing prebuilt
   `kind = "app"` layer. The current SDK's app validator accepted both outputs.

No second kernel/rootfs/image system was introduced. Native dynamic linking,
driver selection, and dependency audits remain the platform/consumer build's
responsibility. Appcoat's native header validation is intentionally small and
must be kept compatible with the SDK; the SDK acceptance check provides evidence
for this prototype's inspected revision, not a permanent compatibility promise.
