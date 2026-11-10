use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static SERIAL: AtomicUsize = AtomicUsize::new(0);
        let root = env::temp_dir().join(format!(
            "appcoat-build-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::write(root.join("program"), "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(root.join("program"), fs::Permissions::from_mode(0o755)).unwrap();
        }
        fs::write(root.join("greeting.txt"), "hello").unwrap();
        fs::write(root.join("appcoat.toml"), "[app]\nid='dev.example.reader'\nslug='reader'\nname='Reader & Friends'\nversion='0.1.0'\nexecutable='reader'\n[build]\nartifact='program'\n[[resources]]\nsource='greeting.txt'\nto='greeting.txt'\n").unwrap();
        Self(root)
    }
    fn pack(&self, platform: Platform, target: Option<&str>, output: &str) -> Result<PathBuf> {
        package(PackageOptions {
            manifest: &self.0.join("appcoat.toml"),
            platform,
            target,
            artifact: None,
            output: &self.0.join(output),
            build: false,
        })
    }
    fn replace(&self, from: &str, to: &str) {
        let manifest = self.0.join("appcoat.toml");
        fs::write(
            &manifest,
            fs::read_to_string(&manifest).unwrap().replace(from, to),
        )
        .unwrap();
    }
    fn elf(&self, machine: u16) {
        let mut elf = [0u8; 64];
        elf[..8].copy_from_slice(b"\x7fELF\x02\x01\x01\x53");
        elf[16..18].copy_from_slice(&2u16.to_le_bytes());
        elf[18..20].copy_from_slice(&machine.to_le_bytes());
        fs::write(self.0.join("program"), elf).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn macos_metadata_and_linux_layout_share_resources() {
    let fixture = Fixture::new();
    let mac = fixture.pack(Platform::Macos, None, "reader.app").unwrap();
    assert!(
        fs::read_to_string(mac.join("Contents/Info.plist"))
            .unwrap()
            .contains("Reader &amp; Friends")
    );
    assert_eq!(
        fs::read(resource_root(&mac).unwrap().join("greeting.txt")).unwrap(),
        b"hello"
    );
    let linux = fixture.pack(Platform::Linux, None, "install").unwrap();
    assert!(linux.join("bin/reader").is_file());
    assert!(
        linux
            .join("share/applications/dev.example.reader.desktop")
            .is_file()
    );
    assert_eq!(
        fs::read(resource_root(&linux).unwrap().join("greeting.txt")).unwrap(),
        b"hello"
    );
    let relocated = fixture.0.join("moved.app");
    fs::rename(mac, &relocated).unwrap();
    assert!(
        resource_root(&relocated)
            .unwrap()
            .starts_with(relocated.canonicalize().unwrap())
    );
}

#[test]
fn existing_output_and_failed_staging_are_preserved() {
    let fixture = Fixture::new();
    let output = fixture.pack(Platform::Macos, None, "reader.app").unwrap();
    fs::write(output.join("sentinel"), "keep").unwrap();
    assert!(fixture.pack(Platform::Macos, None, "reader.app").is_err());
    assert_eq!(fs::read(output.join("sentinel")).unwrap(), b"keep");
    fixture.replace("to='greeting.txt'", "to='../escape'");
    assert!(fixture.pack(Platform::Macos, None, "failed.app").is_err());
    assert!(!fixture.0.join("failed.app").exists());
    assert!(!fixture.0.join("escape").exists());
}

#[test]
fn duplicate_resources_and_symlinks_do_not_publish() {
    let fixture = Fixture::new();
    fixture.replace(
        "to='greeting.txt'",
        "to='greeting.txt'\n[[resources]]\nsource='greeting.txt'\nto='greeting.txt'",
    );
    assert!(
        fixture
            .pack(Platform::Macos, None, "collision.app")
            .is_err()
    );
    assert!(!fixture.0.join("collision.app").exists());
    #[cfg(unix)]
    {
        fs::remove_file(fixture.0.join("greeting.txt")).unwrap();
        std::os::unix::fs::symlink("program", fixture.0.join("greeting.txt")).unwrap();
        assert!(fixture.pack(Platform::Macos, None, "symlink.app").is_err());
    }
    assert!(!fs::read_dir(&fixture.0).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".appcoat-stage")
    }));
}

