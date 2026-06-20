//! Best-effort flow inspection for the in-app Log view.
//!
//! Looks at the *plaintext* IPv4 packets the client sends into the tunnel and
//! pulls out the human-meaningful destination each flow is for:
//!
//! * **DNS** — the query name in a UDP/53 request (`DNS A example.com`).
//! * **TLS** — the SNI host in a TCP/443 ClientHello (`TLS example.com`).
//! * **HTTP** — the method + `Host` of a TCP/80 request (`HTTP GET example.com`).
//!
//! It is purely passive and best-effort: every parser is bounds-checked (the
//! crate is `panic = "abort"`, so a malformed packet must never index out of
//! range) and returns `None` on anything it doesn't understand. Nothing here
//! affects forwarding — it only produces a log string.

use std::net::Ipv4Addr;

use crate::vendor::dns::question;

const IPPROTO_TCP: u8 = 6;
const IPPROTO_UDP: u8 = 17;

/// Inspect one plaintext IPv4 packet and return a short, human-readable
/// description of the flow it carries, or `None` if there's nothing notable.
pub fn describe(pkt: &[u8]) -> Option<String> {
    // IPv4 only, well-formed header.
    if pkt.len() < 20 || pkt[0] >> 4 != 4 {
        return None;
    }
    let ihl = (pkt[0] & 0x0f) as usize * 4;
    if ihl < 20 || pkt.len() < ihl {
        return None;
    }
    let proto = pkt[9];
    let dst = Ipv4Addr::new(pkt[16], pkt[17], pkt[18], pkt[19]);
    let l4 = pkt.get(ihl..)?;

    match proto {
        IPPROTO_UDP => {
            // UDP header: src(2) dst(2) len(2) csum(2) then payload.
            if l4.len() < 8 {
                return None;
            }
            let dport = u16::from_be_bytes([l4[2], l4[3]]);
            if dport != 53 {
                return None;
            }
            let dns = l4.get(8..)?;
            let (name, qtype, _qclass) = question(dns)?;
            Some(format!("DNS {} {}", qtype_name(qtype), name))
        }
        IPPROTO_TCP => {
            // TCP header: ports(4) seq(4) ack(4) then data-offset in the high
            // nibble of byte 12 (in 32-bit words).
            if l4.len() < 20 {
                return None;
            }
            let dport = u16::from_be_bytes([l4[2], l4[3]]);
            let data_off = (l4[12] >> 4) as usize * 4;
            let payload = l4.get(data_off..)?;
            if payload.is_empty() {
                return None;
            }
            match dport {
                443 => tls_sni(payload).map(|h| format!("TLS {h}")),
                80 => http_host(payload).map(|(m, h)| format!("HTTP {m} {h}")),
                _ => {
                    let _ = dst;
                    None
                }
            }
        }
        _ => None,
    }
}

/// Human label for the common DNS query types; numeric for the rest.
fn qtype_name(qtype: u16) -> &'static str {
    match qtype {
        1 => "A",
        28 => "AAAA",
        5 => "CNAME",
        15 => "MX",
        16 => "TXT",
        33 => "SRV",
        65 => "HTTPS",
        _ => "?",
    }
}

/// Extract the SNI host from the start of a TLS stream (a ClientHello). Returns
/// `None` unless the bytes are a TLS handshake record carrying a ClientHello
/// with a non-empty server_name extension.
fn tls_sni(b: &[u8]) -> Option<String> {
    // TLS record: type(1)=0x16 handshake, version(2), length(2).
    if *b.first()? != 0x16 {
        return None;
    }
    let rec = b.get(5..)?; // skip the 5-byte record header

    // Handshake: msg_type(1)=0x01 ClientHello, length(3).
    if *rec.first()? != 0x01 {
        return None;
    }
    let mut p = 4usize; // skip handshake type + 3-byte length
    p = p.checked_add(2)?; // client_version
    p = p.checked_add(32)?; // random

    // session_id: 1-byte length + bytes.
    let sid_len = *rec.get(p)? as usize;
    p = p.checked_add(1)?.checked_add(sid_len)?;

    // cipher_suites: 2-byte length + bytes.
    let cs_len = u16::from_be_bytes([*rec.get(p)?, *rec.get(p + 1)?]) as usize;
    p = p.checked_add(2)?.checked_add(cs_len)?;

    // compression_methods: 1-byte length + bytes.
    let cm_len = *rec.get(p)? as usize;
    p = p.checked_add(1)?.checked_add(cm_len)?;

    // extensions: 2-byte total length, then (type(2) len(2) data) entries.
    let ext_total = u16::from_be_bytes([*rec.get(p)?, *rec.get(p + 1)?]) as usize;
    p = p.checked_add(2)?;
    let ext_end = p.checked_add(ext_total)?.min(rec.len());

    while p + 4 <= ext_end {
        let etype = u16::from_be_bytes([rec[p], rec[p + 1]]);
        let elen = u16::from_be_bytes([rec[p + 2], rec[p + 3]]) as usize;
        let body = rec.get(p + 4..p + 4 + elen)?;
        if etype == 0 {
            // server_name extension: list_len(2), name_type(1), name_len(2), name.
            if body.len() < 5 || body[2] != 0 {
                return None;
            }
            let name_len = u16::from_be_bytes([body[3], body[4]]) as usize;
            let name = body.get(5..5 + name_len)?;
            return std::str::from_utf8(name).ok().map(str::to_string);
        }
        p = p.checked_add(4)?.checked_add(elen)?;
    }
    None
}

