//! UTC period math for the quota engine + RFC 3339 formatting.
//!
//! Quota windows are aligned to UTC period starts (a "day" quota resets at
//! 00:00 UTC). Hand-rolled civil-date math (Howard Hinnant's algorithms)
//! avoids pulling a datetime crate into the hot path — these functions run
//! on every gateway request.

/// Quota/rate window sizes (SPEC §6 `quota_rules.period`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Period {
    Minute,
    Hour,
    Day,
    Month,
}

impl Period {
    /// Parses the DB/API wire form; `None` for unknown strings.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "minute" => Some(Self::Minute),
            "hour" => Some(Self::Hour),
            "day" => Some(Self::Day),
            "month" => Some(Self::Month),
            _ => None,
        }
    }

    /// Canonical wire form (stored in `quota_rules.period`).
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Minute => "minute",
            Self::Hour => "hour",
            Self::Day => "day",
            Self::Month => "month",
        }
    }
}

/// Days since 1970-01-01 for a proleptic-Gregorian civil date (Hinnant).
///
/// Integer-only, valid for any year `i64` can hold; months are 1–12.
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // year-of-era, [0, 399]
    let mp = (i64::from(m) + 9) % 12; // March-based month, [0, 11]
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1; // day-of-year, [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // day-of-era
    era * 146097 + doe - 719468
}

/// Inverse of [`days_from_civil`]: civil date for a day count since epoch.
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// UTC start (unix epoch MILLIS) of the period containing `now_ms`.
///
/// `div_euclid`/`rem_euclid` keep this correct even for pre-1970 timestamps
/// (plain `/` and `%` truncate toward zero and would misalign windows).
pub fn period_start_ms(period: Period, now_ms: i64) -> i64 {
    let secs = now_ms.div_euclid(1000);
    let start_secs = match period {
        Period::Minute => secs - secs.rem_euclid(60),
        Period::Hour => secs - secs.rem_euclid(3600),
        Period::Day => {
            let days = secs.div_euclid(86_400);
            days * 86_400
        }
        // Month: drop back to day granularity, then to the 1st of the month.
        Period::Month => {
            let days = secs.div_euclid(86_400);
            let (y, m, _d) = civil_from_days(days);
            days_from_civil(y, m, 1) * 86_400
        }
    };
    start_secs * 1000
}

/// Millis of the NEXT period start after `now_ms` (quota "resets at" hint).
pub fn next_period_start_ms(period: Period, now_ms: i64) -> i64 {
    // Smallest step that can never straddle a boundary of this period size.
    let step_ms = 1000
        * match period {
            Period::Minute => 60,
            Period::Hour => 3600,
            Period::Day => 86_400,
            Period::Month => 86_400 * 31, // upper bound; loop below fixes overshoot
        };
    let mut candidate = period_start_ms(period, now_ms) + step_ms;
    while period_start_ms(period, candidate) <= now_ms {
        candidate += step_ms;
    }
    candidate
}

/// RFC 3339 UTC timestamp, second precision ("2026-10-17T07:31:56Z").
///
/// Used for human-facing reset hints; machines should use epoch millis.
pub fn rfc3339_utc(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let (y, mo, d) = civil_from_days(days);
    let sod = secs.rem_euclid(86_400);
    let (h, mi, s) = (sod / 3600, (sod % 3600) / 60, sod % 60);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Reference values computed with GNU date -u (verified 2026-10-17).
    const OCT_01_2026: i64 = 1_790_812_800;
    const FEB_01_2024: i64 = 1_706_745_600;
    const MAR_01_2024: i64 = 1_709_251_200;

    #[test]
    fn minute_windows_align() {
        assert_eq!(period_start_ms(Period::Minute, 59_999), 0);
        assert_eq!(period_start_ms(Period::Minute, 60_000), 60_000);
        assert_eq!(period_start_ms(Period::Minute, 119_999), 60_000);
    }

    #[test]
    fn day_windows_align() {
        assert_eq!(period_start_ms(Period::Day, 86_399_999), 0);
        assert_eq!(period_start_ms(Period::Day, 86_400_000), 86_400_000);
    }

    #[test]
    fn month_windows_align_including_leap_years() {
        // Mid-October 2026 → 2026-10-01T00:00:00Z.
        assert_eq!(
            period_start_ms(Period::Month, (OCT_01_2026 + 16 * 86_400) * 1000),
            OCT_01_2026 * 1000
        );
        // Leap year: any instant in Feb 2024 → 2024-02-01.
        assert_eq!(
            period_start_ms(Period::Month, (FEB_01_2024 + 28 * 86_400 - 1) * 1000),
            FEB_01_2024 * 1000
        );
        // 2024-02-29 23:59:59.999 still belongs to February.
        assert_eq!(
            period_start_ms(Period::Month, MAR_01_2024 * 1000 - 1),
            FEB_01_2024 * 1000
        );
        // Boundary itself starts the next month.
        assert_eq!(
            period_start_ms(Period::Month, MAR_01_2024 * 1000),
            MAR_01_2024 * 1000
        );
    }

    #[test]
    fn next_period_start_lands_after_now_and_on_boundary() {
        for period in [Period::Minute, Period::Hour, Period::Day, Period::Month] {
            let now = (OCT_01_2026 + 16 * 86_400 + 4_000) * 1000; // mid-month
            let next = next_period_start_ms(period, now);
            assert!(next > now, "{period:?}: next must be after now");
            assert_eq!(
                period_start_ms(period, next),
                next,
                "{period:?}: next must be a boundary"
            );
        }
    }

    #[test]
    fn rfc3339_formats_known_instants() {
        assert_eq!(rfc3339_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339_utc(1_792_222_316_000), "2026-10-17T07:31:56Z");
        // Milliseconds are floored, never rounded up.
        assert_eq!(rfc3339_utc(1_799), "1970-01-01T00:00:01Z");
    }

    #[test]
    fn civil_roundtrip_covers_leap_days() {
        for &(y, m, d) in &[(1970, 1, 1), (2000, 3, 1), (2024, 2, 29), (2026, 10, 17)] {
            let days = days_from_civil(y, m, d);
            assert_eq!(civil_from_days(days), (y, m, d));
        }
        // Known anchor: 2000-03-01 = 951868800 (GNU date).
        assert_eq!(days_from_civil(2000, 3, 1) * 86_400, 951_868_800);
    }

    #[test]
    fn period_wire_forms_roundtrip() {
        for p in [Period::Minute, Period::Hour, Period::Day, Period::Month] {
            assert_eq!(Period::parse(p.as_str()), Some(p));
        }
        assert_eq!(Period::parse("fortnight"), None);
    }
}