#[test]
fn scarlet_native_headers_and_resource_abi_are_checked() {
    let fixture = Fixture::new();
    fixture.elf(183);
    let app = fixture
        .pack(
            Platform::Scarlet,
            Some("aarch64-unknown-scarlet"),
            "reader.app",
        )
        .unwrap();
    assert!(
        fs::read_to_string(app.join("dev.example.reader.desktop"))
            .unwrap()
            .contains("X-Scarlet-Target=aarch64-unknown-scarlet")
    );
    assert!(
        fixture
            .pack(
                Platform::Scarlet,
                Some("riscv64gc-unknown-scarlet"),
                "wrong/reader.app"
            )
            .is_err()
    );
    fixture.elf(243);
    assert!(
        fixture
            .pack(
                Platform::Scarlet,
                Some("riscv64gc-unknown-scarlet"),
                "rv/reader.app"
            )
            .is_ok()
    );
    fixture.replace("source='greeting.txt'", "source='program'");
    fs::write(fixture.0.join("program"), "#!/bin/sh\nexit 0\n").unwrap();
    assert!(
        fixture
            .pack(
                Platform::Scarlet,
                Some("aarch64-unknown-scarlet"),
                "script/reader.app"
            )
            .is_err()
    );
    fixture.elf(183);
    let mut mixed = fs::read(fixture.0.join("program")).unwrap();
    mixed[7] = 0;
    fs::write(fixture.0.join("greeting.txt"), mixed).unwrap();
    fixture.replace(
        "source='program'\nto='greeting.txt'",
        "source='greeting.txt'\nto='greeting.txt'",
    );
    assert!(
        fixture
            .pack(
                Platform::Scarlet,
                Some("aarch64-unknown-scarlet"),
                "mixed/reader.app"
            )
            .is_err()
    );
}

#[test]
fn scarlet_interpreter_ranges_cannot_panic_or_use_linux_loader() {
    let fixture = Fixture::new();
    fixture.elf(183);
    let mut bytes = fs::read(fixture.0.join("program")).unwrap();
    bytes.resize(120, 0);
    bytes[32..40].copy_from_slice(&64u64.to_le_bytes());
    bytes[54..56].copy_from_slice(&56u16.to_le_bytes());
    bytes[56..58].copy_from_slice(&1u16.to_le_bytes());
    bytes[64..68].copy_from_slice(&3u32.to_le_bytes());
    bytes[72..80].copy_from_slice(&u64::MAX.to_le_bytes());
    bytes[96..104].copy_from_slice(&10u64.to_le_bytes());
    assert!(validate_scarlet_elf(&bytes, "aarch64-unknown-scarlet").is_err());
    bytes[72..80].copy_from_slice(&120u64.to_le_bytes());
    bytes.extend_from_slice(b"/bin/scarlet-ld\0");
    bytes[96..104].copy_from_slice(&16u64.to_le_bytes());
    assert!(validate_scarlet_elf(&bytes, "aarch64-unknown-scarlet").is_ok());
    bytes[125] = b'L';
    assert!(validate_scarlet_elf(&bytes, "aarch64-unknown-scarlet").is_err());
}

#[test]
fn receipt_escape_and_cross_platform_launch_are_rejected() {
    let fixture = Fixture::new();
    let app = fixture.pack(Platform::Macos, None, "reader.app").unwrap();
    let receipt = app.join("Contents/appcoat-package.toml");
    fs::write(
        &receipt,
        fs::read_to_string(&receipt)
            .unwrap()
            .replace("Contents/Resources", "../escape"),
    )
    .unwrap();
    assert!(resource_root(&app).is_err());
    let platform = if cfg!(target_os = "macos") {
        Platform::Linux
    } else {
        Platform::Macos
    };
    let other = fixture.pack(platform, None, "other.app").unwrap();
    assert!(run(&other, &[]).is_err());
}

#[test]
fn malformed_definition_and_nonexecutable_artifact_are_rejected() {
    let fixture = Fixture::new();
    fixture.replace("name='Reader & Friends'", "name='Reader'\nunknown='typo'");
    assert!(fixture.pack(Platform::Macos, None, "bad.app").is_err());
    fixture.replace("\nunknown='typo'", "");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(fixture.0.join("program"), fs::Permissions::from_mode(0o644)).unwrap();
        assert!(fixture.pack(Platform::Macos, None, "bad.app").is_err());
    }
}

#[test]
fn staging_cannot_recurse_into_its_resource_source() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.0.join("assets")).unwrap();
    fs::write(fixture.0.join("assets/greeting.txt"), "hello").unwrap();
    fixture.replace("source='greeting.txt'", "source='assets'");
    assert!(
        fixture
            .pack(Platform::Macos, None, "assets/recursive.app")
            .is_err()
    );
    assert!(!fixture.0.join("assets/recursive.app").exists());
}

