use super::*;
use std::{
    io::{Read, Write},
    net::TcpStream,
    time::{Duration, Instant},
};

/// What a connection to the door gets back before it is closed, read until the end or two seconds.
fn answer_to(hello: &[u8]) -> Vec<u8> {
    let at = door().expect("the door opens");
    let mut stream = TcpStream::connect(at).expect("the door takes a connection");
    stream.set_read_timeout(Some(Duration::from_secs(2))).expect("a timeout");
    let _ = stream.write_all(hello);
    let mut answer = Vec::new();
    let _ = stream.read_to_end(&mut answer);
    answer
}

#[test]
fn the_door_is_on_this_machine_only_and_stays_where_it_is() {
    let at = door().expect("the door opens");
    assert!(at.ip().is_loopback(), "the door listens beyond this machine: {at}");
    assert_ne!(at.port(), 0);
    assert_eq!(door(), Some(at), "a second door was opened");
}

#[test]
fn a_proxy_greeting_gets_no_answer_and_the_connection_ends() {
    let started = Instant::now();
    // SOCKS5: version 5, one method, no authentication. A proxy would answer with two bytes.
    assert!(answer_to(&[5, 1, 0]).is_empty(), "the door answered a SOCKS greeting");
    // An HTTP proxy's request for a tunnel to a tracker.
    assert!(answer_to(b"CONNECT tracker.example:443 HTTP/1.1\r\nHost: tracker.example:443\r\n\r\n").is_empty());
    assert!(answer_to(b"").is_empty());
    assert!(started.elapsed() < Duration::from_secs(5), "the door kept a connection open until the timeout");
}

#[test]
fn every_connection_turned_away_is_counted() {
    let before = turned_away();
    for _ in 0..3 {
        let _ = answer_to(&[5, 1, 0]);
    }
    // The count is shared with the other tests, which run beside this one: at least these three.
    let deadline = Instant::now() + Duration::from_secs(2);
    while turned_away() < before + 3 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(turned_away() >= before + 3, "{} turned away, {before} before", turned_away());
}

#[test]
fn the_journal_hears_of_the_first_and_then_ten_times_fewer() {
    let said: Vec<u64> = (0..=10_001).filter(|count| worth_saying(*count)).collect();
    assert_eq!(said, [1, 10, 100, 1_000, 10_000]);
    assert!(worth_saying(10_000_000_000_000_000_000));
    assert!(!worth_saying(u64::MAX));
}
