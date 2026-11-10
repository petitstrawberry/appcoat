//! Optional logical resource access. No UI, build system or Scarlet dependency.
use std::{
    env, fs, io,
    path::{Component, Path, PathBuf},
};

pub const RESOURCE_ROOT_ENV: &str = "APPCOAT_RESOURCE_ROOT";

#[derive(Clone, Debug)]
pub struct Resources {
    root: PathBuf,
}

impl Resources {
    /// An explicit absolute root supplied by an app, OS integration or launcher.
    pub fn new(root: impl AsRef<Path>) -> io::Result<Self> {
        let root = root.as_ref();
        if !root.is_absolute() {
            return Err(invalid("resource root must be absolute"));
        }
        let root = root.canonicalize()?;
        if !root.is_dir() {
            return Err(invalid("resource root must be a directory"));
        }
        Ok(Self { root })
    }

    /// The cross-language launcher contract; no dependency on the working directory.
    pub fn from_env() -> io::Result<Self> {
        Self::new(
            env::var_os(RESOURCE_ROOT_ENV)
                .ok_or_else(|| invalid("APPCOAT_RESOURCE_ROOT is not set"))?,
        )
    }

    /// Prefer an explicit launcher root, then use the documented native install layout.
    /// `current_exe` comes from the OS, never argv[0]. Unsupported hosts require a root.
    pub fn discover(app_id: &str) -> io::Result<Self> {
        if env::var_os(RESOURCE_ROOT_ENV).is_some() {
            return Self::from_env();
        }
        let executable = env::current_exe()?.canonicalize()?;
        Self::from_installed_executable(&executable, app_id)
    }

    fn from_installed_executable(executable: &Path, app_id: &str) -> io::Result<Self> {
        if app_id.is_empty()
            || !app_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            || matches!(app_id, "." | "..")
        {
            return Err(invalid("invalid application ID"));
        }
        let directory = executable
            .parent()
            .ok_or_else(|| invalid("missing executable directory"))?;
        #[cfg(target_os = "macos")]
        {
            let _ = app_id;
            let contents = directory
                .parent()
                .ok_or_else(|| invalid("missing Contents directory"))?;
            if directory.file_name() != Some(std::ffi::OsStr::new("MacOS"))
                || contents.file_name() != Some(std::ffi::OsStr::new("Contents"))
                || !contents.join("Resources/appcoat-package.toml").is_file()
            {
                return Err(invalid(
                    "executable is not in an appcoat macOS bundle; supply an explicit root",
                ));
            }
            Self::new(contents.join("Resources"))
        }
        #[cfg(target_os = "linux")]
        {
            if directory.file_name() != Some(std::ffi::OsStr::new("bin")) {
                return Err(invalid("executable is not in an appcoat install tree"));
            }
            let root = directory
                .parent()
                .ok_or_else(|| invalid("missing install prefix"))?;
            if !root.join("appcoat-package.toml").is_file() {
                return Err(invalid(
                    "missing appcoat install receipt; supply an explicit root",
                ));
            }
            Self::new(root.join("share/appcoat").join(app_id).join("resources"))
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            let _ = (directory, app_id);
            Err(invalid("this platform requires an explicit resource root"))
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolve a logical slash-separated name, rejecting traversal and symlink escapes.
    pub fn path(&self, logical: &str) -> io::Result<PathBuf> {
        if logical.is_empty()
            || logical.contains('\\')
            || logical
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
            || Path::new(logical)
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err(invalid("resource name must be a relative logical path"));
        }
        let path = self.root.join(logical).canonicalize()?;
        if !path.starts_with(&self.root) {
            return Err(invalid("resource escapes root"));
        }
        Ok(path)
    }

    pub fn read(&self, logical: &str) -> io::Result<Vec<u8>> {
        fs::read(self.path(logical)?)
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn logical_paths_are_contained() {
        let root = env::temp_dir().join(format!("appcoat-runtime-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        fs::write(root.join("hello.txt"), "hello").unwrap();
        let resources = Resources::new(&root).unwrap();
        assert_eq!(resources.read("hello.txt").unwrap(), b"hello");
        for name in [
            "",
            "..",
            "../outside",
            "/hello.txt",
            "./hello.txt",
            "a//b",
            "a\\b",
        ] {
            assert!(resources.path(name).is_err(), "{name}");
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(env::temp_dir(), root.join("escape")).unwrap();
            assert!(resources.path("escape").is_err());
        }
        assert!(Resources::new("relative").is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
