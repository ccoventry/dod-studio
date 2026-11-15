//! How long something took, the one way DoD Studio writes it (#880): `2.1 s`
//! under a minute, then whole seconds as `6 min 12 s` and `1 h 2 min 3 s`.
//! The activity log's lines use it; the frontend's twin is
//! `studio/src/time_format.js`'s `durationText`, and the tests below pin the
//! same cases as its tests.

/// `secs` as `2.1 s`, `6 min 12 s` or `1 h 2 min 3 s`. Negative or NaN reads
/// `0.0 s`.
pub fn duration_text(secs: f64) -> String {
    let x = if secs.is_finite() && secs > 0.0 {
        secs
    } else {
        0.0
    };
    let tenths = (x * 10.0).round();
    if tenths < 600.0 {
        return format!("{:.1} s", tenths / 10.0);
    }
    let whole = x.round() as u64;
    let (h, m, s) = (whole / 3600, (whole % 3600) / 60, whole % 60);
    if h > 0 {
        format!("{h} h {m} min {s} s")
    } else {
        format!("{m} min {s} s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_decimal_and_a_space_under_a_minute() {
        assert_eq!(duration_text(2.14), "2.1 s");
        assert_eq!(duration_text(2.0), "2.0 s");
        assert_eq!(duration_text(0.0), "0.0 s");
        assert_eq!(duration_text(59.94), "59.9 s");
    }

    #[test]
    fn whole_minutes_and_seconds_from_a_minute() {
        assert_eq!(duration_text(59.96), "1 min 0 s");
        assert_eq!(duration_text(372.4), "6 min 12 s");
        assert_eq!(duration_text(3599.4), "59 min 59 s");
    }

    #[test]
    fn hours_from_an_hour() {
        assert_eq!(duration_text(3600.0), "1 h 0 min 0 s");
        assert_eq!(duration_text(3723.0), "1 h 2 min 3 s");
    }

    #[test]
    fn negative_or_nan_reads_zero() {
        assert_eq!(duration_text(-4.0), "0.0 s");
        assert_eq!(duration_text(f64::NAN), "0.0 s");
    }
}
