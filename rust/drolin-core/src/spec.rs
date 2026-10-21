use serde::{Deserialize, Serialize};

use crate::target::{Arch, HostFacts};

/// What the guest should use as its console.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsoleMode {
    /// The guest shares the launcher's standard input and output.
    #[default]
    Stdio,
    /// The guest gets a pseudoterminal allocated by the host kernel.
    Pty,
    /// No console argument is emitted; useful with a fully scripted guest.
    None,
}

impl ConsoleMode {
    pub fn kernel_arg(self) -> Option<&'static str> {
        match self {
            Self::Stdio => Some("con0=fd:0,fd:1"),
            Self::Pty => Some("con0=pty"),
            Self::None => None,
        }
    }
}

/// How the host is asked to intercept guest system calls.
///
/// `Ptrace` is the portable default. `PtraceNoSysemu` is the fallback for
/// kernels that do not implement `PTRACE_SYSEMU`. `Seccomp` is the fast path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BootMode {
    #[default]
    Ptrace,
    PtraceNoSysemu,
    Seccomp,
}

impl BootMode {
    pub fn kernel_args(self) -> &'static [&'static str] {
        match self {
            Self::Ptrace => &[],
            Self::PtraceNoSysemu => &["nosysemu"],
            Self::Seccomp => &["seccomp=on"],
        }
    }
}

/// A host directory exposed to the guest through `hostfs`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Share {
    pub host_path: String,
}

/// A request to boot a guest. This is the input to the planner and is also the
/// document the CLI accepts as JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GuestSpec {
    /// Host path to the User-Mode Linux kernel binary.
    pub kernel: String,
    /// Optional initramfs image.
    pub initramfs: Option<String>,
    /// Optional root filesystem image, attached as `/dev/ubda`.
    pub rootfs: Option<String>,
    /// Optional second disk, attached as `/dev/ubdb`.
    pub data_disk: Option<String>,
    /// Optional host directory mounted through hostfs.
    pub share: Option<Share>,
    pub memory_mib: u32,
    pub cpus: u16,
    /// Guest path executed as PID 1. `None` leaves the kernel default.
    pub init: Option<String>,
    pub console: ConsoleMode,
    pub mode: BootMode,
    /// Guest page size in KiB when the caller knows it. Validated against the
    /// host page size by `validate`.
    pub guest_page_size_kib: Option<u32>,
    /// Extra kernel arguments. Arguments that the planner owns are rejected.
    pub extra_args: Vec<String>,
}

impl Default for GuestSpec {
    fn default() -> Self {
        Self {
            kernel: String::new(),
            initramfs: None,
            rootfs: None,
            data_disk: None,
            share: None,
            memory_mib: 512,
            cpus: 1,
            init: None,
            console: ConsoleMode::Stdio,
            mode: BootMode::Ptrace,
            guest_page_size_kib: None,
            extra_args: Vec::new(),
        }
    }
}

impl GuestSpec {
    pub fn validate(&self) -> Validation {
        self.validate_on(None)
    }

    pub fn validate_on(&self, host: Option<&HostFacts>) -> Validation {
        let mut issues = Vec::new();

        if self.kernel.trim().is_empty() {
            issues.push(Issue::error("kernel.missing", "a kernel binary is required".into()));
        }
        if !(32..=262_144).contains(&self.memory_mib) {
            error(&mut issues, 
                "memory.range",
                format!("memory must be 32..=262144 MiB, got {}", self.memory_mib),
            );
        }
        if !(1..=64).contains(&self.cpus) {
            error(&mut issues, 
                "cpus.range",
                format!("cpus must be 1..=64, got {}", self.cpus),
            );
        }
        if let Some(init) = &self.init {
            if !init.starts_with('/') {
                error(&mut issues, 
                    "init.absolute",
                    format!("init must be an absolute guest path, got {init}"),
                );
            }
        }
        for (field, path) in [
            ("kernel", Some(&self.kernel)),
            ("initramfs", self.initramfs.as_ref()),
            ("rootfs", self.rootfs.as_ref()),
            ("data_disk", self.data_disk.as_ref()),
            ("share", self.share.as_ref().map(|s| &s.host_path)),
        ] {
            let Some(path) = path else { continue };
            if path.trim().is_empty() {
                issues.push(Issue::error("path.empty", format!("{field} path is empty")));
            }
            if path.contains(',') {
                error(&mut issues, 
                    "path.comma",
                    format!("{field} path contains a comma, which User-Mode Linux parses as a separator: {path}"),
                );
            }
            if path.contains('\n') || path.contains('\0') {
                error(&mut issues, 
                    "path.control",
                    format!("{field} path contains a control character"),
                );
            }
        }
        if self.initramfs.is_some() && self.rootfs.is_some() {
            warn(&mut issues, 
                "boot.rootfs-and-initramfs",
                "both an initramfs and a root filesystem are set; the root filesystem is what the guest will mount".into(),
            );
        }
        if self.guest_page_size_kib == Some(0) {
            issues.push(Issue::error("page_size.zero", "guest page size must be positive".into()));
        }
        if let (Some(guest), Some(host)) = (self.guest_page_size_kib, host) {
            let host_kib = host.page_size / 1024;
            if guest < host_kib {
                error(&mut issues, 
                    "page_size.too_small",
                    format!(
                        "guest pages are {guest} KiB but the host uses {host_kib} KiB; a guest page is a host mmap and cannot be smaller"
                    ),
                );
            }
        }

        for arg in &self.extra_args {
            if arg.trim().is_empty() {
                issues.push(Issue::error("extra.empty", "an extra kernel argument is empty".into()));
                continue;
            }
            if arg.contains('\0') || arg.contains('\n') {
                error(&mut issues, 
                    "extra.control",
                    format!("extra kernel argument contains a control character: {arg:?}"),
                );
            }
            if let Some(key) = arg.split('=').next() {
                if is_reserved_key(key) {
                    error(&mut issues, 
                        "extra.reserved",
                        format!("{key}= is owned by the planner; use the typed option instead"),
                    );
                }
            }
        }

        if let Some(host) = host {
            if let Some(machine) = host.arch.elf_machine() {
                let _ = machine;
            }
            if matches!(host.arch, Arch::Other(_)) {
                warn(&mut issues, 
                    "host.unknown-arch",
                    format!("unknown host architecture: {}", host.arch.as_str()),
                );
            }
        }

        Validation { issues }
    }

