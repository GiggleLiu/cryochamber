use super::*;

#[test]
fn test_format_relative_time_basic() {
    assert_eq!(format_relative_time(0), "now");
    assert_eq!(format_relative_time(-5000), "now");
    assert_eq!(format_relative_time(30_000), "<1m");
    assert_eq!(format_relative_time(60_000), "1m");
    assert_eq!(format_relative_time(3_600_000), "1h 0m");
    assert_eq!(format_relative_time(86_400_000), "1d 0h");
}

#[test]
fn test_classify_relative_time_preserves_unit_boundaries() {
    assert_eq!(classify_relative_time(0), RelativeTimeDisplay::Now);
    assert_eq!(
        classify_relative_time(59_999),
        RelativeTimeDisplay::LessThanMinute
    );
    assert_eq!(
        classify_relative_time(3_599_999),
        RelativeTimeDisplay::Minutes(59)
    );
    assert_eq!(
        classify_relative_time(86_399_999),
        RelativeTimeDisplay::HoursMinutes {
            hours: 23,
            minutes: 59,
        }
    );
    assert_eq!(
        classify_relative_time(172_800_000),
        RelativeTimeDisplay::DaysHours { days: 2, hours: 0 }
    );
}
