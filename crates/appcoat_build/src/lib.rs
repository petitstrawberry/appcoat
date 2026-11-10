//! Artifact-first packaging. This crate runs on an upstream Rust host for every target.
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
    time::{SystemTime, UNIX_EPOCH},
};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn error(message: impl Into<String>) -> Box<dyn std::error::Error> {
    io::Error::other(message.into()).into()
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Macos,
    Linux,
    Scarlet,
}

impl Platform {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "macos" => Ok(Self::Macos),
            "linux" => Ok(Self::Linux),
            "scarlet" => Ok(Self::Scarlet),
            _ => Err(error("platform must be macos, linux or scarlet")),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Macos => "macos",
            Self::Linux => "linux",
            Self::Scarlet => "scarlet",
        }
    }
    pub fn is_host(self) -> bool {
        match self {
            Self::Macos => cfg!(target_os = "macos"),
            Self::Linux => cfg!(target_os = "linux"),
            Self::Scarlet => false,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub app: Metadata,
    pub build: Option<BuildCommand>,
    #[serde(default)]
    pub icons: Icons,
    #[serde(default)]
    pub desktop: Desktop,
    #[serde(default)]
    pub resources: Vec<Resource>,
}

/// Manifest-relative, pre-rendered native icons. Appcoat does not run image tools.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Icons {
    pub macos: Option<PathBuf>,
    pub linux: Option<PathBuf>,
    pub scarlet: Option<PathBuf>,
}
impl Icons {
    fn for_platform(&self, platform: Platform) -> Option<&Path> {
        match platform {
            Platform::Macos => self.macos.as_deref(),
            Platform::Linux => self.linux.as_deref(),
            Platform::Scarlet => self.scarlet.as_deref(),
        }
    }
}

/// Desktop-entry integration. macOS document events require a separate OS adapter.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Desktop {
    pub open_file: bool,
    pub new_instance: bool,
    pub mime_types: Vec<String>,
    pub categories: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub version: String,
    pub executable: String,
}

/// An explicit argv vector, not an implicit Cargo invocation or interpolated shell.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildCommand {
    #[serde(default)]
    pub command: Vec<String>,
    pub artifact: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resource {
    pub source: PathBuf,
    pub to: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    pub format: u32,
    pub platform: Platform,
    pub target: Option<String>,
    pub id: String,
    pub executable: PathBuf,
    pub resources: PathBuf,
}

pub struct PackageOptions<'a> {
    pub manifest: &'a Path,
    pub platform: Platform,
    pub target: Option<&'a str>,
    pub artifact: Option<&'a Path>,
    pub output: &'a Path,
    pub build: bool,
}

pub fn read_definition(manifest: &Path) -> Result<Definition> {
    let definition: Definition = toml::from_str(&fs::read_to_string(manifest)?)?;
    let app = &definition.app;
    for (key, value) in [
        ("id", &app.id),
        ("slug", &app.slug),
        ("executable", &app.executable),
    ] {
        if !token(value) {
            return Err(error(format!(
                "app.{key} must be a portable filename token"
            )));
        }
    }
    if app.slug.bytes().any(|b| b.is_ascii_uppercase()) {
        return Err(error("app.slug must be lowercase"));
    }
    for value in [&app.name, &app.version] {
        if value.is_empty() || value.chars().any(char::is_control) {
            return Err(error(
                "name/version must be nonempty text without control characters",
            ));
        }
    }
    for resource in &definition.resources {
        relative(&resource.to)?;
    }
    for mime in &definition.desktop.mime_types {
        let Some((kind, subtype)) = mime.split_once('/') else {
            return Err(error("desktop.mime-types must contain type/subtype"));
        };
        if ![kind, subtype].iter().all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"!#$&^_.+-".contains(&b))
        }) {
            return Err(error("invalid desktop MIME type"));
        }
    }
    if definition
        .desktop
        .categories
        .iter()
        .any(|category| !token(category))
    {
        return Err(error("invalid desktop category"));
    }
    Ok(definition)
}

