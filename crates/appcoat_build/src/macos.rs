//! Opt-in closure for native macOS executables with absolute dylib dependencies.
use super::{Result, error};
use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn tool(program: &str, args: &[&std::ffi::OsStr]) -> Result<String> {
    let output = Command::new(program).args(args).output()?;
    if !output.status.success() {
        return Err(error(format!(
            "{program}: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(String::from_utf8(output.stdout)?)
}

fn dependencies(report: &str) -> Result<Vec<String>> {
    report
        .lines()
        .skip(1)
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let (name, _) = line
                .trim()
                .split_once(" (compatibility version ")
                .ok_or_else(|| {
                    error("unsupported otool dependency report (use a thin Mach-O executable)")
                })?;
            Ok(name.to_owned())
        })
        .collect()
}

pub(super) fn system_library(name: &str) -> bool {
    name.starts_with("/usr/lib/") || name.starts_with("/System/Library/")
}

/// Resolve only dependencies whose location is unambiguous without dyld's
/// process-specific rpath stack. Refuse unsupported layouts rather than ship a
/// bundle that still points at a build machine's files.
fn dependency_path(name: &str, source: &Path, executable: &Path) -> Result<PathBuf> {
    let path = if let Some(tail) = name.strip_prefix("@loader_path/") {
        source.parent().unwrap().join(tail)
    } else if let Some(tail) = name.strip_prefix("@executable_path/") {
        executable.parent().unwrap().join(tail)
    } else if Path::new(name).is_absolute() {
        PathBuf::from(name)
    } else {
        return Err(error(format!(
            "cannot bundle dependency {name}: resolve @rpath dependencies in the consumer build first"
        )));
    };
    let path = path.canonicalize()?;
    if path.extension().is_none_or(|ext| ext != "dylib") || !path.is_file() {
        return Err(error(format!(
            "only dylib files can be bundled: {}",
            path.display()
        )));
    }
    Ok(path)
}

pub(super) fn bundle(
    stage: &Path,
    artifact: &Path,
    executable: &Path,
    system_libraries: &BTreeMap<String, String>,
) -> Result<()> {
    if !cfg!(target_os = "macos") {
        return Err(error(
            "macos.bundle-libraries requires a macOS packaging host",
        ));
    }
    if fs::read(artifact)?.starts_with(b"#!") {
        return Err(error(
            "macos.bundle-libraries requires a Mach-O executable, not a script",
        ));
    }
    let frameworks = stage.join("Contents/Frameworks");
    let main = stage.join(executable);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&main)?.permissions().mode();
        fs::set_permissions(&main, fs::Permissions::from_mode(mode | 0o200))?;
    }
    let mut pending = VecDeque::from([(artifact.to_path_buf(), main.clone(), false)]);
    let mut libraries = BTreeMap::<String, PathBuf>::new();
    while let Some((source, destination, is_library)) = pending.pop_front() {
        let report = tool("/usr/bin/otool", &["-L".as_ref(), source.as_os_str()])?;
        let id_report = tool("/usr/bin/otool", &["-D".as_ref(), source.as_os_str()])?;
        let own_id = id_report.lines().nth(1).map(str::trim);
        for dependency in dependencies(&report)? {
            if Some(dependency.as_str()) == own_id || system_library(&dependency) {
                continue;
            }
            if let Some(replacement) = Path::new(&dependency)
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| system_libraries.get(name))
            {
                tool(
                    "/usr/bin/install_name_tool",
                    &[
                        "-change".as_ref(),
                        dependency.as_ref(),
                        replacement.as_ref(),
                        destination.as_os_str(),
                    ],
                )?;
                continue;
            }
            let path = dependency_path(&dependency, &source, artifact)?;
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| error("library filename must be UTF-8"))?
                .to_owned();
            if let Some(previous) = libraries.get(&name) {
                if previous != &path {
                    return Err(error(format!("bundled library filename collision: {name}")));
                }
            } else {
                fs::create_dir_all(&frameworks)?;
                let copied = frameworks.join(&name);
                super::copy_tree(&path, &copied)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mode = fs::metadata(&copied)?.permissions().mode();
                    fs::set_permissions(&copied, fs::Permissions::from_mode(mode | 0o200))?;
                }
                libraries.insert(name.clone(), path.clone());
                pending.push_back((path, copied, true));
            }
            let reference = if is_library {
                format!("@loader_path/{name}")
            } else {
                format!("@executable_path/../Frameworks/{name}")
            };
            tool(
                "/usr/bin/install_name_tool",
                &[
                    "-change".as_ref(),
                    dependency.as_ref(),
                    reference.as_ref(),
                    destination.as_os_str(),
                ],
            )?;
        }
        if is_library {
            let id = format!(
                "@rpath/{}",
                destination.file_name().unwrap().to_string_lossy()
            );
            tool(
                "/usr/bin/install_name_tool",
                &["-id".as_ref(), id.as_ref(), destination.as_os_str()],
            )?;
        }
    }
    // Changes to load commands invalidate existing signatures. Sign leaves first,
    // then the complete app; this is local ad-hoc signing, not distribution signing.
    for name in libraries.keys() {
        let path = frameworks.join(name);
        tool(
            "/usr/bin/codesign",
            &[
                "--force".as_ref(),
                "--sign".as_ref(),
                "-".as_ref(),
                path.as_os_str(),
            ],
        )?;
    }
    tool(
        "/usr/bin/codesign",
        &[
            "--force".as_ref(),
            "--sign".as_ref(),
            "-".as_ref(),
            stage.as_os_str(),
        ],
    )?;
    tool(
        "/usr/bin/codesign",
        &[
            "--verify".as_ref(),
            "--deep".as_ref(),
            "--strict".as_ref(),
            stage.as_os_str(),
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dependency_reports_preserve_spaces_and_reject_ambiguous_formats() {
        let report = "app:\n\t/nix/store/a/lib/example name.dylib (compatibility version 1.0.0, current version 1.0.0)\n\t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1.0.0)\n";
        assert_eq!(
            dependencies(report).unwrap(),
            [
                "/nix/store/a/lib/example name.dylib",
                "/usr/lib/libSystem.B.dylib"
            ]
        );
        assert!(dependencies("app:\napp (architecture arm64):\n").is_err());
        assert!(system_library(
            "/System/Library/Frameworks/AppKit.framework/AppKit"
        ));
        assert!(!system_library("/usr/local/lib/other.dylib"));
        assert!(
            dependency_path(
                "@rpath/libother.dylib",
                Path::new("/tmp/app"),
                Path::new("/tmp/app")
            )
            .is_err()
        );
    }
}
