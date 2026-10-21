use serde::{Deserialize, Serialize};

use crate::spec::{GuestSpec, Validation};

/// One kernel command-line argument. `key` is `None` for bare flags.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KernelArg {
    pub key: Option<String>,
    pub value: String,
}

impl KernelArg {
    pub fn flag(value: impl Into<String>) -> Self {
        Self {
            key: None,
            value: value.into(),
        }
    }

    pub fn keyed(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: Some(key.into()),
            value: value.into(),
        }
    }
}

impl std::fmt::Display for KernelArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.key {
            Some(key) => write!(f, "{key}={}", self.value),
            None => f.write_str(&self.value),
        }
    }
}

/// The exact executable and arguments a launcher should use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BootPlan {
    pub executable: String,
    pub args: Vec<String>,
    pub spec: GuestSpec,
}

impl BootPlan {
    /// Build a plan without validating it. Callers that accept untrusted input
    /// should use [`BootPlan::validated`].
    pub fn from_spec(spec: GuestSpec) -> Self {
        let executable = spec.kernel.clone();
        let args = kernel_args(&spec)
            .into_iter()
            .map(|arg| arg.to_string())
            .collect();
        Self {
            executable,
            args,
            spec,
        }
    }

    pub fn validated(spec: GuestSpec) -> Result<Self, Validation> {
        let validation = spec.validate();
        if validation.is_ok() {
            Ok(Self::from_spec(spec))
        } else {
            Err(validation)
        }
    }

    /// The full argv, executable first.
    pub fn argv(&self) -> Vec<String> {
        std::iter::once(self.executable.clone())
            .chain(self.args.iter().cloned())
            .collect()
    }

    /// A shell-quoted rendering for logs and for pasting into a terminal. This
    /// is never passed to a shell by the launcher.
    pub fn command_line(&self) -> String {
        self.argv()
            .iter()
            .map(|arg| shell_quote(arg))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Turn a spec into the ordered kernel command line.
///
/// Order is fixed so that plans compare equal and logs are reproducible.
pub fn kernel_args(spec: &GuestSpec) -> Vec<KernelArg> {
    let mut args = vec![
        KernelArg::keyed("mem", format!("{}M", spec.memory_mib)),
        KernelArg::keyed("ncpus", spec.cpus.to_string()),
    ];

    if let Some(console) = spec.console.kernel_arg() {
        args.push(KernelArg::flag(console));
    }
    if let Some(initramfs) = &spec.initramfs {
        args.push(KernelArg::keyed("initrd", initramfs));
    }
    if let Some(rootfs) = &spec.rootfs {
        args.push(KernelArg::keyed("ubd0", rootfs));
        args.push(KernelArg::flag("root=/dev/ubda"));
        args.push(KernelArg::flag("rw"));
    }
    if let Some(data) = &spec.data_disk {
        args.push(KernelArg::keyed("ubd1", data));
    }
    if let Some(share) = &spec.share {
        args.push(KernelArg::keyed("hostfs", &share.host_path));
    }
    if let Some(init) = &spec.init {
        args.push(KernelArg::keyed("init", init));
    }
    if let Some(stub) = &spec.stub_exe {
        args.push(KernelArg::keyed("stub_exe", stub));
    }
    args.push(KernelArg::flag("panic=-1"));
    args.extend(
        spec.mode
            .kernel_args()
            .iter()
            .map(|arg| KernelArg::flag(*arg)),
    );
    args.extend(spec.extra_args.iter().cloned().map(KernelArg::flag));
    args
}

fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._/:=".contains(&b))
    {
        return value.to_owned();
    }
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('\'');
    for ch in value.chars() {
        if ch == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(ch);
        }
    }
    quoted.push('\'');
    quoted
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{BootMode, ConsoleMode, Share};

    fn full_spec() -> GuestSpec {
        GuestSpec {
            kernel: "/k/linux".into(),
            initramfs: Some("/k/initrd.gz".into()),
            rootfs: Some("/k/root.ext4".into()),
            data_disk: Some("/k/data.ext4".into()),
            share: Some(Share {
                host_path: "/host share".into(),
            }),
            memory_mib: 1024,
            cpus: 2,
            init: Some("/init".into()),
            stub_exe: Some("/k/stub_exe".into()),
            console: ConsoleMode::Stdio,
            mode: BootMode::Seccomp,
            guest_page_size_kib: Some(16),
            extra_args: vec!["quiet".into()],
        }
    }

    #[test]
    fn argument_order_is_stable() {
        let plan = BootPlan::from_spec(full_spec());
        let expected: Vec<String> = [
            "mem=1024M",
            "ncpus=2",
            "con0=fd:0,fd:1",
            "initrd=/k/initrd.gz",
            "ubd0=/k/root.ext4",
            "root=/dev/ubda",
            "rw",
            "ubd1=/k/data.ext4",
            "hostfs=/host share",
            "init=/init",
            "stub_exe=/k/stub_exe",
            "panic=-1",
            "seccomp=on",
            "quiet",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        for (i, (a, b)) in plan.args.iter().zip(expected.iter()).enumerate() {
            assert_eq!(a, b, "arg {i} differs");
        }
        assert_eq!(plan.args.len(), expected.len());
    }

    #[test]
    fn modes_emit_their_switch() {
        let mut spec = full_spec();
        spec.mode = BootMode::PtraceNoSysemu;
        assert!(
            kernel_args(&spec)
                .iter()
                .any(|a| a.to_string() == "nosysemu")
        );
    }

    #[test]
    fn no_console_emits_no_con0() {
        let mut spec = full_spec();
        spec.console = ConsoleMode::None;
        assert!(
            !kernel_args(&spec)
                .iter()
                .any(|a| a.to_string().starts_with("con0"))
        );
    }

    #[test]
    fn command_line_quotes_spaces() {
        let plan = BootPlan::from_spec(full_spec());
        assert!(plan.command_line().contains("'hostfs=/host share'"));
        assert!(plan.command_line().starts_with("/k/linux mem=1024M"));
    }

    #[test]
    fn shell_quote_escapes_single_quotes() {
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
        assert_eq!(shell_quote("plain-1.0"), "plain-1.0");
        assert_eq!(shell_quote(""), "''");
    }

    #[test]
    fn validated_returns_errors() {
        let spec = GuestSpec::default();
        assert!(BootPlan::validated(spec).is_err());
    }
}