fn token(value: &str) -> bool {
    !value.is_empty()
        && !matches!(value, "." | "..")
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
fn relative(value: &str) -> Result<&Path> {
    if value.is_empty() || !value.split('/').all(token) {
        return Err(error(format!(
            "expected portable relative path without traversal: {value}"
        )));
    }
    Ok(Path::new(value))
}
fn relative_path(path: &Path) -> Result<&Path> {
    relative(path.to_str().ok_or_else(|| error("path must be UTF-8"))?)
}

fn resolve_resource(root: &Path, source: &Path) -> Result<PathBuf> {
    relative_path(source)?;
    let path = root.join(source);
    let mut current = root.to_path_buf();
    for component in source.components() {
        current.push(component);
        if fs::symlink_metadata(&current)?.file_type().is_symlink() {
            return Err(error("resource symlinks are unsupported"));
        }
    }
    if !path.canonicalize()?.starts_with(root) {
        return Err(error("resource escapes manifest directory"));
    }
    Ok(path)
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(source)?;
    if metadata.file_type().is_symlink() {
        return Err(error("package symlinks are unsupported"));
    }
    if metadata.is_dir() {
        if destination.is_file() {
            return Err(error("resource directory collides with a file"));
        }
        fs::create_dir_all(destination)?;
        let mut entries = fs::read_dir(source)?.collect::<std::result::Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            if !token(
                entry
                    .file_name()
                    .to_str()
                    .ok_or_else(|| error("resource filename must be UTF-8"))?,
            ) {
                return Err(error("resource filenames must be portable tokens"));
            }
            copy_tree(&entry.path(), &destination.join(entry.file_name()))?;
        }
    } else if metadata.is_file() {
        if fs::symlink_metadata(destination).is_ok() {
            return Err(error(format!(
                "resource collision: {}",
                destination.display()
            )));
        }
        fs::create_dir_all(
            destination
                .parent()
                .ok_or_else(|| error("missing destination parent"))?,
        )?;
        fs::copy(source, destination)?;
    } else {
        return Err(error("unsupported package file type"));
    }
    Ok(())
}

pub fn package(options: PackageOptions<'_>) -> Result<PathBuf> {
    let manifest = options.manifest.canonicalize()?;
    let root = manifest
        .parent()
        .ok_or_else(|| error("missing manifest directory"))?;
    let definition = read_definition(&manifest)?;
    if options.platform == Platform::Scarlet {
        scarlet_machine(
            options
                .target
                .ok_or_else(|| error("Scarlet requires --target"))?,
        )?;
        if options.output.file_name().and_then(|s| s.to_str())
            != Some(&format!("{}.app", definition.app.slug))
        {
            return Err(error("Scarlet output must be <slug>.app"));
        }
    } else {
        if options.target.is_some() {
            return Err(error("--target is currently only supported for Scarlet"));
        }
        if options.platform == Platform::Macos
            && options.output.extension().is_none_or(|s| s != "app")
        {
            return Err(error("macOS output must end in .app"));
        }
    }
    if fs::symlink_metadata(options.output).is_ok() {
        return Err(error("output already exists; choose a new destination"));
    }
    // Validate the selected icon before executing any consumer build command.
    if let Some(icon) = definition.icons.for_platform(options.platform) {
        validate_icon(&resolve_resource(root, icon)?, options.platform)?;
    }
    // Check declared resource containment before allowing a consumer command to run.
    for resource in &definition.resources {
        resolve_resource(root, &resource.source)?;
    }
    if options.build {
        let build = definition
            .build
            .as_ref()
            .ok_or_else(|| error("--build requires [build]"))?;
        let (program, args) = build
            .command
            .split_first()
            .ok_or_else(|| error("--build requires a nonempty build.command"))?;
        let mut command = Command::new(program);
        command
            .args(args)
            .current_dir(root)
            .env("APPCOAT_PLATFORM", options.platform.name())
            .env("APPCOAT_MANIFEST_DIR", root);
        if let Some(target) = options.target {
            command.env("APPCOAT_TARGET", target);
        } else {
            command.env_remove("APPCOAT_TARGET");
        }
        if !command.status()?.success() {
            return Err(error("consumer build command failed"));
        }
    }
    let artifact = options
        .artifact
        .or_else(|| {
            definition
                .build
                .as_ref()
                .map(|build| build.artifact.as_path())
        })
        .ok_or_else(|| error("supply --artifact or build.artifact"))?;
    let artifact = if artifact.is_absolute() {
        artifact.to_path_buf()
    } else {
        root.join(artifact)
    };
    // Artifacts may live in an external build directory or a Nix store output.
    let artifact = artifact.canonicalize()?;
    validate_artifact(&artifact, options.platform, options.target)?;
    let output = if options.output.is_absolute() {
        options.output.to_path_buf()
    } else {
        env::current_dir()?.join(options.output)
    };
    let parent = output
        .parent()
        .ok_or_else(|| error("missing output parent"))?;
    fs::create_dir_all(parent)?;
    let parent = parent.canonicalize()?;
    for resource in &definition.resources {
        let source = resolve_resource(root, &resource.source)?;
        if source.is_dir() && parent.starts_with(source.canonicalize()?) {
            return Err(error("output cannot be inside a resource source directory"));
        }
    }
    let output = parent.join(
        output
            .file_name()
            .ok_or_else(|| error("missing output name"))?,
    );
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let stage = parent.join(format!(".appcoat-stage-{}-{stamp}", std::process::id()));
    fs::create_dir(&stage)?;
    let result = assemble(
        &stage,
        &definition,
        root,
        &artifact,
        options.platform,
        options.target,
    )
    .and_then(|()| {
        // Reserve the new destination. Never delete or replace an existing package.
        fs::create_dir(&output)?;
        if let Err(err) = fs::rename(&stage, &output) {
            let _ = fs::remove_dir(&output);
            return Err(err.into());
        }
        Ok(output)
    });
    if result.is_err() {
        let _ = fs::remove_dir_all(&stage);
    }
    result
}

