//! Connection code: the host address and the secret key in one pasteable string.
//!
//! `LD-XXXXXX-XXXXXX-...` is Crockford base32 of
//! `version(1) | family(1) | ip(4|16) | port(2) | key(32) | checksum(2)`.
//! The code is a secret (it contains the key): share it only with whoever
//! may control this computer.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use anyhow::{Result, bail, ensure};

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const CODE_VERSION: u8 = 1;

fn checksum(data: &[u8]) -> u16 {
    // Fletcher-16: catches the typos and dropped characters people make when retyping.
    let (mut a, mut b) = (0u16, 0u16);
    for &x in data {
        a = (a + x as u16) % 255;
        b = (b + a) % 255;
    }
    (b << 8) | a
}

fn b32_encode(data: &[u8]) -> String {
    let mut out = String::new();
    let (mut acc, mut bits) = (0u32, 0u32);
    for &b in data {
        acc = (acc << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((acc >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((acc << (5 - bits)) & 31) as usize] as char);
    }
    out
}

fn b32_decode(s: &str) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.chars() {
        let c = match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            c => c,
        };
        let Some(v) = ALPHABET.iter().position(|&a| a as char == c) else {
            bail!("invalid character '{c}' in code");
        };
        acc = (acc << 5) | v as u32;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Ok(out)
}

pub fn encode(addr: SocketAddr, key: &[u8; 32]) -> String {
    let mut b = vec![CODE_VERSION];
    match addr.ip() {
        IpAddr::V4(ip) => {
            b.push(4);
            b.extend_from_slice(&ip.octets());
        }
        IpAddr::V6(ip) => {
            b.push(6);
            b.extend_from_slice(&ip.octets());
        }
    }
    b.extend_from_slice(&addr.port().to_be_bytes());
    b.extend_from_slice(key);
    let sum = checksum(&b);
    b.extend_from_slice(&sum.to_be_bytes());
    let raw = b32_encode(&b);
    let groups: Vec<&str> = raw.as_bytes().chunks(6).map(|c| std::str::from_utf8(c).unwrap()).collect();
    format!("LD-{}", groups.join("-"))
}

pub fn decode(code: &str) -> Result<(SocketAddr, [u8; 32])> {
    let mut s: String = code.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    ensure!(s.len() > 2 && s[..2].eq_ignore_ascii_case("LD"), "not a lindesk code (should start with LD-)");
    s.drain(..2);
    let b = b32_decode(&s)?;
    ensure!(b.len() >= 6, "code too short");
    let (body, sum) = b.split_at(b.len() - 2);
    ensure!(checksum(body) == u16::from_be_bytes([sum[0], sum[1]]), "code has a typo or is incomplete");
    ensure!(body[0] == CODE_VERSION, "code from a newer lindesk version; update the app");
    let (ip, rest): (IpAddr, &[u8]) = match body[1] {
        4 if body.len() >= 2 + 4 => (IpAddr::V4(Ipv4Addr::from(<[u8; 4]>::try_from(&body[2..6])?)), &body[6..]),
        6 if body.len() >= 2 + 16 => (IpAddr::V6(Ipv6Addr::from(<[u8; 16]>::try_from(&body[2..18])?)), &body[18..]),
        _ => bail!("bad address in code"),
    };
    ensure!(rest.len() == 2 + 32, "bad code length");
    let port = u16::from_be_bytes([rest[0], rest[1]]);
    let key: [u8; 32] = rest[2..].try_into()?;
    Ok((SocketAddr::new(ip, port), key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_v4_v6() {
        let key = [0xA7u8; 32];
        for a in ["192.168.0.10:7000", "[2001:db8::1]:7001"] {
            let addr: SocketAddr = a.parse().unwrap();
            let code = encode(addr, &key);
            assert!(code.starts_with("LD-"));
            assert_eq!(decode(&code).unwrap(), (addr, key));
        }
    }

    #[test]
    fn tolerant_input() {
        let key = [3u8; 32];
        let addr: SocketAddr = "10.0.0.2:7000".parse().unwrap();
        let code = encode(addr, &key);
        let messy = format!("  {}\n", code.to_lowercase().replace('-', " "));
        assert_eq!(decode(&messy).unwrap().0, addr);
    }

    #[test]
    fn detects_typos() {
        let code = encode("10.0.0.2:7000".parse().unwrap(), &[3u8; 32]);
        let mut chars: Vec<char> = code.chars().collect();
        let i = chars.len() / 2;
        chars[i] = if chars[i] == '2' { '3' } else { '2' };
        assert!(decode(&chars.into_iter().collect::<String>()).is_err());
        assert!(decode(&code[..code.len() - 4]).is_err());
        assert!(decode("hello").is_err());
    }
}
