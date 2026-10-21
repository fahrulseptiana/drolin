use serde::{Deserialize, Serialize};

/// An instruction-set architecture that a guest kernel can target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Arch {
    Aarch64,
    X86_64,
    Riscv64,
    Other(String),
}

impl Arch {
    pub fn host() -> Self {
        Self::from_machine_name(std::env::consts::ARCH)
    }

    pub fn from_machine_name(name: &str) -> Self {
        match name {
            "aarch64" | "arm64" => Self::Aarch64,
            "x86_64" | "amd64" => Self::X86_64,
            "riscv64" => Self::Riscv64,
            other => Self::Other(other.to_owned()),
        }
    }

    /// The `e_machine` value of an ELF header for this architecture.
    pub fn elf_machine(&self) -> Option<u16> {
        match self {
            Self::Aarch64 => Some(183),
            Self::X86_64 => Some(62),
            Self::Riscv64 => Some(243),
            Self::Other(_) => None,
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Aarch64 => "aarch64",
            Self::X86_64 => "x86_64",
            Self::Riscv64 => "riscv64",
            Self::Other(name) => name,
        }
    }

    /// A User-Mode Linux binary is an ordinary process, so it must match the
    /// host architecture exactly. There is no cross-architecture execution.
    pub fn matches_host(&self) -> bool {
        self == &Self::host()
    }
}

/// Facts about the machine that will host the guest.
///
/// Kept as plain data so validation stays pure. `drolin-runtime` fills this in
/// from the operating system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostFacts {
    pub arch: Arch,
    pub page_size: u32,
    /// `None` when the host does not expose the knob.
    pub ptrace_scope: Option<u8>,
    pub seccomp_available: bool,
    pub hostname: Option<String>,
}

impl HostFacts {
    pub fn new(arch: Arch, page_size: u32) -> Self {
        Self {
            arch,
            page_size,
            ptrace_scope: None,
            seccomp_available: false,
            hostname: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_names_are_normalised() {
        assert_eq!(Arch::from_machine_name("arm64"), Arch::Aarch64);
        assert_eq!(Arch::from_machine_name("aarch64"), Arch::Aarch64);
        assert_eq!(Arch::from_machine_name("amd64"), Arch::X86_64);
        assert_eq!(Arch::from_machine_name("mips"), Arch::Other("mips".into()));
    }

    #[test]
    fn elf_machines_are_the_abi_values() {
        assert_eq!(Arch::Aarch64.elf_machine(), Some(183));
        assert_eq!(Arch::X86_64.elf_machine(), Some(62));
        assert_eq!(Arch::Riscv64.elf_machine(), Some(243));
    }
}