fn assemble(
    stage: &Path,
    definition: &Definition,
    root: &Path,
    artifact: &Path,
    platform: Platform,
    target: Option<&str>,
) -> Result<()> {
    let app = &definition.app;
    let (executable, resources, receipt) = match platform {
        Platform::Macos => (
            PathBuf::from("Contents/MacOS").join(&app.executable),
            PathBuf::from("Contents/Resources"),
            PathBuf::from("Contents/appcoat-package.toml"),
        ),
        Platform::Linux => (
            PathBuf::from("bin").join(&app.executable),
            PathBuf::from("share/appcoat")
                .join(&app.id)
                .join("resources"),
            PathBuf::from("appcoat-package.toml"),
        ),
        Platform::Scarlet => (
            PathBuf::from("bin").join(&app.executable),
            PathBuf::from("resources"),
            PathBuf::from("appcoat-package.toml"),
        ),
    };
    copy_tree(artifact, &stage.join(&executable))?;
    fs::create_dir_all(stage.join(&resources))?;
    for resource in &definition.resources {
        copy_tree(
            &resolve_resource(root, &resource.source)?,
            &stage.join(&resources).join(relative(&resource.to)?),
        )?;
    }
    let icon_name = if let Some(icon) = definition.icons.for_platform(platform) {
        let extension = if platform == Platform::Macos {
            "icns"
        } else {
            "png"
        };
        let name = format!("{}.{}", app.id, extension);
        let destination = if platform == Platform::Linux {
            PathBuf::from("share/pixmaps").join(&name)
        } else {
            resources.join(&name)
        };
        let source = resolve_resource(root, icon)?;
        validate_icon(&source, platform)?;
        copy_tree(&source, &stage.join(&destination))?;
        Some(name)
    } else {
        None
    };
    match platform {
        Platform::Macos => {
            let xml = |value: &str| {
                value
                    .replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;")
                    .replace('"', "&quot;")
                    .replace('\'', "&apos;")
            };
            let entries = [
                ("CFBundleIdentifier", &app.id),
                ("CFBundleName", &app.name),
                ("CFBundleExecutable", &app.executable),
                ("CFBundleShortVersionString", &app.version),
                ("CFBundleVersion", &app.version),
                ("CFBundlePackageType", &"APPL".to_string()),
            ];
            let mut plist = String::from(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict>\n",
            );
            for (key, value) in entries {
                plist.push_str(&format!(
                    "<key>{key}</key><string>{}</string>\n",
                    xml(value)
                ));
            }
            plist.push_str("<key>NSHighResolutionCapable</key><true/>\n");
            if let Some(icon) = &icon_name {
                plist.push_str(&format!(
                    "<key>CFBundleIconFile</key><string>{}</string>\n",
                    xml(icon)
                ));
            }
            plist.push_str("</dict></plist>\n");
            fs::write(stage.join("Contents/Info.plist"), plist)?;
        }
        Platform::Linux | Platform::Scarlet => {
            let name = app.name.replace('\\', "\\\\");
            let mut desktop = format!(
                "[Desktop Entry]\nType=Application\nName={name}\nExec={}\nTerminal=false\n",
                if platform == Platform::Scarlet {
                    format!("bin/{}", app.executable)
                } else {
                    app.executable.clone()
                }
            );
            if definition.desktop.open_file {
                desktop = desktop.replace("\nTerminal=", " %f\nTerminal=");
            }
            if let Some(icon) = &icon_name {
                let icon = if platform == Platform::Scarlet {
                    format!("resources/{icon}")
                } else {
                    app.id.clone()
                };
                desktop.push_str(&format!("Icon={icon}\n"));
            }
            for (key, values) in [
                ("MimeType", &definition.desktop.mime_types),
                ("Categories", &definition.desktop.categories),
            ] {
                if !values.is_empty() {
                    desktop.push_str(&format!("{key}={};\n", values.join(";")));
                }
            }
            let path = if platform == Platform::Scarlet {
                desktop.push_str(&format!(
                    "X-Scarlet-NewInstance={}\nX-Scarlet-AppFormat=1\nX-Scarlet-Target={}\n",
                    definition.desktop.new_instance,
                    target.unwrap()
                ));
                PathBuf::from(format!("{}.desktop", app.id))
            } else {
                PathBuf::from("share/applications").join(format!("{}.desktop", app.id))
            };
            fs::create_dir_all(stage.join(&path).parent().unwrap())?;
            fs::write(stage.join(path), desktop)?;
        }
    }
    let package = Package {
        format: 1,
        platform,
        target: target.map(str::to_owned),
        id: app.id.clone(),
        executable,
        resources,
    };
    fs::write(stage.join(receipt), toml::to_string(&package)?)?;
    if platform == Platform::Scarlet {
        validate_scarlet_tree(stage, target.unwrap())?;
    }
    Ok(())
}

