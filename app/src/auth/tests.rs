use super::*;

#[test]
fn a_code_is_digits_and_no_more_of_them_than_it_needs() {
    assert_eq!(digits_of("12 34a5678"), "123456");
    assert_eq!(digits_of("abc"), "");
}

#[test]
fn some_failures_say_nothing() {
    assert_eq!(error_of(&Error::Cancelled).0, 0);
    assert_eq!(error_of(&Error::SessionEnded).0, 0);
    assert_eq!(error_of(&Error::TooManyAttempts { seconds: 5 }).0, 0);
}

#[test]
fn proton_s_own_words_are_shown_with_their_code() {
    let error = Error::Api { status: 422, code: 8002, message: "Incorrect login".to_string() };
    assert_eq!(error_of(&error), (ERROR_SAID, "Incorrect login (code 8002, HTTP 422)".to_string()));
}

#[test]
fn a_silent_server_gets_words_from_the_app() {
    let error = Error::Api { status: 503, code: 0, message: String::new() };
    assert_eq!(error_of(&error).0, ERROR_UNAVAILABLE);
}

#[test]
fn connection_failures_are_told_apart() {
    assert_eq!(error_of(&Error::Network("x".to_string())).0, ERROR_NETWORK);
    assert_eq!(error_of(&Error::Untrusted).0, ERROR_UNTRUSTED);
    assert_eq!(error_of(&Error::WrongPassword).0, ERROR_WRONG_PASSWORD);
}
