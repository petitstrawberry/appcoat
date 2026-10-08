use appcoat_build::{PackageOptions, Platform, Result};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
    process::ExitCode,
};

const HELP: &str = "appcoat: artifact-first application packaging\n\n\
Usage:\n\
  appcoat package --manifest FILE --platform macos|linux|scarlet --output PATH [--artifact FILE] [--build] [--target TRIPLE]\n\
  appcoat run --package PATH [-- ARGS...]\n\
  appcoat resource-root --package PATH\n\n\
Artifact paths in [build] are relative to the manifest; --artifact is relative to the caller.\n\
Build commands run only with --build, in the manifest directory. No implicit Cargo or Nix.\n\
Scarlet requires --target aarch64-unknown-scarlet or riscv64gc-unknown-scarlet.\n\
Output must be new. run preserves cwd, forwards arguments, and sets APPCOAT_RESOURCE_ROOT.\n";

fn main() -> ExitCode {
    match execute() {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("appcoat: {error}");
            ExitCode::FAILURE
        }
    }
}

fn execute() -> Result<u8> {
    let mut args = std::env::args_os().skip(1);
    let Some(command) = args.next() else {
        print!("{HELP}");
        return Ok(0);
    };
    if command == "--help" || command == "-h" {
        print!("{HELP}");
        return Ok(0);
    }
    let mut options = BTreeMap::<String, OsString>::new();
    let mut build = false;
    let mut trailing = Vec::new();
    while let Some(arg) = args.next() {
        if arg == "--" {
            trailing.extend(args);
            break;
        }
        let key = arg.to_str().ok_or("option names must be UTF-8")?;
        if key == "--build" {
            if build {
                return Err("duplicate --build".into());
            }
            build = true;
            continue;
        }
        if !key.starts_with("--") {
            return Err(format!("unexpected argument: {key}").into());
        }
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for {key}"))?;
        if value.to_str().is_some_and(|s| s.starts_with("--")) {
            return Err(format!("missing value for {key}").into());
        }
        if options.insert(key.to_owned(), value).is_some() {
            return Err(format!("duplicate option: {key}").into());
        }
    }
    let required = |options: &mut BTreeMap<String, OsString>, key: &str| -> Result<OsString> {
        options
            .remove(key)
            .ok_or_else(|| format!("missing {key}").into())
    };
    let code = match command.to_str().ok_or("command must be UTF-8")? {
        "package" => {
            if !trailing.is_empty() {
                return Err("package does not take trailing arguments".into());
            }
            let manifest = PathBuf::from(required(&mut options, "--manifest")?);
            let platform = required(&mut options, "--platform")?;
            let platform = Platform::parse(platform.to_str().ok_or("platform must be UTF-8")?)?;
            let output = PathBuf::from(required(&mut options, "--output")?);
            let target = options.remove("--target");
            let target = target
                .as_ref()
                .map(|s| s.to_str().ok_or("target must be UTF-8"))
                .transpose()?;
            let artifact = options.remove("--artifact").map(PathBuf::from);
            let artifact = artifact.map(|path| {
                if path.is_absolute() {
                    path
                } else {
                    std::env::current_dir().unwrap().join(path)
                }
            });
            check_unknown(&options)?;
            let path = appcoat_build::package(PackageOptions {
                manifest: &manifest,
                platform,
                target,
                artifact: artifact.as_deref(),
                output: &output,
                build,
            })?;
            appcoat_build::write_path(&path)?;
            0
        }
        "run" | "resource-root" => {
            if build {
                return Err("--build is only valid with package".into());
            }
            let package = required(&mut options, "--package")?;
            check_unknown(&options)?;
            if command == "resource-root" {
                if !trailing.is_empty() {
                    return Err("resource-root does not take trailing arguments".into());
                }
                appcoat_build::write_path(&appcoat_build::resource_root(Path::new(&package))?)?;
                0
            } else {
                let status = appcoat_build::run(Path::new(&package), &trailing)?;
                #[cfg(unix)]
                {
                    use std::os::unix::process::ExitStatusExt;
                    status
                        .code()
                        .unwrap_or_else(|| 128 + status.signal().unwrap_or(1))
                        as u8
                }
                #[cfg(not(unix))]
                {
                    status.code().unwrap_or(1) as u8
                }
            }
        }
        _ => return Err("unknown command; use appcoat --help".into()),
    };
    Ok(code)
}

fn check_unknown(options: &BTreeMap<String, OsString>) -> Result<()> {
    if let Some(key) = options.keys().next() {
        return Err(format!("unknown option: {key}").into());
    }
    Ok(())
}
