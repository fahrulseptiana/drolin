use serde::{Deserialize, Serialize};

/// Configuration for the userspace network path.
///
/// UML's vector `fd` transport carries one Ethernet frame per datagram. passt
/// speaks qemu's length-prefixed stream protocol, so the runtime owns a small
/// bridge between them. None of this needs a tap device or any capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ForwardProtocol {
    Tcp,
    Udp,
}

impl ForwardProtocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tcp => "tcp",
            Self::Udp => "udp",
        }
    }
}

/// A host port exposed to the guest through passt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortForward {
    pub protocol: ForwardProtocol,
    pub host_port: u16,
    pub guest_port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Path to the passt executable.
    pub passt: String,
    pub mac: String,
    pub address: String,
    pub netmask: String,
    pub gateway: String,
    pub dns: Option<String>,
    /// Optional host interface to mirror (`passt -i`).
    pub interface: Option<String>,
    /// Host-to-guest port forwards (`passt -t`/`-u`).
    #[serde(default)]
    pub forwards: Vec<PortForward>,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            passt: "passt".into(),
            mac: "02:00:00:00:00:01".into(),
            address: "10.0.2.15".into(),
            netmask: "24".into(),
            gateway: "10.0.2.2".into(),
            dns: Some("1.1.1.1".into()),
            interface: None,
            forwards: Vec::new(),
        }
    }
}

impl NetworkConfig {
    pub fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if self.passt.trim().is_empty() {
            problems.push("passt path is empty".into());
        }
        if !valid_mac(&self.mac) {
            problems.push(format!("invalid MAC address {:?}", self.mac));
        }
        for (name, value) in [
            ("address", &self.address),
            ("netmask", &self.netmask),
            ("gateway", &self.gateway),
        ] {
            if value.trim().is_empty() || value.contains(',') || value.contains('\n') {
                problems.push(format!("invalid network {name}"));
            }
        }
        if let Some(dns) = &self.dns
            && (dns.trim().is_empty() || dns.contains(',') || dns.contains('\n'))
        {
            problems.push("invalid DNS server".into());
        }
        if let Some(interface) = &self.interface
            && (interface.trim().is_empty() || interface.contains(',') || interface.contains('\n'))
        {
            problems.push("invalid network interface".into());
        }
        problems
    }
}

fn valid_mac(value: &str) -> bool {
    let parts: Vec<&str> = value.split(':').collect();
    parts.len() == 6
        && parts
            .iter()
            .all(|part| part.len() == 2 && part.bytes().all(|b| b.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        assert!(NetworkConfig::default().problems().is_empty());
    }

    #[test]
    fn rejects_a_bad_mac() {
        let config = NetworkConfig {
            mac: "not-a-mac".into(),
            ..NetworkConfig::default()
        };
        assert!(!config.problems().is_empty());
    }

    #[test]
    fn rejects_commas_in_values() {
        let config = NetworkConfig {
            dns: Some("1.1.1.1,8.8.8.8".into()),
            ..NetworkConfig::default()
        };
        assert!(!config.problems().is_empty());
    }
}
