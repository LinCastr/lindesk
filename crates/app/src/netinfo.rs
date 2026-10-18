//! Addresses the host can be reached at.

use std::net::{Ipv4Addr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    pub label: String,
    pub ip: Ipv4Addr,
}

impl std::fmt::Display for Route {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} · {}", self.label, self.ip)
    }
}

/// Non-loopback IPv4 addresses of this computer (LAN, VPN, ...), best guess first.
pub fn local_routes() -> Vec<Route> {
    let mut v: Vec<Route> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| !i.is_loopback())
        .filter_map(|i| match i.ip() {
            std::net::IpAddr::V4(ip) if !ip.is_link_local() => {
                let name = i.name;
                let label = if name.starts_with("tun") || name.starts_with("tap") || name.starts_with("wg") || name.starts_with("rad") || name.starts_with("tailscale") {
                    format!("VPN ({name})")
                } else if name.starts_with("docker") || name.starts_with("br-") || name.starts_with("veth") || name.starts_with("virbr") {
                    format!("Virtual ({name})")
                } else {
                    format!("Local network ({name})")
                };
                Some(Route { label, ip })
            }
            _ => None,
        })
        .collect();
    v.sort_by_key(|r| r.label.starts_with("Virtual"));
    v
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
