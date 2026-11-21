use super::*;

#[test]
fn reads_numbers_as_a_browser_does() {
    assert_eq!(non_negative("3"), Some(3));
    assert_eq!(non_negative("  +12px"), Some(12));
    assert_eq!(non_negative("\t7.9"), Some(7));
    assert_eq!(non_negative("0"), Some(0));
    assert_eq!(non_negative("99999999999999999999999999"), Some(u64::MAX));
}

#[test]
fn reads_no_number_where_there_is_none() {
    for value in ["", " ", "-3", "+", "abc", "x5", "\u{661}"] {
        assert_eq!(non_negative(value), None, "{value:?}");
    }
}

#[test]
fn a_column_span_stops_at_a_thousand() {
    assert_eq!(span("40000", MOST_COLUMNS, false), Some(1000));
    assert_eq!(span("65535", MOST_COLUMNS, false), Some(1000));
    assert_eq!(span("1000", MOST_COLUMNS, false), Some(1000));
    assert_eq!(span("2", MOST_COLUMNS, false), Some(2));
}

#[test]
fn a_column_span_of_nothing_or_zero_is_dropped() {
    for value in ["0", "", "wide", "-1"] {
        assert_eq!(span(value, MOST_COLUMNS, false), None, "{value:?}");
    }
}

#[test]
fn a_row_span_may_be_zero_and_stops_where_browsers_stop() {
    assert_eq!(span("0", MOST_ROWS, true), Some(0));
    assert_eq!(span("70000", MOST_ROWS, true), Some(65_534));
    assert_eq!(span("3", MOST_ROWS, true), Some(3));
}
