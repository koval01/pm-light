//! Whether a domain's records are right at this moment, read through Quad9, without waiting for
//! Proton's own periodic check. The expected values come from Proton's domain settings page.

use serde::Serialize;

const MAIL_HOST: &str = "mail.protonmail.ch";
const BACKUP_HOST: &str = "mailsec.protonmail.ch";
const SPF_INCLUDE: &str = "include:_spf.protonmail.ch";

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub ok: bool,
    /// What is in DNS now, which is public.
    pub found: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LiveCheck {
    pub name: String,
    pub verified: Finding,
    pub mx: Finding,
    pub spf: Finding,
    pub dkim: Finding,
    pub dmarc: Finding,
}

fn host(name: &str) -> String {
    name.trim().trim_end_matches('.').to_ascii_lowercase()
}

#[must_use]
pub fn verified(texts: Vec<String>, code: &str) -> Finding {
    let wanted = code.trim();
    Finding { ok: !wanted.is_empty() && texts.iter().any(|text| text.trim() == wanted), found: texts }
}

/// Proton's server must be the lowest number: other mail hosts would get the mail first.
#[must_use]
pub fn mail_hosts(mut hosts: Vec<(u16, String)>) -> Finding {
    hosts.sort_by_key(|(priority, _)| *priority);
    let first = hosts.first().map(|(_, name)| host(name));
    let backup = hosts.iter().any(|(_, name)| host(name) == BACKUP_HOST);
    Finding {
        ok: first.as_deref() == Some(MAIL_HOST) && backup,
        found: hosts.iter().map(|(priority, name)| format!("{priority} {}", host(name))).collect(),
    }
}

/// A domain may publish only one SPF record: receivers treat two as an error.
#[must_use]
pub fn spf(texts: &[String]) -> Finding {
    let records: Vec<String> =
        texts.iter().filter(|text| text.trim_start().starts_with("v=spf1")).cloned().collect();
    let ok = records.len() == 1 && records.iter().all(|record| record.split_whitespace().any(|part| part == SPF_INCLUDE));
    Finding { ok, found: records }
}

#[must_use]
pub fn dmarc(texts: &[String]) -> Finding {
    let records: Vec<String> =
        texts.iter().filter(|text| text.trim_start().starts_with("v=DMARC1")).cloned().collect();
    let has_policy = |record: &String| record.split(';').any(|part| part.trim().starts_with("p="));
    Finding { ok: records.len() == 1 && records.iter().all(has_policy), found: records }
}

/// Each pair is what Proton expects and what DNS holds for one host.
#[must_use]
pub fn dkim(pairs: Vec<(String, Vec<String>)>) -> Finding {
    let good = |(expected, found): &(String, Vec<String>)| found.iter().any(|alias| host(alias) == host(expected));
    let ok = !pairs.is_empty() && pairs.iter().all(good);
    Finding { ok, found: pairs.into_iter().flat_map(|(_, found)| found).map(|alias| host(&alias)).collect() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(all: &[&str]) -> Vec<String> {
        all.iter().map(|text| (*text).to_string()).collect()
    }

    #[test]
    fn the_verification_code_must_match_exactly() {
        assert!(verified(strings(&["other", " protonmail-verification=abc "]), "protonmail-verification=abc").ok);
        assert!(!verified(strings(&["protonmail-verification=abd"]), "protonmail-verification=abc").ok);
        assert!(!verified(strings(&[""]), "").ok);
    }

    #[test]
    fn proton_must_come_first_with_its_backup() {
        let good = vec![(20, "mailsec.protonmail.ch.".to_string()), (10, "MAIL.protonmail.ch.".to_string())];
        let finding = mail_hosts(good);
        assert!(finding.ok);
        assert_eq!(finding.found[0], "10 mail.protonmail.ch");
        assert!(!mail_hosts(vec![(10, "mail.protonmail.ch.".to_string())]).ok);
        let behind = vec![(1, "mx.other.example.".to_string()), (10, "mail.protonmail.ch.".to_string()), (20, "mailsec.protonmail.ch.".to_string())];
        assert!(!mail_hosts(behind).ok);
        assert!(!mail_hosts(vec![]).ok);
    }

    #[test]
    fn one_spf_record_that_includes_proton() {
        assert!(spf(&strings(&["v=spf1 include:_spf.protonmail.ch ~all", "unrelated"])).ok);
        assert!(spf(&strings(&["v=spf1 include:_spf.protonmail.ch include:spf.example.com ~all"])).ok);
        assert!(!spf(&strings(&["v=spf1 ~all"])).ok);
        assert!(!spf(&strings(&["v=spf1 include:_spf.protonmail.ch ~all", "v=spf1 ~all"])).ok);
        assert!(!spf(&strings(&["include:_spf.protonmail.ch"])).ok);
    }

    #[test]
    fn one_dmarc_record_with_a_policy() {
        assert!(dmarc(&strings(&["v=DMARC1; p=quarantine"])).ok);
        assert!(!dmarc(&strings(&["v=DMARC1"])).ok);
        assert!(!dmarc(&strings(&[])).ok);
        assert!(!dmarc(&strings(&["v=DMARC1; p=none", "v=DMARC1; p=reject"])).ok);
    }

    #[test]
    fn every_dkim_record_must_point_where_proton_says() {
        let pair = |expected: &str, found: &[&str]| (expected.to_string(), strings(found));
        assert!(dkim(vec![pair("a.proton.ch.", &["A.proton.ch."]), pair("b.proton.ch", &["b.proton.ch."])]).ok);
        assert!(!dkim(vec![pair("a.proton.ch", &["a.proton.ch"]), pair("b.proton.ch", &[])]).ok);
        assert!(!dkim(vec![]).ok);
    }
}
