# appcoat

A working prototype of an application packaging foundation for macOS, Linux and
Scarlet. The application can use any language or UI framework. The host packager
and optional resource library use ordinary upstream Rust. Nix coordinates builds
when wanted; consumer applications do not need Nix, Cargo, Scarlet or scarlet-ui.
The API and TOML format are provisional. No license has been selected for this
prototype.

## Try the prototype

```sh
./scripts/dev
cargo build --workspace --locked
target/debug/appcoat package --manifest examples/resource-reader/appcoat.toml \
  --platform macos --build --output dist/new-reader.app
target/debug/appcoat run --package dist/new-reader.app -- "space inside" 日本語
target/debug/appcoat resource-root --package dist/new-reader.app
```

On Linux select `--platform linux --output dist/new-reader`. Build outputs must
be new. Existing packages are never replaced. The final package is published only
after assembly and validation; staging is cleaned on failure.

The Rust example also runs directly from a macOS bundle or Linux install tree,
even after relocation and from an unrelated working directory:

```sh
cd /tmp
/absolute/path/to/new-reader.app/Contents/MacOS/resource-reader
```

There is a real C consumer with no Cargo manifest:

```sh
target/debug/appcoat package --manifest examples/custom-c/appcoat.toml \
  --platform macos --build --output dist/new-c.app
target/debug/appcoat run --package dist/new-c.app -- "C argument"
```

Its build command uses `sh` and `cc`; its resource access uses the explicit
launcher environment. Direct launch of this C example requires setting
`APPCOAT_RESOURCE_ROOT` or adding native resource discovery in the consumer.
The prototype does not inject an environment into Finder or desktop launches.
The examples are console processes, not GUI applications.

## Artifact and build contracts

One common definition supplies the stable ID, name, version, executable name and
logical resources. See [the Rust definition](examples/resource-reader/appcoat.toml).

```toml
[app]
id = "org.example.player"
slug = "player"
name = "Player"
version = "0.1.0"
executable = "player"

[build]
command = ["sh", "build-my-app.sh"]
artifact = "build/player"

[[resources]]
source = "assets"
to = "assets"
```

`package` can consume an already built `--artifact FILE`; `[build]` is optional.
`--artifact` is relative to the caller's directory. `build.artifact` and resource
sources are relative to the manifest. Artifact input can live in an external
build directory or Nix store output. Resources must stay inside the manifest
directory, contain portable ASCII filename components, and contain no symlinks.
Destinations reject traversal and file collisions.

The optional `--build` runs exactly the declared argv vector in the manifest's
directory. It sets `APPCOAT_PLATFORM`, `APPCOAT_MANIFEST_DIR`, and, for Scarlet,
`APPCOAT_TARGET`. It removes an inherited target variable for other platforms.
There is no implicit Cargo invocation, shell expansion, dependency resolver,
toolchain switch or Nix invocation. A shell is used only when declared by the app.
The Rust sample adapter reads Cargo's JSON executable report, so a custom
`CARGO_TARGET_DIR` works. Appcoat itself never guesses a Cargo output path.

Output layouts:

| Platform | Executable | Resources | Generated metadata |
| --- | --- | --- | --- |
| macOS | `Contents/MacOS/<executable>` | `Contents/Resources` | `Contents/Info.plist` |
| Linux | `bin/<executable>` | `share/appcoat/<id>/resources` | `share/applications/<id>.desktop` |
| Scarlet | `bin/<executable>` | `resources` | Root `<id>.desktop` with native target fields |

A versioned relative-path receipt records the runtime layout. The macOS receipt
is `Contents/Resources/appcoat-package.toml`; other receipts are at the package root.
Linux output is currently a dedicated application install prefix, not a merger
into `/usr` shared by many appcoat packages. Desktop launch needs its `bin` on
PATH, with its `share` directory on `XDG_DATA_DIRS` for desktop entries and icons.
Extra executable payloads and shared-prefix installation policies are deferred.

