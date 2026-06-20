//! Runtime country → CIDR extraction with an on-disk per-country cache.
//!
//! ShadowVPN's split-tunnel lets the user pick **which country's IP ranges
//! bypass the tunnel** (not just China). The country set is derived at runtime
//! from a bundled MaxMind GeoLite2 *Country* database, so a new country never
//! needs a rebuild — but walking the whole IPv4 space of the mmdb and folding it
//! into CIDRs is comparatively expensive, so the result is **cached to a file**
//! the first time a country is requested and reused on every later tunnel start
//! and app launch.
//!
//! Cache file: `<cache_dir>/chnroute-<COUNTRY>-<mmdb_len>.txt`. Embedding the
//! mmdb byte length in the name is a cheap, naive invalidation: shipping a
//! refreshed `Country.mmdb` (a different size) produces a different cache file,
//! so a stale set is never served after an app update. The file format is the
//! same plain `a.b.c.d/len`-per-line text the rest of the stack already parses
//! (Swift `SVTunnelSettings` for excludedRoutes, the vendored `chnroute` parser
//! for the chinadns decision), so the cache file *is* the `chnroute_path` the
//! NE hands back into `config_json`.
//!
//! The geoip walk mirrors `shadowvpn::policy::geoip::load_country_routes`
//! upstream; the range→CIDR split is the standard most-significant-aligned-block
//! decomposition. Only IPv4 is collected (the tunnel is IPv4-only).

use std::net::Ipv4Addr;
use std::path::Path;

use ipnetwork::{IpNetwork, Ipv4Network};
use maxminddb::{geoip2, Reader, WithinOptions};

/// Ensure the CIDR file for `country` exists in `cache_dir` (extracting it from
/// the mmdb at `mmdb_path` once if missing), and return its absolute path.
///
/// Reusable across the FFI boundary via [`crate::svpn_country_cidrs_file`].
pub fn ensure_country_file(
    mmdb_path: &str,
    country: &str,
    cache_dir: &str,
) -> Result<String, String> {
    let code = normalize_country(country);
    if code.is_empty() {
        return Err("country code is empty / not alphanumeric".to_string());
    }

    let mmdb_len = std::fs::metadata(mmdb_path)
        .map_err(|e| format!("stat mmdb {mmdb_path}: {e}"))?
        .len();

    let dir = Path::new(cache_dir);
    std::fs::create_dir_all(dir).map_err(|e| format!("create cache dir {cache_dir}: {e}"))?;
    let cache_file = dir.join(format!("chnroute-{code}-{mmdb_len}.txt"));

    // Fast path: a non-empty cache file for this (country, mmdb) already exists.
    if let Ok(meta) = std::fs::metadata(&cache_file) {
        if meta.len() > 0 {
            return Ok(path_string(&cache_file));
        }
    }

    // Slow path: walk the mmdb once, build the CIDR text, write it atomically.
    let text = build_country_cidrs(mmdb_path, &code)?;
    write_atomic(&cache_file, &text)?;
    Ok(path_string(&cache_file))
}

