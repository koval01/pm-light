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

#[test]
fn a_repeat_is_named_when_it_is_asked() {
    let (log, ms) = (RequestLog::new(), Duration::from_millis);
    let (one, other) =
        ("/mail/v4/conversations/AbCdEfGhIjKlMnOpQrStUv==", "/mail/v4/conversations/ZyXwVuTsRqPoNmLkJiHgFe==");
    assert_eq!(log.note(ms(0), "GET", one, false), None);
    assert_eq!(log.note(ms(300), "GET", other, false), None, "another conversation is no repeat");
    assert_eq!(log.note(ms(700), "GET", one, false), Some(ms(700)));
    assert_eq!(log.note(ms(900), "GET", one, false), Some(ms(200)), "the nearest earlier one counts");
    assert_eq!(log.note(ms(950), "PUT", one, false), None, "another method is no repeat");
    assert_eq!(log.note(ms(1900), "GET", one, false), None, "a second later is no repeat");
    // The summary counts what the journal was told.
    assert_eq!(log.summary().repeats, [("GET /mail/v4/conversations/{id}".to_string(), 2)]);
}

#[test]
fn a_read_after_a_change_is_no_repeat_and_a_change_twice_is() {
    let list = "/mail/v4/conversations?Page=0";
    let summary = summarize(&[
        asked(0.0, "GET", list),
        asked(0.2, "PUT", "/mail/v4/conversations/read"),
        asked(0.4, "GET", list),
        asked(0.5, "GET", list),
        asked(2.0, "PUT", "/mail/v4/conversations/read"),
        asked(2.3, "PUT", "/mail/v4/conversations/read"),
    ]);
    // The read after the change asks anew; the one right after it, with nothing between, repeats it; so does the change.
    assert_eq!(
        summary.repeats,
        vec![("GET /mail/v4/conversations".to_string(), 1), ("PUT /mail/v4/conversations/read".to_string(), 1)]
    );
    let (log, ms) = (RequestLog::new(), Duration::from_millis);
    assert_eq!(log.note(ms(0), "GET", list, false), None);
    assert_eq!(log.note(ms(200), "PUT", "/mail/v4/conversations/read", true), None);
    assert_eq!(log.note(ms(400), "GET", list, false), None, "a read after a change was named a repeat");
    assert_eq!(log.note(ms(500), "GET", list, false), Some(ms(100)));
    assert_eq!(log.note(ms(2000), "PUT", "/mail/v4/conversations/read", true), None);
    assert_eq!(log.note(ms(2300), "PUT", "/mail/v4/conversations/read", true), Some(ms(300)), "a change twice");
}

#[test]
fn errors_proton_answered_are_counted_by_status_and_kind_without_ids() {
    let log = RequestLog::new();
    for status in [200, 204, 301, 399] {
        log.answered("GET", "/mail/v4/messages/abc", status);
    }
    assert!(log.summary().turned_down.is_empty(), "a success was counted as an error");
    log.answered("GET", "/mail/v4/messages/abc==", 422);
    log.answered("GET", "/mail/v4/messages/def==", 422);
    log.answered("GET", "/core/v4/domains", 403);
    log.answered("GET", "/mail/v4/messages/abc==", 500);
    let turned_down = log.summary().turned_down;
    assert_eq!(
        turned_down,
        [
            ("422 GET /mail/v4/messages/{id}".to_string(), 2),
            ("403 GET /core/v4/domains".to_string(), 1),
            ("500 GET /mail/v4/messages/{id}".to_string(), 1),
        ]
    );
    assert!(turned_down.iter().all(|(kind, _)| !kind.contains("abc") && !kind.contains("def")), "an id was kept");
}

#[test]
fn an_answer_is_no_request() {
    let log = RequestLog::new();
    log.answered("GET", "/core/v4/users", 401);
    let summary = log.summary();
    assert_eq!((summary.total, summary.by_kind.len(), summary.turned_down.len()), (0, 0, 1));
}
