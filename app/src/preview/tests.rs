use super::*;

fn served() -> Served {
    Served { page: b"page".to_vec(), file: Arc::new(b"0123456789".to_vec()), file_type: "video/mp4" }
}

fn asked(path: &str, range: Option<&str>) -> Response<Cow<'static, [u8]>> {
    let mut request = Request::builder().uri(format!("attachment://preview{path}"));
    if let Some(range) = range {
        request = request.header(header::RANGE, range);
    }
    answer(&served(), &request.body(Vec::new()).unwrap())
}

#[test]
fn text_cannot_become_a_tag() {
    let content = content_of(&Shown::Text, b"<script>alert(1)</script>").unwrap();
    assert!(!content.contains("<script>") && content.contains("&lt;script&gt;"));
}

#[test]
fn a_range_is_read_as_a_browser_writes_it() {
    assert_eq!(byte_range(Some("bytes=0-1"), 10), Some((0, 1)));
    assert_eq!(byte_range(Some("bytes=4-"), 10), Some((4, 9)));
    assert_eq!(byte_range(Some("bytes=-3"), 10), Some((7, 9)));
    assert_eq!(byte_range(Some("bytes=5-99"), 10), Some((5, 9)));
}

#[test]
fn a_range_that_makes_no_sense_is_refused() {
    for wanted in ["bytes=9-2", "bytes=10-", "bytes=x-y", "items=0-1", ""] {
        assert_eq!(byte_range(Some(wanted), 10), None, "{wanted}");
    }
    assert_eq!(byte_range(Some("bytes=0-1"), 0), None);
    assert_eq!(byte_range(None, 10), None);
}

#[test]
fn the_file_comes_whole_or_in_parts_with_the_type_found() {
    let whole = asked(FILE_PATH, None);
    assert_eq!(whole.body().as_ref(), b"0123456789");
    assert_eq!(whole.headers().get(header::CONTENT_TYPE).unwrap(), "video/mp4");
    let part = asked(FILE_PATH, Some("bytes=2-4"));
    assert_eq!(part.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(part.body().as_ref(), b"234");
    assert_eq!(part.headers().get(header::CONTENT_RANGE).unwrap(), "bytes 2-4/10");
    assert_eq!(asked(FILE_PATH, Some("bytes=50-")).status(), StatusCode::RANGE_NOT_SATISFIABLE);
}

#[test]
fn nothing_else_exists_on_the_protocol() {
    assert_eq!(asked("/other", None).status(), StatusCode::NOT_FOUND);
    assert_eq!(asked("/", None).status(), StatusCode::NOT_FOUND);
}

#[test]
fn every_answer_forbids_guessing_and_loading_elsewhere() {
    for path in [PAGE_PATH, FILE_PATH] {
        let answer = asked(path, None);
        assert_eq!(answer.headers().get(header::X_CONTENT_TYPE_OPTIONS).unwrap(), "nosniff");
        assert!(
            answer
                .headers()
                .get(header::CONTENT_SECURITY_POLICY)
                .unwrap()
                .to_str()
                .unwrap()
                .contains("default-src 'none'")
        );
    }
}