#[test]
fn native_icons_and_desktop_metadata_are_packaged() {
    let fixture = Fixture::new();
    let mut icns = b"icns".to_vec();
    icns.extend_from_slice(&8u32.to_be_bytes());
    fs::write(fixture.0.join("icon.icns"), &icns).unwrap();
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend_from_slice(&13u32.to_be_bytes());
    png.extend_from_slice(b"IHDR");
    png.extend_from_slice(&512u32.to_be_bytes());
    png.extend_from_slice(&512u32.to_be_bytes());
    fs::write(fixture.0.join("icon.png"), &png).unwrap();
    fixture.replace("[build]", "[icons]\nmacos='icon.icns'\nlinux='icon.png'\nscarlet='icon.png'\n[desktop]\nopen-file=true\nnew-instance=true\nmime-types=['text/plain','text/x-c++src']\ncategories=['Development','TextEditor']\n[build]");
    let mac = fixture.pack(Platform::Macos, None, "reader.app").unwrap();
    let plist = fs::read_to_string(mac.join("Contents/Info.plist")).unwrap();
    assert!(plist.contains("<key>CFBundleIconFile</key><string>dev.example.reader.icns</string>"));
    assert!(plist.contains("<key>NSHighResolutionCapable</key><true/>"));
    assert!(!plist.contains("CFBundleDocumentTypes"));
    assert_eq!(
        fs::read(mac.join("Contents/Resources/dev.example.reader.icns")).unwrap(),
        icns
    );
    let linux = fixture.pack(Platform::Linux, None, "linux").unwrap();
    let desktop =
        fs::read_to_string(linux.join("share/applications/dev.example.reader.desktop")).unwrap();
    assert!(desktop.contains("Exec=reader %f\n"));
    assert!(desktop.contains("Icon=dev.example.reader\n"));
    assert!(desktop.contains("MimeType=text/plain;text/x-c++src;\n"));
    assert!(desktop.contains("Categories=Development;TextEditor;\n"));
    assert_eq!(
        fs::read(linux.join("share/pixmaps/dev.example.reader.png")).unwrap(),
        png
    );
    fixture.elf(183);
    let scarlet = fixture
        .pack(
            Platform::Scarlet,
            Some("aarch64-unknown-scarlet"),
            "scarlet/reader.app",
        )
        .unwrap();
    let desktop = fs::read_to_string(scarlet.join("dev.example.reader.desktop")).unwrap();
    assert!(desktop.contains("Exec=bin/reader %f\n"));
    assert!(desktop.contains("Icon=resources/dev.example.reader.png\n"));
    assert!(desktop.contains("X-Scarlet-NewInstance=true\n"));
    assert_eq!(
        fs::read(scarlet.join("resources/dev.example.reader.png")).unwrap(),
        png
    );
}

#[test]
fn invalid_icons_and_desktop_values_do_not_publish_or_run_build() {
    let fixture = Fixture::new();
    fixture.replace(
        "[build]",
        "[icons]\nmacos='../escape.icns'\n[build]\ncommand=['sh','-c','touch build-ran']",
    );
    assert!(
        package(PackageOptions {
            manifest: &fixture.0.join("appcoat.toml"),
            platform: Platform::Macos,
            target: None,
            artifact: None,
            output: &fixture.0.join("bad.app"),
            build: true,
        })
        .is_err()
    );
    assert!(!fixture.0.join("build-ran").exists());
    fixture.replace("../escape.icns", "greeting.txt");
    assert!(fixture.pack(Platform::Macos, None, "bad.app").is_err());
    assert!(!fixture.0.join("bad.app").exists());
    // Unselected platform icons need not exist when packaging another platform.
    fixture.replace("greeting.txt'\n[build]", "missing.icns'\n[build]");
    assert!(fixture.pack(Platform::Linux, None, "linux").is_ok());
    fixture.replace(
        "[icons]",
        "[desktop]\nmime-types=['text/plain;Injected=true']\n[icons]",
    );
    assert!(fixture.pack(Platform::Linux, None, "bad-linux").is_err());
    fixture.replace(
        "mime-types=['text/plain;Injected=true']",
        "categories=['Editor;Injected']",
    );
    assert!(fixture.pack(Platform::Linux, None, "bad-linux").is_err());
}

#[cfg(unix)]
#[test]
fn icon_symlinks_and_resource_collisions_are_rejected() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("icon.icns"), b"icns\0\0\0\x08").unwrap();
    std::os::unix::fs::symlink("icon.icns", fixture.0.join("link.icns")).unwrap();
    fixture.replace("[build]", "[icons]\nmacos='link.icns'\n[build]");
    assert!(fixture.pack(Platform::Macos, None, "link.app").is_err());
    fixture.replace("macos='link.icns'", "macos='icon.icns'");
    fixture.replace("to='greeting.txt'", "to='dev.example.reader.icns'");
    assert!(
        fixture
            .pack(Platform::Macos, None, "collision.app")
            .is_err()
    );
    assert!(!fixture.0.join("collision.app").exists());
}
