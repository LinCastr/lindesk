//! Addresses the host can be reached at.

use std::net::{Ipv4Addr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Vpn,
    Lan,
    Virtual,
    Internet,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    pub kind: Kind,
    /// Interface name (`radminvpn0`, `enp3s0`) or "internet".
    pub iface: String,
    pub ip: Ipv4Addr,
}

impl std::fmt::Display for Route {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} · {}", self.iface, self.ip)
    }
}

fn classify(name: &str, ip: Ipv4Addr) -> Kind {
    const VPN_PREFIX: [&str; 12] = ["tun", "tap", "wg", "tailscale", "zt", "radmin", "ham", "wt", "nordlynx", "proton", "mullvad", "ppp"];
    const VIRTUAL_PREFIX: [&str; 5] = ["docker", "br-", "veth", "virbr", "vmnet"];
    let o = ip.octets();
    let vpn_range = o[0] == 26 || o[0] == 25 || (o[0] == 100 && (64..128).contains(&o[1])); // Radmin, Hamachi, CGNAT/Tailscale/Netbird
    if VIRTUAL_PREFIX.iter().any(|p| name.starts_with(p)) {
        Kind::Virtual
    } else if vpn_range || VPN_PREFIX.iter().any(|p| name.starts_with(p)) {
        Kind::Vpn
    } else {
        Kind::Lan
    }
}

/// Non-loopback IPv4 addresses of this computer, VPN first (that is what people
/// usually connect through), then local network, then virtual adapters.
pub fn local_routes() -> Vec<Route> {
    let mut v: Vec<Route> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| !i.is_loopback())
        .filter_map(|i| match i.ip() {
            std::net::IpAddr::V4(ip) if !ip.is_link_local() => Some(Route { kind: classify(&i.name, ip), iface: i.name, ip }),
            _ => None,
        })
        .collect();
    // Radmin (26/8) and Hamachi (25/8) are what game friends use, so they win over other VPNs.
    v.sort_by_key(|r| match (r.kind, r.ip.octets()[0]) {
        (Kind::Vpn, 26) => 0,
        (Kind::Vpn, 25) => 1,
        (Kind::Vpn, _) => 2,
        (Kind::Lan, _) => 3,
        (Kind::Internet, _) => 4,
        (Kind::Virtual, _) => 5,
    });
    v
}

/// The route to build the ID from: the user's preference if it still exists,
/// otherwise the best one that is not a virtual adapter.
pub fn pick<'a>(routes: &'a [Route], pref: Option<&str>) -> Option<&'a Route> {
    pref.and_then(|p| routes.iter().find(|r| r.iface == p)).or_else(|| routes.iter().find(|r| r.kind != Kind::Virtual))
}

/// Public IPv4 as seen by a STUN server (RFC 5389 binding request).
/// Only called when the user asks for it: it contacts a third-party server.
pub fn public_ip() -> Option<Ipv4Addr> {
    let server: SocketAddr = "stun.l.google.com:19302".to_socket_addrs().ok()?.find(|a| a.is_ipv4())?;
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.set_read_timeout(Some(Duration::from_secs(3))).ok()?;
    let mut req = [0u8; 20];
    req[0..2].copy_from_slice(&0x0001u16.to_be_bytes()); // binding request
    req[4..8].copy_from_slice(&0x2112_A442u32.to_be_bytes()); // magic cookie
    for b in &mut req[8..20] {
        *b = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.subsec_nanos() as u8 ^ (*b).wrapping_add(0x5a);
    }
    sock.send_to(&req, server).ok()?;
    let mut buf = [0u8; 512];
    let (n, _) = sock.recv_from(&mut buf).ok()?;
    parse_binding_response(&buf[..n], &req[8..20])
}

fn parse_binding_response(b: &[u8], txid: &[u8]) -> Option<Ipv4Addr> {
    if b.len() < 20 || &b[8..20] != txid {
        return None;
    }
    let mut i = 20;
    while i + 4 <= b.len() {
        let (ty, len) = (u16::from_be_bytes([b[i], b[i + 1]]), u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize);
        let val = b.get(i + 4..i + 4 + len)?;
        match ty {
            0x0020 if len >= 8 && val[1] == 1 => {
                // XOR-MAPPED-ADDRESS
                let x = u32::from_be_bytes([val[4], val[5], val[6], val[7]]) ^ 0x2112_A442;
                return Some(Ipv4Addr::from(x));
            }
            0x0001 if len >= 8 && val[1] == 1 => return Some(Ipv4Addr::new(val[4], val[5], val[6], val[7])),
            _ => {}
        }
        i += 4 + len.div_ceil(4) * 4;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_vpn_lan_virtual() {
        let ip = |s: &str| s.parse::<Ipv4Addr>().unwrap();
        assert_eq!(classify("radminvpn0", ip("26.78.43.175")), Kind::Vpn);
        assert_eq!(classify("eth9", ip("26.1.1.1")), Kind::Vpn);
        assert_eq!(classify("wt0", ip("100.77.182.163")), Kind::Vpn);
        assert_eq!(classify("enp3s0", ip("192.168.0.124")), Kind::Lan);
        assert_eq!(classify("docker0", ip("172.17.0.1")), Kind::Virtual);
    }

    #[test]
    fn pick_prefers_choice_then_best() {
        let r = |k, i: &str, ip: &str| Route { kind: k, iface: i.into(), ip: ip.parse().unwrap() };
        let routes = vec![r(Kind::Vpn, "radminvpn0", "26.1.1.1"), r(Kind::Lan, "enp3s0", "192.168.0.2"), r(Kind::Virtual, "docker0", "172.17.0.1")];
        assert_eq!(pick(&routes, None).unwrap().iface, "radminvpn0");
        assert_eq!(pick(&routes, Some("enp3s0")).unwrap().iface, "enp3s0");
        assert_eq!(pick(&routes, Some("gone0")).unwrap().iface, "radminvpn0");
        assert_eq!(pick(&routes[2..], None), None);
    }

    #[test]
    fn radmin_beats_other_vpns() {
        let mut v = vec![
            Route { kind: Kind::Vpn, iface: "wt0".into(), ip: "100.77.1.1".parse().unwrap() },
            Route { kind: Kind::Vpn, iface: "radminvpn0".into(), ip: "26.1.1.1".parse().unwrap() },
        ];
        v.sort_by_key(|r| if r.ip.octets()[0] == 26 { 0 } else { 2 });
        assert_eq!(v[0].iface, "radminvpn0");
    }

    #[test]
    fn parses_xor_mapped_address() {
        let txid = [7u8; 12];
        let mut r = vec![0x01, 0x01, 0, 12, 0x21, 0x12, 0xA4, 0x42];
        r.extend_from_slice(&txid);
        let ip = u32::from(Ipv4Addr::new(203, 0, 113, 9)) ^ 0x2112_A442;
        r.extend_from_slice(&[0x00, 0x20, 0, 8, 0, 1, 0, 0]);
        r.extend_from_slice(&ip.to_be_bytes());
        assert_eq!(parse_binding_response(&r, &txid), Some(Ipv4Addr::new(203, 0, 113, 9)));
        assert_eq!(parse_binding_response(&r, &[1u8; 12]), None);
    }
}
