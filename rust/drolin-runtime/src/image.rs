use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use drolin_core::ImageRef;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::paths::DataRoot;

/// Content-addressed by manifest name and version. A file is installed under
/// its final name only after its digest matches, so a partial download is never
/// mistaken for an image.
#[derive(Debug, Clone)]
pub struct ImageStore {
    root: PathBuf,
}

#[derive(Debug, Error)]
pub enum ImageError {
    #[error("invalid image manifest: {0}")]
    Invalid(String),
    #[error("cannot access {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("sha256 mismatch for {path}: expected {expected}, got {actual}")]
    DigestMismatch {
        path: PathBuf,
        expected: String,
        actual: String,
    },
    #[error("image is not installed: {0}")]
    NotInstalled(String),
}

impl ImageStore {
    pub fn new(data: &DataRoot) -> Self {
        Self {
            root: data.images(),
        }
    }

    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn path_of(&self, image: &ImageRef) -> PathBuf {
        self.root.join(image.file_name())
    }

    pub fn is_installed(&self, image: &ImageRef) -> bool {
        self.path_of(image).is_file()
    }

    /// Copy `source` into the store and verify the digest.
    pub fn install(&self, image: &ImageRef, source: &Path) -> Result<PathBuf, ImageError> {
        if !image.is_valid() {
            return Err(ImageError::Invalid(image.problems().join("; ")));
        }
        std::fs::create_dir_all(&self.root).map_err(|source| ImageError::Io {
            path: self.root.clone(),
            source,
        })?;
        let destination = self.path_of(image);
        if destination.is_file() {
            self.verify(image)?;
            return Ok(destination);
        }

        let temp = self.root.join(format!(
            ".{}.{}.part",
            image.file_name(),
            std::process::id()
        ));
        let result = self.copy_and_hash(source, &temp, image);
        match result {
            Ok(()) => {
                std::fs::rename(&temp, &destination).map_err(|source| ImageError::Io {
                    path: destination.clone(),
                    source,
                })?;
                Ok(destination)
            }
            Err(error) => {
                let _ = std::fs::remove_file(&temp);
                Err(error)
            }
        }
    }

    fn copy_and_hash(
        &self,
        source: &Path,
        temp: &Path,
        image: &ImageRef,
    ) -> Result<(), ImageError> {
        let mut input = File::open(source).map_err(|error| ImageError::Io {
            path: source.to_owned(),
            source: error,
        })?;
        let mut output = File::create(temp).map_err(|error| ImageError::Io {
            path: temp.to_owned(),
            source: error,
        })?;
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let read = input.read(&mut buffer).map_err(|error| ImageError::Io {
                path: source.to_owned(),
                source: error,
            })?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
            output
                .write_all(&buffer[..read])
                .map_err(|error| ImageError::Io {
                    path: temp.to_owned(),
                    source: error,
                })?;
        }
        output.flush().map_err(|error| ImageError::Io {
            path: temp.to_owned(),
            source: error,
        })?;
        let actual = format!("{:x}", hasher.finalize());
        if !actual.eq_ignore_ascii_case(&image.sha256) {
            return Err(ImageError::DigestMismatch {
                path: temp.to_owned(),
                expected: image.sha256.clone(),
                actual,
            });
        }
        Ok(())
    }

    pub fn verify(&self, image: &ImageRef) -> Result<(), ImageError> {
        let path = self.path_of(image);
        if !path.is_file() {
            return Err(ImageError::NotInstalled(image.name.clone()));
        }
        let digest = sha256_file(&path)?;
        if !digest.eq_ignore_ascii_case(&image.sha256) {
            return Err(ImageError::DigestMismatch {
                path,
                expected: image.sha256.clone(),
                actual: digest,
            });
        }
        Ok(())
    }

    pub fn list(&self) -> io::Result<Vec<PathBuf>> {
        let mut entries = Vec::new();
        if !self.root.is_dir() {
            return Ok(entries);
        }
        for entry in std::fs::read_dir(&self.root)? {
            let path = entry?.path();
            if path.is_file() {
                entries.push(path);
            }
        }
        entries.sort();
        Ok(entries)
    }
}

pub fn sha256_file(path: &Path) -> Result<String, ImageError> {
    let mut file = File::open(path).map_err(|source| ImageError::Io {
        path: path.to_owned(),
        source,
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|source| ImageError::Io {
            path: path.to_owned(),
            source,
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use drolin_core::Compression;

    fn image() -> ImageRef {
        ImageRef {
            name: "root".into(),
            version: "1".into(),
            url: "https://example.invalid/root.ext4".into(),
            sha256: "0".repeat(64),
            size_bytes: 4,
            compression: Compression::None,
        }
    }

    #[test]
    fn installs_and_verifies() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        std::fs::write(&source, b"data").unwrap();
        let digest = sha256_file(&source).unwrap();
        let mut image = image();
        image.sha256 = digest;
        let store = ImageStore::at(dir.path().join("store"));
        let installed = store.install(&image, &source).unwrap();
        assert_eq!(std::fs::read(&installed).unwrap(), b"data");
        store.verify(&image).unwrap();
        assert!(store.is_installed(&image));
    }

    #[test]
    fn rejects_a_bad_digest_and_leaves_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        std::fs::write(&source, b"data").unwrap();
        let store = ImageStore::at(dir.path().join("store"));
        let error = store.install(&image(), &source).unwrap_err();
        assert!(matches!(error, ImageError::DigestMismatch { .. }));
        assert!(!store.is_installed(&image()));
    }

    #[test]
    fn rejects_an_invalid_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let store = ImageStore::at(dir.path().join("store"));
        let mut broken = image();
        broken.sha256 = "nope".into();
        let error = store.install(&broken, dir.path()).unwrap_err();
        assert!(matches!(error, ImageError::Invalid(_)));
    }

    #[test]
    fn list_is_sorted_and_ignores_directories() {
        let dir = tempfile::tempdir().unwrap();
        let store = ImageStore::at(dir.path());
        std::fs::write(dir.path().join("b"), b"").unwrap();
        std::fs::write(dir.path().join("a"), b"").unwrap();
        std::fs::create_dir(dir.path().join("nested")).unwrap();
        assert_eq!(
            store.list().unwrap(),
            vec![dir.path().join("a"), dir.path().join("b")]
        );
    }
}
