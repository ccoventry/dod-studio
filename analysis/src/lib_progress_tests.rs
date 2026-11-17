use super::*;

#[test]
fn progress_is_placed_on_one_scale() {
    assert_eq!(scaled(0, 100, 0, PARSE_SHARE), 0);
    assert_eq!(scaled(100, 100, 0, PARSE_SHARE), PARSE_SHARE);
    assert_eq!(scaled(50, 100, PARSE_SHARE, PROGRESS_SCALE), 925);
    assert_eq!(scaled(5, 0, PARSE_SHARE, PROGRESS_SCALE), PROGRESS_SCALE);
    // A 4 GB file's worth of bytes still scales without overflow.
    assert_eq!(scaled(usize::MAX / 2, usize::MAX, 0, 1000), 499);
}