    /// A stable identity for the request, used by callers that cache plans.
    pub fn cache_key(&self) -> String {
        serde_json::to_string(self).expect("GuestSpec is always serialisable")
    }
}

fn error(issues: &mut Vec<Issue>, code: &'static str, message: String) {
    issues.push(Issue::error(code, message));
}

fn warn(issues: &mut Vec<Issue>, code: &'static str, message: String) {
    issues.push(Issue::warning(code, message));
}

fn is_reserved_key(key: &str) -> bool {
    matches!(
        key,
        "mem"
            | "ncpus"
            | "con0"
            | "initrd"
            | "ubd0"
            | "ubd1"
            | "root"
            | "rw"
            | "init"
            | "hostfs"
            | "seccomp"
            | "nosysemu"
            | "panic"
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub severity: Severity,
    pub code: String,
    pub message: String,
}

impl Issue {
    pub fn error(code: impl Into<String>, message: String) -> Self {
        Self {
            severity: Severity::Error,
            code: code.into(),
            message,
        }
    }

    pub fn warning(code: impl Into<String>, message: String) -> Self {
        Self {
            severity: Severity::Warning,
            code: code.into(),
            message,
        }
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Validation {
    pub issues: Vec<Issue>,
}

impl Validation {
    pub fn is_ok(&self) -> bool {
        !self.issues.iter().any(Issue::is_error)
    }

    pub fn errors(&self) -> impl Iterator<Item = &Issue> {
        self.issues.iter().filter(|i| i.is_error())
    }

    pub fn warnings(&self) -> impl Iterator<Item = &Issue> {
        self.issues.iter().filter(|i| !i.is_error())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> GuestSpec {
        GuestSpec {
            kernel: "/tmp/linux".into(),
            ..GuestSpec::default()
        }
    }

    #[test]
    fn a_minimal_spec_is_valid() {
        assert!(spec().validate().is_ok());
    }

    #[test]
    fn reserved_arguments_are_rejected() {
        let mut s = spec();
        s.extra_args.push("mem=4096".into());
        let v = s.validate();
        assert!(!v.is_ok());
        assert!(v.issues.iter().any(|i| i.code == "extra.reserved"));
    }

    #[test]
    fn flags_without_values_are_allowed() {
        let mut s = spec();
        s.extra_args.push("quiet".into());
        assert!(s.validate().is_ok());
    }

    #[test]
    fn commas_in_device_paths_are_rejected() {
        let mut s = spec();
        s.rootfs = Some("/tmp/root,backup.ext4".into());
        let v = s.validate();
        assert!(!v.is_ok());
        assert!(v.issues.iter().any(|i| i.code == "path.comma"));
    }

    #[test]
    fn init_must_be_absolute() {
        let mut s = spec();
        s.init = Some("init".into());
        assert!(!s.validate().is_ok());
    }

    #[test]
    fn guest_pages_may_not_be_smaller_than_host_pages() {
        let mut s = spec();
        s.guest_page_size_kib = Some(4);
        let host = HostFacts::new(Arch::Aarch64, 16 * 1024);
        let v = s.validate_on(Some(&host));
        assert!(!v.is_ok());
        assert!(v.issues.iter().any(|i| i.code == "page_size.too_small"));
    }

    #[test]
    fn guest_pages_may_be_larger_than_host_pages() {
        let mut s = spec();
        s.guest_page_size_kib = Some(16);
        let host = HostFacts::new(Arch::Aarch64, 4 * 1024);
        assert!(s.validate_on(Some(&host)).is_ok());
    }

    #[test]
    fn range_checks_are_reported_together() {
        let mut s = spec();
        s.memory_mib = 1;
        s.cpus = 0;
        let v = s.validate();
        assert!(v.issues.iter().any(|i| i.code == "memory.range"));
        assert!(v.issues.iter().any(|i| i.code == "cpus.range"));
    }
}
