use super::{COVERS, SHOWN, covered};

/// Panels over the mail are counted: the page comes back only when the last one goes, and a close too many never
/// leaves it hidden for good.
#[test]
fn the_page_comes_back_only_when_nothing_covers_it() {
    SHOWN.set(true);
    covered(true);
    covered(true);
    assert_eq!(COVERS.get(), 2);
    covered(false);
    assert_eq!(COVERS.get(), 1);
    covered(false);
    covered(false);
    assert_eq!(COVERS.get(), 0);
    covered(true);
    assert_eq!(COVERS.get(), 1);
    covered(false);
    assert!(SHOWN.get(), "covering is not closing: the page is still meant to show");
}
