use super::*;

#[test]
fn every_boundary_is_new() {
    let all: std::collections::BTreeSet<String> = (0..100).map(|_| boundary()).collect();
    assert_eq!(all.len(), 100);
    assert!(all.iter().all(|one| one.len() == 32 && *one != "0".repeat(32)));
}
