//! Every way out to the network is named here, with the one file each may live in. A person who chose Tor must not
//! be exposed by a new connection added somewhere nobody thought to route through it: such a line fails this test
//! until it is checked and listed, with what keeps it inside Tor written beside it.

#![allow(clippy::expect_used, clippy::indexing_slicing, reason = "a test that cannot read its files must fail")]

use std::{
    fs,
    path::{Path, PathBuf},
};

/// What can reach the network or the outside world, and the only files that may use it.
const WAYS_OUT: &[(&str, &[&str], &str)] = &[
    (
        "reqwest::Client::builder",
        &["core/src/client.rs", "core/src/dns.rs"],
        "Proton's API and Quad9: through Tor when it is chosen",
    ),
    ("reqwest::get", &[], "a request with no chosen way"),
    ("reqwest::blocking", &[], "a request with no chosen way"),
    ("TcpStream::connect", &[], "a connection that bypasses the HTTP clients"),
    ("TcpListener::bind", &["core/src/tor/socks.rs"], "the door to Tor, on the loopback interface only"),
    ("UdpSocket", &[], "anything over UDP, which Tor does not carry"),
    ("ToSocketAddrs", &[], "the system's resolver"),
    ("lookup_host", &[], "the system's resolver"),
    ("getaddrinfo", &[], "the system's resolver"),
    ("open::that", &["app/src/view.rs"], "a link to the browser, which asks first under Tor"),
    (
        ".with_url(",
        &["app/src/mail_view.rs", "app/src/composer.rs", "app/src/preview.rs", "app/src/verify.rs"],
        "webviews: the app's own protocols, and Proton's page through Tor",
    ),
    ("load_url(", &["app/src/mail_view.rs"], "the mail page, over the app's own protocol"),
    ("with_proxy_config", &["app/src/verify.rs"], "Proton's page through Tor"),
    ("hyper::Client", &[], "an HTTP client of its own"),
    ("ureq::", &[], "an HTTP client of its own"),
    ("Command::new", &["app/src/guard.rs"], "the app starting itself again"),
];

fn files(folder: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in fs::read_dir(folder).expect("a folder of the project").flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(files(&path));
        } else if path.extension().is_some_and(|end| end == "rs") {
            found.push(path);
        }
    }
    found
}

/// The code a program is built from: files of tests, and the tests at the end of a file, are left out.
fn shipped(path: &Path) -> Option<String> {
    let text = path.to_string_lossy();
    if text.contains("/tests/") || text.ends_with("tests.rs") || text.ends_with("test_server.rs") {
        return None;
    }
    let source = fs::read_to_string(path).expect("a source file");
    Some(without_test_modules(&source))
}

/// The source with each `#[cfg(test)] mod tests { ... }` taken out, braces counted, and everything around it kept:
/// code after a test module must be read as well.
fn without_test_modules(source: &str) -> String {
    let mut kept = String::new();
    let mut rest = source;
    while let Some(at) = rest.find("#[cfg(test)]\nmod tests") {
        kept.push_str(rest.get(..at).unwrap_or_default());
        let module = rest.get(at..).unwrap_or_default();
        let Some(open) = module.find(['{', ';']) else { break };
        if module.get(open..=open) == Some(";") {
            rest = module.get(open + 1..).unwrap_or_default();
            continue;
        }
        let mut depth = 0usize;
        let mut end = module.len();
        for (position, c) in module.char_indices().skip(open) {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        end = position + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        rest = module.get(end..).unwrap_or_default();
    }
    kept.push_str(rest);
    kept
}

#[test]
fn every_way_out_is_where_it_is_known_to_be() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("the project");
    let mut sources = Vec::new();
    for folder in ["core/src", "app/src"] {
        for path in files(&root.join(folder)) {
            if let Some(code) = shipped(&path) {
                let relative =
                    path.strip_prefix(root).expect("inside the project").to_string_lossy().replace('\\', "/");
                sources.push((relative, code));
            }
        }
    }
    assert!(sources.len() > 50, "the sources did not load");
    let mut found = Vec::new();
    for (way, allowed, why) in WAYS_OUT {
        for (file, code) in &sources {
            if code.contains(way) && !allowed.contains(&file.as_str()) {
                found.push(format!("{file} uses {way} ({why}): route it through Tor, then list it here"));
            }
        }
    }
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn code_after_a_test_module_is_read_too() {
    let source = "fn a() {}\n#[cfg(test)]\nmod tests {\n    fn t() { let x = TcpStream::connect(1); }\n}\nfn b() { TcpStream::connect(2); }\n";
    let kept = without_test_modules(source);
    assert!(kept.contains("fn b() { TcpStream::connect(2); }"));
    assert!(!kept.contains("connect(1)"));
    assert!(without_test_modules("x\n#[cfg(test)]\nmod tests;\ny").contains('y'));
}

#[test]
fn the_list_itself_is_checked() {
    let code = "fn leak() { let s = std::net::TcpStream::connect(\"x:1\"); }";
    assert!(WAYS_OUT.iter().any(|(way, allowed, _)| code.contains(way) && allowed.is_empty()));
}

/// WebKit and Chromium look up the names of links on a page before anyone clicks them, by the system's own resolver,
/// past Tor and past Quad9. Every page the app writes says not to.
#[test]
fn no_page_looks_up_the_names_of_its_links() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("the project");
    let mut pages = 0;
    for path in files(&root.join("app/src")) {
        let Some(code) = shipped(&path) else { continue };
        for (at, _) in code.match_indices("<!doctype html>") {
            pages += 1;
            let head = code.get(at..).and_then(|page| page.split("</head>").next()).unwrap_or_default();
            assert!(
                head.contains(r#"<meta http-equiv="x-dns-prefetch-control" content="off">"#),
                "{} writes a page that may look up its links' names",
                path.display()
            );
        }
    }
    assert!(pages >= 4, "only {pages} pages were found");
}
