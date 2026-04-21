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
