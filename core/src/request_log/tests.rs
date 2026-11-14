use super::*;

fn asked(seconds: f64, method: &str, path: &str) -> Asked {
    Asked { at: Duration::from_secs_f64(seconds), method: method.into(), path: path.into(), refused: method != "GET" }
}

#[test]
fn kinds_are_counted_without_their_ids() {
    let summary = summarize(&[
        asked(0.0, "GET", "/mail/v4/messages/AbCdEfGhIjKlMnOpQrStUv=="),
        asked(5.0, "GET", "/mail/v4/messages/ZyXwVuTsRqPoNmLkJiHgFe=="),
        asked(9.0, "PUT", "/mail/v4/messages/read"),
    ]);
    assert_eq!(summary.total, 3);
    assert_eq!(summary.refused, 1);
    assert_eq!(
        summary.by_kind,
        vec![("GET /mail/v4/messages/{id}".to_string(), 2), ("PUT /mail/v4/messages/read".to_string(), 1)]
    );
    assert!(summary.by_kind.iter().all(|(kind, _)| !kind.contains("AbCd")), "an id reached the summary");
}

#[test]
fn the_same_request_again_within_a_second_is_a_repeat_and_another_one_is_not() {
    let summary = summarize(&[
        asked(0.0, "GET", "/core/v4/events/one"),
        asked(0.4, "GET", "/core/v4/events/one"),
        asked(0.5, "GET", "/core/v4/events/two"),
        asked(2.0, "GET", "/core/v4/events/one"),
    ]);
    assert_eq!(summary.repeats, vec![("GET /core/v4/events/one".to_string(), 1)]);
}

#[test]
fn the_busiest_minute_is_found_wherever_it_falls() {
    let mut requests: Vec<Asked> = (0..10).map(|n| asked(f64::from(n) * 10.0, "GET", "/a")).collect();
    requests.extend((0..30).map(|n| asked(200.0 + f64::from(n), "GET", "/b")));
    requests.push(asked(400.0, "GET", "/c"));
    assert_eq!(summarize(&requests).most_in_a_minute, 30);
}

#[test]
fn nothing_asked_is_an_empty_summary() {
    assert_eq!(summarize(&[]), RequestSummary::default());
}

#[test]
fn a_log_records_what_it_is_told() {
    let log = RequestLog::new();
    log.record("GET", "/core/v4/users", false);
    log.record("DELETE", "/auth/v4", true);
    let summary = log.summary();
    assert_eq!((summary.total, summary.refused), (2, 1));
}
