//! Small helpers: time, random ids, hashing, HMAC, form encoding and a rate limiter.

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// (year, month, day) of a day count since 1970-01-01 (Howard Hinnant's algorithm).
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// ISO 8601 UTC, seconds.
pub fn iso(t: i64) -> String {
    let (y, m, d) = civil_from_days(t.div_euclid(86_400));
    let s = t.rem_euclid(86_400);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", s / 3600, s / 60 % 60, s % 60)
}

/// Billing period "YYYY-MM".
pub fn period(t: i64) -> String {
    let (y, m, _) = civil_from_days(t.div_euclid(86_400));
    format!("{y:04}-{m:02}")
}

/// `n` random bytes as lower-case hex.
pub fn random_hex(n: usize) -> Result<String, String> {
    let mut b = vec![0u8; n];
    getrandom::fill(&mut b).map_err(|e| format!("no randomness: {e}"))?;
    Ok(hex(&b))
}

pub fn random_bytes<const N: usize>() -> Result<[u8; N], String> {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).map_err(|e| format!("no randomness: {e}"))?;
    Ok(b)
}

pub fn hex(b: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        s.push(H[(x >> 4) as usize] as char);
        s.push(H[(x & 15) as usize] as char);
    }
    s
}

pub fn sha256_hex(s: &str) -> String {
    hex(&Sha256::digest(s.as_bytes()))
}

/// HMAC-SHA256 (RFC 2104).
pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(k.map(|b| b ^ 0x36));
    inner.update(msg);
    let ih = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(k.map(|b| b ^ 0x5c));
    outer.update(ih);
    outer.finalize().into()
}

/// Compares without an early exit.
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// `application/x-www-form-urlencoded`.
pub fn form_encode(pairs: &[(&str, &str)]) -> String {
    let enc = |s: &str| {
        let mut o = String::new();
        for b in s.bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => o.push(b as char),
                b' ' => o.push('+'),
                b => o.push_str(&format!("%{b:02X}")),
            }
        }
        o
    };
    pairs.iter().map(|(k, v)| format!("{}={}", enc(k), enc(v))).collect::<Vec<_>>().join("&")
}

/// (window start, calls in the window), per address and action.
type Hits = HashMap<(IpAddr, &'static str), (i64, u32)>;

/// Fixed-window rate limiter, per address and action.
#[derive(Default)]
pub struct RateLimiter {
    hits: Mutex<Hits>,
}

impl RateLimiter {
    /// True when the call is allowed: at most `max` per `window` seconds.
    pub fn allow(&self, ip: IpAddr, action: &'static str, max: u32, window: i64) -> bool {
        let t = now();
        let Ok(mut map) = self.hits.lock() else { return false };
        if map.len() > 100_000 {
            map.retain(|_, (start, _)| t - *start < 3600);
        }
        let e = map.entry((ip, action)).or_insert((t, 0));
        if t - e.0 >= window {
            *e = (t, 0);
        }
        e.1 += 1;
        e.1 <= max
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(iso(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(iso(1_793_016_000), "2026-10-26T12:00:00Z");
        assert_eq!(period(1_793_016_000), "2026-10");
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
    }

    #[test]
    fn hmac_rfc4231_case_2() {
        let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
        assert_eq!(hex(&mac), "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843");
        // Case 6: a key longer than the block.
        let mac = hmac_sha256(&[0xaa; 131], b"Test Using Larger Than Block-Size Key - Hash Key First");
        assert_eq!(hex(&mac), "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54");
    }

    #[test]
    fn helpers() {
        assert_eq!(form_encode(&[("a b", "x&y=z/é")]), "a+b=x%26y%3Dz%2F%C3%A9");
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"ab"));
        assert_eq!(random_hex(16).unwrap().len(), 32);
        let rl = RateLimiter::default();
        let ip: IpAddr = [127, 0, 0, 1].into();
        assert!(rl.allow(ip, "x", 2, 60));
        assert!(rl.allow(ip, "x", 2, 60));
        assert!(!rl.allow(ip, "x", 2, 60));
        assert!(rl.allow(ip, "y", 2, 60));
    }
}
