use serde::{Deserialize, Serialize};

/// A downloadable guest image. The manifest is data, not a downloader; the
/// runtime verifies the digest after transfer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageRef {
    pub name: String,
    pub version: String,
    pub url: String,
    pub sha256: String,
    pub size_bytes: u64,
    #[serde(default)]
    pub compression: Compression,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Compression {
    #[default]
    None,
    Gzip,
    Xz,
    Zstd,
}

impl Compression {
    pub fn extension(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Gzip => ".gz",
            Self::Xz => ".xz",
            Self::Zstd => ".zst",
        }
    }
}

impl ImageRef {
    pub fn file_name(&self) -> String {
        format!(
            "{}-{}{}",
            self.name,
            self.version,
            self.compression.extension()
        )
    }

    /// Returns a list of human-readable problems. Empty means valid.
    pub fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if self.name.is_empty()
            || !self
                .name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-._".contains(&b))
        {
            problems.push(format!(
                "name must be lowercase [a-z0-9._-], got {:?}",
                self.name
            ));
        }
        if self.version.is_empty()
            || !self
                .version
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            problems.push(format!(
                "version must be [A-Za-z0-9._-], got {:?}",
                self.version
            ));
        }
        if !self.url.starts_with("https://") && !self.url.starts_with("http://") {
            problems.push(format!("url must be http(s), got {:?}", self.url));
        }
        if self.sha256.len() != 64 || !self.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            problems.push("sha256 must be 64 hexadecimal characters".into());
        }
        if self.size_bytes == 0 {
            problems.push("size_bytes must be positive".into());
        }
        problems
    }

    pub fn is_valid(&self) -> bool {
        self.problems().is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageSet {
    #[serde(default)]
    pub images: Vec<ImageRef>,
}

impl ImageSet {
    pub fn find(&self, name: &str) -> Option<&ImageRef> {
        self.images.iter().find(|i| i.name == name)
    }

    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        for image in &self.images {
            for problem in image.problems() {
                problems.push(format!("{}: {problem}", image.name));
            }
        }
        for (index, image) in self.images.iter().enumerate() {
            if self.images[..index]
                .iter()
                .any(|other| other.name == image.name)
            {
                problems.push(format!("duplicate image name {:?}", image.name));
            }
        }
        problems
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image() -> ImageRef {
        ImageRef {
            name: "alpine-mini".into(),
            version: "7.2.12".into(),
            url: "https://example.invalid/image.tar.gz".into(),
            sha256: "a".repeat(64),
            size_bytes: 10,
            compression: Compression::Gzip,
        }
    }

    #[test]
    fn a_good_image_has_no_problems() {
        assert!(image().is_valid());
    }

    #[test]
    fn file_names_include_compression() {
        assert_eq!(image().file_name(), "alpine-mini-7.2.12.gz");
    }

    #[test]
    fn digests_are_checked_lexically() {
        let mut i = image();
        i.sha256 = "abc".into();
        assert!(!i.is_valid());
    }

    #[test]
    fn traversal_in_names_is_rejected() {
        let mut i = image();
        i.name = "../etc".into();
        assert!(!i.is_valid());
    }

    #[test]
    fn duplicate_names_are_reported() {
        let set = ImageSet {
            images: vec![image(), image()],
        };
        assert!(set.validate().iter().any(|p| p.contains("duplicate")));
    }

    #[test]
    fn lookup_is_by_name() {
        let set = ImageSet {
            images: vec![image()],
        };
        assert!(set.find("alpine-mini").is_some());
        assert!(set.find("other").is_none());
    }
}
