//! Impossible-travel country resolution (SPEC §9, M4) — the DORMANT
//! feature pattern.
//!
//! Dormancy contract: the default build carries **no GeoIP dependency**
//! (kickoff constraint). Per-key IP history is ALWAYS recorded; the
//! country-differs check activates only when BOTH hold:
//!
//! 1. the binary was built with the `geoip` cargo feature (pulls
//!    `maxminddb` — justified in PLAN.md, workflow step 10), and
//! 2. `TRIBUTARY_GEOIP_DB_PATH` points at a readable GeoLite2-Country mmdb.
//!
//! A configured path in a feature-less build logs ONE warning at startup
//! and stays dormant — the operator learns immediately, the data plane
//! never blocks on it.
//!
//! The resolver seam is a trait (SPEC §15) so unit tests drive the travel
//! verdict with fixture countries instead of shipping a real mmdb file.
#[cfg(feature = "geoip")] // only the mmdb-backed resolver parses IPs itself
use std::net::IpAddr;

/// Resolves an IP to an ISO-3166 alpha-2 country code ("VN", "US", …).
pub trait CountryResolver: Send + Sync {
    fn country_of(&self, ip: &str) -> Option<String>;
}

/// The default, dependency-free resolver: always "unknown" — the check
/// stays dormant. Cheaper than an `Option<Arc<dyn …>>` at every request.
pub struct NoGeo;

impl CountryResolver for NoGeo {
    fn country_of(&self, _ip: &str) -> Option<String> {
        None
    }
}

/// The GeoLite2-backed resolver (feature `geoip` only).
#[cfg(feature = "geoip")]
pub struct MaxMindDb {
    reader: maxminddb::Reader<Vec<u8>>,
}

#[cfg(feature = "geoip")]
impl MaxMindDb {
    /// Reads the database file into memory at startup. Failure is fatal
    /// to the resolver construction, not to the process:
    /// `build_geo_resolver` falls back to [`NoGeo`] with a loud log.
    pub fn open(path: &std::path::Path) -> anyhow::Result<Self> {
        Ok(Self {
            reader: maxminddb::Reader::open_readfile(path)?,
        })
    }
}

#[cfg(feature = "geoip")]
impl CountryResolver for MaxMindDb {
    fn country_of(&self, ip: &str) -> Option<String> {
        let addr: IpAddr = ip.parse().ok()?;
        // Mmdb records expose `country.iso_code`; "local" and other
        // non-IP strings resolve to None (dormant for that request).
        // 0.32 API: lookup returns a lazy handle; `decode` materializes
        // the typed record (None = no data for this IP).
        let found = self.reader.lookup(addr).ok()?;
        let record: Option<maxminddb::geoip2::Country> = found.decode().ok()?;
        record
            .map(|c| c.country)
            .and_then(|c| c.iso_code.map(str::to_string))
    }
}

/// Builds the configured resolver: `Some(path)` + feature ⇒ MaxMindDb;
/// anything else ⇒ [`NoGeo`] (with one warning when a path was set but
/// the feature is off — silent misconfiguration is the enemy).
pub fn build_geo_resolver(path: Option<&str>) -> std::sync::Arc<dyn CountryResolver> {
    match path {
        Some(path) => {
            #[cfg(feature = "geoip")]
            {
                match MaxMindDb::open(std::path::Path::new(path)) {
                    Ok(db) => {
                        tracing::info!(path = %path, "GeoLite2 resolver active — impossible-travel check enabled");
                        std::sync::Arc::new(db)
                    }
                    Err(err) => {
                        tracing::error!(
                            "GeoLite2 db at {path} failed to open ({err:#}) — travel check dormant"
                        );
                        std::sync::Arc::new(NoGeo)
                    }
                }
            }
            #[cfg(not(feature = "geoip"))]
            {
                tracing::warn!(
                    "TRIBUTARY_GEOIP_DB_PATH is set ({path}) but this build lacks the geoip \
                     feature — impossible-travel check stays dormant"
                );
                std::sync::Arc::new(NoGeo)
            }
        }
        None => std::sync::Arc::new(NoGeo),
    }
}

/// The travel verdict, isolated as a pure function so tests exercise the
/// POLICY without a database file: fire when we already know ≥1 country
/// for the key and the current request resolves to a DIFFERENT one.
///
/// Deliberately country-differs (SPEC §9 "simple"), not speed-of-light
/// math: with per-key history this catches the shared-key-in-two-continents
/// pattern that matters for leak detection, without a false-positive
/// factory (VPNs, mobile carriers) that distance+time heuristics create.
pub fn travel_verdict(prior_countries: &[String], current: Option<&str>) -> bool {
    match current {
        Some(current) if !prior_countries.is_empty() => {
            !prior_countries.iter().any(|c| c == current)
        }
        // Unknown country never fires (the resolver is likely dormant).
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdict_fires_only_on_a_genuinely_new_country() {
        let prior = vec!["VN".to_string()];
        assert!(travel_verdict(&prior, Some("US")));
        assert!(!travel_verdict(&prior, Some("VN")));
        // No history yet → nothing to differ from.
        assert!(!travel_verdict(&[], Some("US")));
        // Unresolvable current country → dormant, never fires.
        assert!(!travel_verdict(&prior, None));
    }

    #[test]
    fn no_geo_resolver_is_dormant_by_construction() {
        assert_eq!(NoGeo.country_of("8.8.8.8"), None);
        // Unconfigured path builds the dormant resolver without panicking.
        let resolver = build_geo_resolver(None);
        assert_eq!(resolver.country_of("8.8.8.8"), None);
    }
}