### Application icons and desktop entries

```toml
[icons]
macos = "assets/player.icns"
linux = "assets/player.png"
scarlet = "assets/player.png"

[desktop]
open-file = true
new-instance = true
mime-types = ["text/plain"]
categories = ["Development", "TextEditor"]
```

All fields are optional. Icon paths are relative to the manifest, obey resource
containment rules, and must already exist before `--build`. Only the selected
platform's icon is required. Supply ICNS on macOS and PNG on Linux/Scarlet;
Appcoat checks headers, not a complete image decode, and does not convert images.
macOS bundles declare `CFBundleIconFile` and enable high-resolution rendering.
Linux installs the icon under `share/pixmaps/<app-id>.png`; Scarlet places it in
`resources`. Icons cannot overwrite declared resources.

`desktop` applies to Linux and Scarlet. `open-file` adds `%f` to `Exec`, so the
application must handle a file path argument. `new-instance` sets Scarlet's
launcher policy. MIME types and categories are emitted into the desktop entry;
Appcoat does not register them as system defaults or update desktop caches.
macOS file associations are not emitted: handling native document-open events
requires application/framework support beyond command-line arguments.

macOS accepts Mach-O files or executable shebang scripts; Linux accepts ELF or
executable shebang scripts. These are format checks, not architecture, linkage
or deployment compatibility audits. Scarlet checks ELF64, native OSABI, selected
architecture, executable permissions and `/bin/scarlet-ld` for a dynamic
interpreter; all ELF resources must have the same ABI. Its package contains
exactly one root desktop descriptor and no build recipes. The first version
does not bundle dependent libraries unless the macOS option below is enabled. User data and OS libraries or
GPU drivers are not ordinary app resources.

### macOS dynamic libraries

```toml
[macos]
bundle-libraries = true
```

On a macOS packaging host, this copies non-system dylibs and their transitive
dependencies into `Contents/Frameworks`, rewrites their load commands to bundle
relative paths, and ad-hoc signs and verifies the finished app. This removes
absolute build-machine paths such as Nix store references. The original executable
and libraries are not changed. `/usr/lib` and `/System/Library` dependencies stay
provided by the OS. Apple's `otool`, `install_name_tool`, and `codesign` are required.

This option accepts thin Mach-O binaries with absolute, `@loader_path`, or
`@executable_path` dependencies. Unresolved `@rpath` dependencies, external
frameworks, and conflicting library filenames fail without publishing a package.
Resolve those in the consumer build first. Include redistribution licenses for
bundled libraries in the application's resources. Ad-hoc signing is for local use;
Developer ID signing and notarization remain separate release steps.

An application may explicitly substitute an ABI-compatible OS library:

```toml
[macos.system-libraries]
"libiconv.2.dylib" = "/usr/lib/libiconv.2.dylib"
```

These filename mappings are applied before copying dependencies. Appcoat does
not infer ABI compatibility; the consumer must verify it. This is useful when a
build toolchain links a private copy of a library already supplied by macOS.
Libraries may also require data files or plugins: dylib closure alone does not
relocate those resources.


## Optional runtime

The reusable library crates are `appcoat_runtime` and `appcoat_build`.
`appcoat_cli` is the binary package (`appcoat`), and `appcoat_resource_reader`
is the workspace's example executable. Nothing has been published to a registry.
For a consumer next to this checkout:

```toml
[dependencies]
appcoat_runtime = { path = "../appcoat/crates/appcoat_runtime" }
# In a host build tool, only if you want to call the packager as a library:
appcoat_build = { path = "../appcoat/crates/appcoat_build" }
```

Minimal runtime use under `appcoat run`:

```rust
fn main() -> std::io::Result<()> {
    let resources = appcoat_runtime::Resources::from_env()?;
    let bytes = resources.read("greeting.txt")?;
    print!("{}", String::from_utf8_lossy(&bytes));
    Ok(())
}
```

