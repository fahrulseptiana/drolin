use std::env;
use std::path::{Path, PathBuf};

use thiserror::Error;

/// The one directory drolin owns. Everything it writes is below this path, so
/// uninstalling is a single recursive delete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataRoot {
    root: PathBuf,
}

#[derive(Debug, Error)]
pub enum PathError {
    #[error("neither DROLIN_HOME, XDG_DATA_HOME nor HOME is set")]
    NoHome,
    #[error("cannot create {path}: {source}")]
    Create {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

impl DataRoot {
    pub fn from_env() -> Result<Self, PathError> {
        if let Some(value) = env::var_os("DROLIN_HOME") {
            return Ok(Self::at(value));
        }
        if let Some(value) = env::var_os("XDG_DATA_HOME") {
            return Ok(Self::at(PathBuf::from(value).join("drolin")));
        }
        if let Some(value) = env::var_os("HOME") {
            return Ok(Self::at(
                PathBuf::from(value)
                    .join(".local")
                    .join("share")
                    .join("drolin"),
            ));
        }
        Err(PathError::NoHome)
    }

    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn images(&self) -> PathBuf {
        self.root.join("images")
    }

    pub fn logs(&self) -> PathBuf {
        self.root.join("logs")
    }

    pub fn ensure(&self) -> Result<(), PathError> {
        for path in [self.root.clone(), self.images(), self.logs()] {
            std::fs::create_dir_all(&path).map_err(|source| PathError::Create {
                path: path.clone(),
                source,
            })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directories_are_below_the_root() {
        let root = DataRoot::at("/tmp/drolin");
        assert_eq!(root.images(), PathBuf::from("/tmp/drolin/images"));
        assert_eq!(root.logs(), PathBuf::from("/tmp/drolin/logs"));
    }

    #[test]
    fn ensure_creates_the_tree() {
        let dir = tempfile::tempdir().unwrap();
        let root = DataRoot::at(dir.path().join("data"));
        root.ensure().unwrap();
        assert!(root.images().is_dir());
        assert!(root.logs().is_dir());
    }
}
