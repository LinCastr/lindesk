//! The 10-digit ID: the host's IPv4 address as one decimal number.
//!
//! `192.168.0.124` becomes `3232235644`. There is no server behind it: the ID
//! only says where to knock, and who gets in is decided by the host's owner.
//! Shown in groups, `3 232 235 644`, easy to read aloud.

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

use anyhow::{Result, bail, ensure};

pub const DEFAULT_PORT: u16 = 7000;

/// Ten digits, zero-padded: `0436207616`.
pub fn encode(ip: Ipv4Addr) -> String {
    format!("{:010}", u32::from(ip))
}

/// `3 232 235 644` (the padded ID in groups of 1-3-3-3).
pub fn pretty(id: &str) -> String {
    let d: Vec<char> = id.chars().filter(char::is_ascii_digit).collect();
    if d.len() != 10 {
        return id.to_string();
    }
    let s: String = d.iter().collect();
    format!("{} {} {} {}", &s[0..1], &s[1..4], &s[4..7], &s[7..10])
}

/// Accepts what people actually type: spaces, dashes or dots between digits and
/// an optional `:port` (only needed when the host changed its port).
pub fn parse(text: &str) -> Result<SocketAddr> {
    let (digits, port) = match text.trim().split_once(':') {
        Some((d, p)) => (d, p.trim().parse::<u16>().map_err(|_| anyhow::anyhow!("invalid port '{}'", p.trim()))?),
        None => (text.trim(), DEFAULT_PORT),
    };
    let digits: String = digits.chars().filter(|c| !matches!(c, ' ' | '-' | '.' | '\u{a0}')).collect();
    ensure!(!digits.is_empty(), "type the 10-digit ID");
    if !digits.chars().all(|c| c.is_ascii_digit()) {
        bail!("the ID has only digits");
    }
    ensure!(digits.len() == 10, "the ID has 10 digits, you typed {}", digits.len());
    let n: u32 = digits.parse().map_err(|_| anyhow::anyhow!("that is not a valid ID"))?;
    ensure!(n != 0, "that is not a valid ID");
    Ok(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::from(n), port)))
}

/// The ID of a socket address; port is dropped (the default is assumed).
pub fn of(addr: SocketAddr) -> Option<String> {
    match addr {
        SocketAddr::V4(a) => Some(encode(*a.ip())),
        SocketAddr::V6(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        for ip in ["192.168.0.124", "26.78.43.175", "255.255.255.255", "1.0.0.0"] {
            let ip: Ipv4Addr = ip.parse().unwrap();
            let id = encode(ip);
            assert_eq!(id.len(), 10);
            assert_eq!(parse(&id).unwrap(), SocketAddr::from((ip, DEFAULT_PORT)));
        }
    }

    #[test]
    fn known_value_and_pretty() {
        assert_eq!(encode("192.168.0.124".parse().unwrap()), "3232235644");
        assert_eq!(pretty("3232235644"), "3 232 235 644");
        assert_eq!(encode("26.78.43.175".parse().unwrap()), "0441330607");
        assert_eq!(pretty("0441330607"), "0 441 330 607");
    }

    #[test]
    fn tolerant_input_and_port() {
        let a = parse(" 3 232-235 644 ").unwrap();
        assert_eq!(a, "192.168.0.124:7000".parse().unwrap());
        assert_eq!(parse("3232235644:7001").unwrap().port(), 7001);
    }

    #[test]
    fn rejects_garbage() {
        for bad in ["", "abc", "123", "12345678901", "0000000000", "3232235644:x", "3232235644:99999"] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }
}
