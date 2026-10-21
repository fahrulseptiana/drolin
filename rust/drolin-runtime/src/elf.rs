use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use drolin_core::Arch;
use thiserror::Error;

/// The useful part of an ELF header, without pulling in an ELF crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelInfo {
    pub path: PathBuf,
    pub arch: Option<Arch>,
    pub is_64_bit: bool,
    pub is_little_endian: bool,
    pub size_bytes: u64,
}

#[derive(Debug, Error)]
pub enum ElfError {
    #[error("cannot read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{path} is not an ELF file")]
    NotElf { path: PathBuf },
    #[error("{path} is truncated")]
    Truncated { path: PathBuf },
}

/// Read only the 64-byte ELF header. A kernel binary can be hundreds of
/// megabytes; reading it to answer "what is this?" would be wasteful.
pub fn inspect(path: &Path) -> Result<KernelInfo, ElfError> {
    let mut file = File::open(path).map_err(|source| ElfError::Read {
        path: path.to_owned(),
        source,
    })?;
    let mut header = [0u8; 64];
    let read = file.read(&mut header).map_err(|source| ElfError::Read {
        path: path.to_owned(),
        source,
    })?;
    if read < 4 {
        return Err(ElfError::Truncated {
            path: path.to_owned(),
        });
    }
    if &header[..4] != b"\x7fELF" {
        return Err(ElfError::NotElf {
            path: path.to_owned(),
        });
    }
    if read < 20 {
        return Err(ElfError::Truncated {
            path: path.to_owned(),
        });
    }
    let is_64_bit = header[4] == 2;
    let is_little_endian = header[5] == 1;
    let machine = if is_little_endian {
        u16::from_le_bytes([header[18], header[19]])
    } else {
        u16::from_be_bytes([header[18], header[19]])
    };
    let arch = match machine {
        183 => Some(Arch::Aarch64),
        62 => Some(Arch::X86_64),
        243 => Some(Arch::Riscv64),
        _ => None,
    };
    let size_bytes = file
        .metadata()
        .map_err(|source| ElfError::Read {
            path: path.to_owned(),
            source,
        })?
        .len();
    Ok(KernelInfo {
        path: path.to_owned(),
        arch,
        is_64_bit,
        is_little_endian,
        size_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn fake_elf(machine: u16) -> Vec<u8> {
        let mut header = vec![0u8; 64];
        header[..4].copy_from_slice(b"\x7fELF");
        header[4] = 2;
        header[5] = 1;
        header[6] = 1;
        header[18..20].copy_from_slice(&machine.to_le_bytes());
        header
    }

    #[test]
    fn reads_an_aarch64_header() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("linux");
        File::create(&path).unwrap().write_all(&fake_elf(183)).unwrap();
        let info = inspect(&path).unwrap();
        assert_eq!(info.arch, Some(Arch::Aarch64));
        assert!(info.is_64_bit);
        assert_eq!(info.size_bytes, 64);
    }

    #[test]
    fn reads_an_x86_64_header() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("linux");
        File::create(&path).unwrap().write_all(&fake_elf(62)).unwrap();
        assert_eq!(inspect(&path).unwrap().arch, Some(Arch::X86_64));
    }

    #[test]
    fn rejects_non_elf() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("linux");
        File::create(&path).unwrap().write_all(b"#!/bin/sh\n").unwrap();
        assert!(matches!(inspect(&path), Err(ElfError::NotElf { .. })));
    }

    #[test]
    fn rejects_truncated_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("linux");
        File::create(&path).unwrap().write_all(b"\x7fELF").unwrap();
        assert!(matches!(inspect(&path), Err(ElfError::Truncated { .. })));
    }
}
