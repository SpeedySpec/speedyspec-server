use std::net::IpAddr;

use ipnet::IpNet;

use crate::settings::SettingsIssue;

/// A set of addresses and networks (a Specification over client addresses).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IpSet {
    nets: Vec<IpNet>,
}

impl IpSet {
    /// One IP or CIDR per line; blank lines and lines starting with `#` are ignored. A line that is neither is
    /// skipped and reported under the setting name `"ip"`; callers rename it.
    pub fn parse(text: &str) -> (IpSet, Vec<SettingsIssue>) {
        let mut nets = Vec::new();
        let mut issues = Vec::new();
        for (index, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parsed = line
                .parse::<IpNet>()
                .ok()
                .or_else(|| line.parse::<IpAddr>().ok().map(IpNet::from))
                .map(canonical_net);
            match parsed {
                Some(net) => nets.push(net),
                None => issues.push(SettingsIssue {
                    setting: "ip",
                    line: index + 1,
                    message: format!("not an IP address or CIDR network: {line:?}"),
                }),
            }
        }
        (IpSet { nets }, issues)
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        let ip = ip.to_canonical();
        self.nets.iter().any(|net| net.contains(&ip))
    }

    pub fn is_empty(&self) -> bool {
        self.nets.is_empty()
    }
}

/// `::ffff:10.0.0.0/120` becomes `10.0.0.0/24`, so IPv4-mapped entries match IPv4 clients.
fn canonical_net(net: IpNet) -> IpNet {
    match net {
        IpNet::V6(v6) => match v6.addr().to_ipv4_mapped() {
            Some(v4) if v6.prefix_len() >= 96 => {
                IpNet::new(IpAddr::V4(v4), v6.prefix_len() - 96).map(|n| n.trunc()).unwrap_or(net)
            }
            _ => net,
        },
        IpNet::V4(_) => net.trunc(),
    }
}
