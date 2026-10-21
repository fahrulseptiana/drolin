use std::path::Path;

use drolin_core::{BootMode, GuestSpec, HostFacts};
use serde::{Deserialize, Serialize};

use crate::elf;
use crate::host::host_facts;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    pub name: String,
    pub status: Status,
    pub detail: String,
}

impl Check {
    pub fn pass(name: &str, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: Status::Pass,
            detail: detail.into(),
        }
    }

    pub fn warn(name: &str, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: Status::Warn,
            detail: detail.into(),
        }
    }

    pub fn fail(name: &str, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: Status::Fail,
            detail: detail.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnosis {
    pub checks: Vec<Check>,
}

impl Diagnosis {
    pub fn ok(&self) -> bool {
        !self.checks.iter().any(|c| c.status == Status::Fail)
    }

    pub fn failures(&self) -> impl Iterator<Item = &Check> {
        self.checks.iter().filter(|c| c.status == Status::Fail)
    }
}

/// Answer "will this boot?" without booting. The checks are ordered from the
/// most common mistake to the least so the first failure is usually the one
/// worth reading.
pub fn diagnose(spec: &GuestSpec) -> Diagnosis {
    let facts = host_facts();
    diagnose_on(spec, &facts)
}

pub fn diagnose_on(spec: &GuestSpec, facts: &HostFacts) -> Diagnosis {
    let mut checks = Vec::new();

    checks.push(Check::pass(
        "host.arch",
        format!(
            "{} ({} KiB pages)",
            facts.arch.as_str(),
            facts.page_size / 1024
        ),
    ));

    match elf::inspect(Path::new(&spec.kernel)) {
        Ok(info) => {
            match &info.arch {
                Some(arch) if arch == &facts.arch => checks.push(Check::pass(
                    "kernel.arch",
                    format!("{} matches the host", arch.as_str()),
                )),
                Some(arch) => checks.push(Check::fail(
                    "kernel.arch",
                    format!(
                        "kernel is {} but the host is {}; User-Mode Linux cannot execute across architectures",
                        arch.as_str(),
                        facts.arch.as_str()
                    ),
                )),
                None => checks.push(Check::warn(
                    "kernel.arch",
                    "ELF machine is not one drolin recognises",
                )),
            }
            checks.push(Check::pass(
                "kernel.elf",
                format!(
                    "{} MiB, {}-bit {} endian",
                    info.size_bytes / (1024 * 1024),
                    if info.is_64_bit { 64 } else { 32 },
                    if info.is_little_endian {
                        "little"
                    } else {
                        "big"
                    }
                ),
            ));
        }
        Err(error) => checks.push(Check::fail("kernel.elf", error.to_string())),
    }

    check_input(&mut checks, "initramfs", spec.initramfs.as_deref());
    check_input(&mut checks, "rootfs", spec.rootfs.as_deref());
    check_input(&mut checks, "data_disk", spec.data_disk.as_deref());
    check_input(&mut checks, "stub_exe", spec.stub_exe.as_deref());
    if let Some(share) = &spec.share {
        if Path::new(&share.host_path).is_dir() {
            checks.push(Check::pass("share", "host directory exists"));
        } else {
            checks.push(Check::fail("share", "share is not a directory"));
        }
    }

    match facts.ptrace_scope {
        Some(0) | Some(1) => checks.push(Check::pass(
            "host.ptrace",
            format!("yama ptrace_scope={}", facts.ptrace_scope.unwrap_or(0)),
        )),
        Some(scope) => checks.push(Check::warn(
            "host.ptrace",
            format!("yama ptrace_scope={scope}; the guest may be unable to trace its own children"),
        )),
        None => checks.push(Check::pass("host.ptrace", "no yama restriction visible")),
    }

    if spec.mode == BootMode::Seccomp {
        if facts.seccomp_available {
            checks.push(Check::pass("host.seccomp", "seccomp filters are available"));
        } else {
            checks.push(Check::fail(
                "host.seccomp",
                "seccomp mode was requested but the host reports no seccomp support",
            ));
        }
    }

    let validation = spec.validate_on(Some(facts));
    for issue in validation.issues {
        let status = if issue.is_error() {
            Status::Fail
        } else {
            Status::Warn
        };
        checks.push(Check {
            name: "plan".into(),
            status,
            detail: format!("{}: {}", issue.code, issue.message),
        });
    }

    Diagnosis { checks }
}

fn check_input(checks: &mut Vec<Check>, name: &'static str, path: Option<&str>) {
    let Some(path) = path else { return };
    let path = Path::new(path);
    if !path.exists() {
        checks.push(Check::fail(
            name,
            format!("{} does not exist", path.display()),
        ));
    } else if !path.is_file() {
        checks.push(Check::fail(
            name,
            format!("{} is not a file", path.display()),
        ));
    } else {
        checks.push(Check::pass(name, format!("{} is readable", path.display())));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use drolin_core::{Arch, BootMode};

    fn spec() -> GuestSpec {
        GuestSpec {
            kernel: "/definitely/missing/linux".into(),
            ..GuestSpec::default()
        }
    }

    #[test]
    fn a_missing_kernel_is_a_failure() {
        let diagnosis = diagnose_on(&spec(), &HostFacts::new(Arch::host(), 4096));
        assert!(!diagnosis.ok());
        assert!(diagnosis.failures().any(|c| c.name == "kernel.elf"));
    }

    #[test]
    fn seccomp_mode_requires_seccomp() {
        let mut spec = spec();
        spec.mode = BootMode::Seccomp;
        let mut facts = HostFacts::new(Arch::host(), 4096);
        facts.seccomp_available = false;
        let diagnosis = diagnose_on(&spec, &facts);
        assert!(diagnosis.failures().any(|c| c.name == "host.seccomp"));
    }

    #[test]
    fn arch_mismatch_is_explained() {
        use std::io::Write;
        let other = if Arch::host() == Arch::Aarch64 {
            Arch::X86_64
        } else {
            Arch::Aarch64
        };
        let machine = Arch::host().elf_machine().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let kernel = dir.path().join("linux");
        let mut header = vec![0u8; 64];
        header[..4].copy_from_slice(b"\x7fELF");
        header[4] = 2;
        header[5] = 1;
        header[18..20].copy_from_slice(&machine.to_le_bytes());
        std::fs::File::create(&kernel)
            .unwrap()
            .write_all(&header)
            .unwrap();
        let mut spec = spec();
        spec.kernel = kernel.display().to_string();
        let diagnosis = diagnose_on(&spec, &HostFacts::new(other, 4096));
        assert!(diagnosis.failures().any(|c| c.name == "kernel.arch"));
    }
}
