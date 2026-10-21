use std::fs;

use drolin_core::{Arch, HostFacts};

/// Read the host facts the planner cares about. Missing pseudo-files are not
/// errors: a container may hide them, and the planner only uses them to warn.
pub fn host_facts() -> HostFacts {
    let arch = Arch::host();
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    let page_size = if page_size > 0 {
        page_size as u32
    } else {
        4096
    };
    HostFacts {
        arch,
        page_size,
        ptrace_scope: read_ptrace_scope(),
        seccomp_available: read_seccomp_available(),
        hostname: fs::read_to_string("/proc/sys/kernel/hostname")
            .ok()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty()),
    }
}

fn read_ptrace_scope() -> Option<u8> {
    fs::read_to_string("/proc/sys/kernel/yama/ptrace_scope")
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

fn read_seccomp_available() -> bool {
    fs::read_to_string("/proc/self/status")
        .map(|status| {
            status
                .lines()
                .find_map(|line| line.strip_prefix("Seccomp:"))
                .map(|value| value.trim() != "0")
                .unwrap_or(false)
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_size_is_plausible() {
        let facts = host_facts();
        assert!(facts.page_size >= 4096);
        assert_eq!(facts.page_size % 1024, 0);
    }

    #[test]
    fn arch_is_known() {
        assert!(!matches!(host_facts().arch, Arch::Other(_)));
    }
}