/// Uppercase, keep only ASCII alphanumerics. Guards the cache filename and the
/// ISO match against odd input (`"cn "`, `"Cn"`, `"../x"` …).
fn normalize_country(country: &str) -> String {
    country
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// Walk every IPv4 network in the mmdb and emit the merged CIDR text for the
/// networks whose country ISO code matches `code` (already normalized upper).
fn build_country_cidrs(mmdb_path: &str, code: &str) -> Result<String, String> {
    // Read the whole database into memory (a few hundred KB) rather than mmap —
    // `from_source` keeps us off the `mmap` cargo feature, which matters for a
    // clean iOS cross-compile.
    let bytes = std::fs::read(mmdb_path).map_err(|e| format!("read mmdb {mmdb_path}: {e}"))?;
    let reader = Reader::from_source(bytes).map_err(|e| format!("open mmdb: {e}"))?;

    let all_v4 = IpNetwork::V4(
        Ipv4Network::new(Ipv4Addr::UNSPECIFIED, 0).expect("0.0.0.0/0 is a valid network"),
    );

    let mut ranges: Vec<(u32, u32)> = Vec::new();
    for item in reader
        .within(all_v4, WithinOptions::default())
        .map_err(|e| format!("iterate mmdb networks: {e}"))?
    {
        let item = item.map_err(|e| format!("decode mmdb network: {e}"))?;
        let net = match item
            .network()
            .map_err(|e| format!("read mmdb network: {e}"))?
        {
            IpNetwork::V4(v4) => v4,
            IpNetwork::V6(_) => continue,
        };
        let record: Option<geoip2::Country> = item
            .decode()
            .map_err(|e| format!("decode mmdb country: {e}"))?;
        let matches = record
            .and_then(|r| r.country.iso_code)
            .is_some_and(|iso| iso.eq_ignore_ascii_case(code));
        if matches {
            ranges.push((u32::from(net.network()), u32::from(net.broadcast())));
        }
    }

    if ranges.is_empty() {
        return Err(format!("no IPv4 networks found for country {code}"));
    }

    // Sort + merge adjacent/overlapping ranges, then re-split into minimal CIDRs.
    ranges.sort_unstable();
    let mut merged: Vec<(u32, u32)> = Vec::with_capacity(ranges.len());
    for (s, e) in ranges {
        if let Some(last) = merged.last_mut() {
            if s <= last.1.saturating_add(1) {
                if e > last.1 {
                    last.1 = e;
                }
                continue;
            }
        }
        merged.push((s, e));
    }

    let mut cidrs: Vec<String> = Vec::new();
    for (start, end) in &merged {
        range_to_cidrs(*start, *end, &mut cidrs);
    }

    let header = format!(
        "# chnroute ({code}) — generated at runtime from a MaxMind GeoLite2 Country mmdb\n\
         # {} CIDR ranges. Cached; delete this file to force regeneration.\n",
        cidrs.len()
    );
    Ok(format!("{header}{}\n", cidrs.join("\n")))
}

/// Decompose an inclusive `[start, end]` u32 range into the minimal set of CIDR
/// blocks (standard range-to-prefix splitting): at each step take the largest
/// block that is both aligned to `start` and fits within the remaining span.
fn range_to_cidrs(mut start: u32, end: u32, out: &mut Vec<String>) {
    loop {
        // Largest aligned block at `start` (trailing_zeros(0) == 32 → /0 ok).
        let align_bits = start.trailing_zeros();
        // Largest power-of-two block that fits the remaining span.
        let span = (end - start) as u64 + 1;
        let span_bits = 63 - span.leading_zeros(); // floor(log2(span)), span >= 1
        let bits = align_bits.min(span_bits);
        let prefix = 32 - bits;
        out.push(format!("{}/{}", Ipv4Addr::from(start), prefix));
        let next = start as u64 + (1u64 << bits);
        if next > end as u64 {
            break;
        }
        start = next as u32;
    }
}

/// Write `text` to `path` atomically: a sibling temp file + rename, so a
/// concurrent reader never observes a half-written cache file.
fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    let tmp = path.with_extension("txt.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("rename into {}: {e}", path.display()))?;
    Ok(())
}

/// Lossy path → String (cache paths are app-controlled UTF-8 in practice).
fn path_string(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cidrs(start: &str, end: &str) -> Vec<String> {
        let mut out = Vec::new();
        range_to_cidrs(
            u32::from(start.parse::<Ipv4Addr>().unwrap()),
            u32::from(end.parse::<Ipv4Addr>().unwrap()),
            &mut out,
        );
        out
    }

    #[test]
    fn single_aligned_block() {
        assert_eq!(cidrs("1.0.1.0", "1.0.1.255"), vec!["1.0.1.0/24"]);
    }

    #[test]
    fn adjacent_blocks_merge_into_minimal_cidrs() {
        // 1.0.1.0 – 1.0.3.255 == 1.0.1.0/24 + 1.0.2.0/23
        assert_eq!(
            cidrs("1.0.1.0", "1.0.3.255"),
            vec!["1.0.1.0/24", "1.0.2.0/23"]
        );
    }

    #[test]
    fn full_space_is_a_single_zero_route() {
        assert_eq!(cidrs("0.0.0.0", "255.255.255.255"), vec!["0.0.0.0/0"]);
    }

    #[test]
    fn single_host_is_a_slash_32() {
        assert_eq!(cidrs("8.8.8.8", "8.8.8.8"), vec!["8.8.8.8/32"]);
    }

    #[test]
    fn round_trip_covers_exactly_the_range() {
        // Every address in [start,end] is covered and none outside it.
        let (s, e): (u32, u32) = (
            u32::from("10.20.30.5".parse::<Ipv4Addr>().unwrap()),
            u32::from("10.20.31.200".parse::<Ipv4Addr>().unwrap()),
        );
        let mut out = Vec::new();
        range_to_cidrs(s, e, &mut out);
        // Reconstruct the covered set from the emitted CIDRs.
        let mut covered: Vec<(u32, u32)> = out
            .iter()
            .map(|c| {
                let (a, l) = c.split_once('/').unwrap();
                let base = u32::from(a.parse::<Ipv4Addr>().unwrap());
                let len: u32 = l.parse().unwrap();
                let mask = if len == 0 { 0 } else { u32::MAX << (32 - len) };
                (base & mask, (base & mask) | !mask)
            })
            .collect();
        covered.sort_unstable();
        assert_eq!(covered.first().unwrap().0, s, "starts at range start");
        assert_eq!(covered.last().unwrap().1, e, "ends at range end");
        // Contiguous, non-overlapping tiling.
        for w in covered.windows(2) {
            assert_eq!(w[0].1 + 1, w[1].0, "blocks tile without gaps/overlap");
        }
    }

    #[test]
    fn normalize_country_is_strict() {
        assert_eq!(normalize_country(" cn "), "CN");
        assert_eq!(normalize_country("Hk"), "HK");
        assert_eq!(normalize_country("../x"), "X");
        assert_eq!(normalize_country("!!"), "");
    }
}