The host build API can package a consumer artifact without invoking a CLI:

```rust
use appcoat_build::{PackageOptions, Platform};
use std::path::{Path, PathBuf};

pub fn package_macos(manifest: &Path, artifact: &Path, output: &Path)
    -> appcoat_build::Result<PathBuf>
{
    appcoat_build::package(PackageOptions {
        manifest,
        platform: Platform::Macos,
        target: None,
        artifact: Some(artifact),
        output,
        build: false,
    })
}
```

`read_definition`, `inspect`, `resource_root`, and `run` are also public build
library APIs. The CLI is just a caller of this library. An independent consumer
with local path dependencies was built and used both public libraries on this Mac.

`appcoat_runtime` has no dependencies. `Resources::new(absolute_root)` is the
portable base API; `Resources::from_env()` reads `APPCOAT_RESOURCE_ROOT`.
`Resources::discover(id)` first honors an explicit root and then uses the OS's
`current_exe` plus the documented macOS/Linux layout and package receipt.
It never uses cwd or argv[0]. Unsupported OSes need an explicit root.
Logical resource lookup rejects traversal and canonicalized symlink escapes.

`appcoat run --package PATH -- ARGS...` runs only a package matching the host,
sets the absolute resource root, preserves cwd, passes argv and returns the child
exit status. `resource-root` prints the same absolute root for any language or
external launcher. Applications can also use their framework's native resource
API and omit the runtime crate entirely. Config, cache and user data remain
separate, application-owned concerns.

## Nix without a mandatory consumer toolchain

```sh
nix build .#appcoat
nix build .#resource-reader       # result/resource-reader.app on this Mac
nix build .#custom-c              # C derivation + the same host packager
./scripts/dev ./scripts/check
nix flake check
```

The default flake pins only nixpkgs. The shell and packager use upstream
`pkgs.rustPlatform`; there is no Scarlet input. `lib.mkArtifactPackage` accepts a
file produced by any derivation and a source tree supplying metadata/resources:

```nix
appcoat.lib.mkArtifactPackage {
  inherit pkgs;
  appcoat = appcoat.packages.${system}.appcoat; # host tool
  src = ./my-app;
  artifact = "${myBuild}/bin/player";          # your own build system/toolchain
  platform = "scarlet";
  target = "aarch64-unknown-scarlet";
  slug = "player";
}
```

The helper never executes the manifest's build command. A consumer that targets
Scarlet can add its own `scarlet-rust-nix` input and build `myBuild` with that
compiler; it still uses the upstream-built host packager. Details and the
reason for this boundary are in [DESIGN.md](DESIGN.md).

## Scarlet example and SDK integration

```sh
SCARLET_RUSTC=/path/to/scarlet-toolchain/bin/rustc \
  target/debug/appcoat package --manifest examples/scarlet/appcoat.toml \
  --platform scarlet --target aarch64-unknown-scarlet --build \
  --output dist/scarlet-reader.app
```

The example is outside the Cargo workspace. The explicit adapter compiles
`appcoat_runtime` and the consumer with the selected fork, in a separate output
directory, without changing host dependencies or compiler selection. Use a
different output for `riscv64gc-unknown-scarlet`. The consumer defaults to the
explicit deployment path `/applications/scarlet-reader.app/resources` until a
Scarlet launcher provides `APPCOAT_RESOURCE_ROOT`; this is an example policy,
not runtime discovery in appcoat.

The resulting prebuilt app is accepted by the current Scarlet SDK:

```sh
cargo scarlet app build --source dist/scarlet-reader.app \
  --target aarch64-unknown-scarlet --output staging/scarlet-reader.app
```

An existing Scarlet image can use its `kind = "app"` layer with this prebuilt
directory. No SDK source changes or additional compiler dependency are needed in
appcoat. Image construction, boot and Scarlet runtime behavior are not tested here.