fn validate_icon(path: &Path, platform: Platform) -> Result<()> {
    if !path.is_file() {
        return Err(error("icon must be a file"));
    }
    let data = fs::read(path)?;
    let valid = if platform == Platform::Macos {
        data.len() >= 8
            && data.starts_with(b"icns")
            && u32::from_be_bytes(data[4..8].try_into()?) as usize == data.len()
    } else {
        data.len() >= 24
            && data.starts_with(b"\x89PNG\r\n\x1a\n")
            && &data[12..16] == b"IHDR"
            && u32::from_be_bytes(data[16..20].try_into()?) > 0
            && u32::from_be_bytes(data[20..24].try_into()?) > 0
    };
    if !valid {
        return Err(error(
            "expected an ICNS icon for macOS or a PNG icon for Linux/Scarlet",
        ));
    }
    Ok(())
}

fn validate_artifact(path: &Path, platform: Platform, target: Option<&str>) -> Result<()> {
    if !path.is_file() {
        return Err(error("artifact must be an executable file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if fs::metadata(path)?.permissions().mode() & 0o111 == 0 {
            return Err(error("artifact lacks executable permissions"));
        }
    }
    let bytes = fs::read(path)?;
    let magic = bytes
        .get(..4)
        .ok_or_else(|| error("artifact is too short"))?;
    match platform {
        Platform::Scarlet => validate_scarlet_elf(&bytes, target.unwrap()),
        Platform::Macos => {
            if !bytes.starts_with(b"#!")
                && !matches!(
                    magic,
                    [0xce, 0xfa, 0xed, 0xfe]
                        | [0xcf, 0xfa, 0xed, 0xfe]
                        | [0xfe, 0xed, 0xfa, 0xce]
                        | [0xfe, 0xed, 0xfa, 0xcf]
                        | [0xca, 0xfe, 0xba, 0xbe]
                        | [0xca, 0xfe, 0xba, 0xbf]
                )
            {
                return Err(error("macOS artifact must be Mach-O or a shebang script"));
            }
            Ok(())
        }
        Platform::Linux => {
            if !(bytes.starts_with(b"#!")
                || (bytes.len() >= 64 && magic == b"\x7fELF" && matches!(bytes[7], 0 | 3)))
            {
                return Err(error(
                    "Linux artifact must be Linux/System-V ELF or a shebang script",
                ));
            }
            Ok(())
        }
    }
}

fn scarlet_machine(target: &str) -> Result<u16> {
    match target {
        "aarch64-unknown-scarlet" => Ok(183),
        "riscv64gc-unknown-scarlet" => Ok(243),
        _ => Err(error(
            "Scarlet supports aarch64-unknown-scarlet and riscv64gc-unknown-scarlet",
        )),
    }
}

/// Match the local Scarlet SDK artifact contract without importing its compiler or crates.
fn validate_scarlet_elf(bytes: &[u8], target: &str) -> Result<()> {
    let machine = scarlet_machine(target)?;
    if bytes.len() < 64
        || &bytes[..8] != b"\x7fELF\x02\x01\x01\x53"
        || u16::from_le_bytes(bytes[18..20].try_into()?) != machine
        || !matches!(u16::from_le_bytes(bytes[16..18].try_into()?), 2 | 3)
    {
        return Err(error(
            "expected native Scarlet ELF64 with matching architecture",
        ));
    }
    let phoff = usize::try_from(u64::from_le_bytes(bytes[32..40].try_into()?))?;
    let size = usize::from(u16::from_le_bytes(bytes[54..56].try_into()?));
    let count = usize::from(u16::from_le_bytes(bytes[56..58].try_into()?));
    if count > 0
        && (size != 56
            || phoff
                .checked_add(count * size)
                .is_none_or(|end| end > bytes.len()))
    {
        return Err(error("invalid ELF program headers"));
    }
    let mut interpreters = 0;
    for index in 0..count {
        let header = &bytes[phoff + index * size..phoff + (index + 1) * size];
        if u32::from_le_bytes(header[..4].try_into()?) == 3 {
            interpreters += 1;
            let start = usize::try_from(u64::from_le_bytes(header[8..16].try_into()?))?;
            let length = usize::try_from(u64::from_le_bytes(header[32..40].try_into()?))?;
            let end = start
                .checked_add(length)
                .ok_or_else(|| error("interpreter range overflow"))?;
            if bytes.get(start..end) != Some(b"/bin/scarlet-ld\0".as_slice()) {
                return Err(error("Scarlet interpreter must be /bin/scarlet-ld"));
            }
        }
    }
    if interpreters > 1 {
        return Err(error("multiple ELF interpreters"));
    }
    Ok(())
}

fn validate_scarlet_tree(root: &Path, target: &str) -> Result<()> {
    fn visit(path: &Path, target: &str) -> Result<()> {
        let metadata = fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() {
            return Err(error("Scarlet package symlinks are unsupported"));
        }
        if metadata.is_dir() {
            for entry in fs::read_dir(path)? {
                visit(&entry?.path(), target)?;
            }
        } else {
            if matches!(
                path.file_name().and_then(|s| s.to_str()),
                Some("app.toml" | "Cargo.toml" | "bundle.toml")
            ) {
                return Err(error("Scarlet packages cannot contain build recipes"));
            }
            let bytes = fs::read(path)?;
            if bytes.starts_with(b"\x7fELF") {
                validate_scarlet_elf(&bytes, target)?;
            }
        }
        Ok(())
    }
    fn desktop_count(path: &Path) -> Result<usize> {
        if path.is_dir() {
            let mut count = 0;
            for entry in fs::read_dir(path)? {
                count += desktop_count(&entry?.path())?;
            }
            Ok(count)
        } else {
            Ok(usize::from(
                path.extension().is_some_and(|s| s == "desktop"),
            ))
        }
    }
    visit(root, target)?;
    if desktop_count(root)? != 1 {
        return Err(error("Scarlet requires exactly one desktop descriptor"));
    }
    Ok(())
}

pub fn inspect(package_root: &Path) -> Result<(PathBuf, Package)> {
    let root = package_root.canonicalize()?;
    let receipt = if root.join("Contents/appcoat-package.toml").is_file() {
        root.join("Contents/appcoat-package.toml")
    } else {
        root.join("appcoat-package.toml")
    };
    let package: Package = toml::from_str(&fs::read_to_string(receipt)?)?;
    if package.format != 1 || !token(&package.id) {
        return Err(error("unsupported package receipt"));
    }
    relative_path(&package.executable)?;
    relative_path(&package.resources)?;
    for path in [&package.executable, &package.resources] {
        if !root.join(path).canonicalize()?.starts_with(&root) {
            return Err(error("package receipt path escapes package"));
        }
    }
    if !root.join(&package.resources).is_dir() || !root.join(&package.executable).is_file() {
        return Err(error("missing package executable or resources"));
    }
    Ok((root, package))
}

pub fn resource_root(package_root: &Path) -> Result<PathBuf> {
    let (root, package) = inspect(package_root)?;
    Ok(root.join(package.resources).canonicalize()?)
}

/// Launch a host package with its explicit resource root, preserving cwd and argv.
pub fn run(package_root: &Path, args: &[std::ffi::OsString]) -> Result<ExitStatus> {
    let (root, package) = inspect(package_root)?;
    if !package.platform.is_host() {
        return Err(error(
            "cannot run this cross-platform package on the current host",
        ));
    }
    Command::new(root.join(package.executable))
        .args(args)
        .env(
            "APPCOAT_RESOURCE_ROOT",
            root.join(package.resources).canonicalize()?,
        )
        .status()
        .map_err(Into::into)
}

/// Print paths without requiring callers to parse informational build output.
pub fn write_path(path: &Path) -> Result<()> {
    writeln!(io::stdout(), "{}", path.display())?;
    Ok(())
}

#[cfg(test)]
mod tests;