/// Extract `(method, host)` from the start of an HTTP/1.x request. Reads only
/// the head (capped) and is tolerant of non-UTF-8 bytes in a body.
fn http_host(b: &[u8]) -> Option<(String, String)> {
    const METHODS: [&str; 8] = [
        "GET ", "POST ", "HEAD ", "PUT ", "DELETE ", "OPTIONS ", "PATCH ", "CONNECT ",
    ];
    let head = String::from_utf8_lossy(&b[..b.len().min(1024)]);
    let method = METHODS
        .iter()
        .find(|m| head.starts_with(**m))
        .map(|m| m.trim_end())?;
    for line in head.split("\r\n") {
        if line.len() >= 5 && line[..5].eq_ignore_ascii_case("host:") {
            let host = line[5..].trim();
            if !host.is_empty() {
                return Some((method.to_string(), host.to_string()));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal IPv4+UDP/53 packet carrying `dns`.
    fn udp53(dns: &[u8]) -> Vec<u8> {
        let mut p = vec![0u8; 20 + 8 + dns.len()];
        p[0] = 0x45;
        p[9] = IPPROTO_UDP;
        p[28 - 8 + 2] = 0; // src port hi (offset 20+0)
                           // dst port = 53 at bytes 20+2..20+4
        p[22] = 0;
        p[23] = 53;
        p[28..].copy_from_slice(dns);
        p
    }

    fn dns_query(name: &str) -> Vec<u8> {
        let mut m = vec![0u8, 0, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0];
        for label in name.split('.') {
            m.push(label.len() as u8);
            m.extend_from_slice(label.as_bytes());
        }
        m.push(0);
        m.extend_from_slice(&1u16.to_be_bytes()); // A
        m.extend_from_slice(&1u16.to_be_bytes()); // IN
        m
    }

    #[test]
    fn extracts_dns_query() {
        let pkt = udp53(&dns_query("example.com"));
        assert_eq!(describe(&pkt).as_deref(), Some("DNS A example.com"));
    }

    #[test]
    fn http_host_parsed() {
        let req = b"GET /index.html HTTP/1.1\r\nHost: www.example.com\r\n\r\n";
        assert_eq!(
            http_host(req),
            Some(("GET".to_string(), "www.example.com".to_string()))
        );
    }

    #[test]
    fn tls_sni_parsed() {
        // Minimal ClientHello with an SNI extension for "a.com".
        let host = b"a.com";
        let mut hs = vec![0x01, 0, 0, 0]; // ClientHello + 3-byte len (patched later)
        hs.extend_from_slice(&[0x03, 0x03]); // version
        hs.extend_from_slice(&[0u8; 32]); // random
        hs.push(0); // session id len
        hs.extend_from_slice(&2u16.to_be_bytes()); // cipher suites len
        hs.extend_from_slice(&[0x13, 0x01]); // one cipher suite
        hs.push(1); // compression len
        hs.push(0); // null compression
                    // extensions
        let mut sni_body = Vec::new();
        let entry_len = 1 + 2 + host.len();
        sni_body.extend_from_slice(&(entry_len as u16).to_be_bytes()); // list len
        sni_body.push(0); // name type host
        sni_body.extend_from_slice(&(host.len() as u16).to_be_bytes());
        sni_body.extend_from_slice(host);
        let mut exts = Vec::new();
        exts.extend_from_slice(&0u16.to_be_bytes()); // ext type server_name
        exts.extend_from_slice(&(sni_body.len() as u16).to_be_bytes());
        exts.extend_from_slice(&sni_body);
        hs.extend_from_slice(&(exts.len() as u16).to_be_bytes());
        hs.extend_from_slice(&exts);
        // patch handshake length
        let body_len = (hs.len() - 4) as u32;
        hs[1..4].copy_from_slice(&body_len.to_be_bytes()[1..]);
        // wrap in a TLS record
        let mut rec = vec![0x16, 0x03, 0x01];
        rec.extend_from_slice(&(hs.len() as u16).to_be_bytes());
        rec.extend_from_slice(&hs);
        assert_eq!(tls_sni(&rec).as_deref(), Some("a.com"));
    }

    #[test]
    fn rejects_garbage() {
        assert!(describe(&[0u8; 4]).is_none());
        assert!(tls_sni(&[0x16, 0x03, 0x01, 0x00]).is_none());
        assert!(http_host(b"\x00\x01\x02 not http").is_none());
    }
}
