//! About: who made the app, the license it is under with its additional terms, and every component it is built
//! from, with its version and license. These are the program's Appropriate Legal Notices in the sense of the GNU
//! AGPL, and the additional terms in `NOTICE` require that they stay.
//!
//! The license comes from `LICENSE` and `NOTICE` at the root of the repository, the components from what
//! `build.rs` took from `components/` for this system: nothing here is written twice.

use super::{
    Ctx,
    rows::{self, ghost, note, plain, section},
};
use crate::{preview, view};
use proton_core::REPOSITORY;

/// The license and its additional terms, as the repository holds them.
pub const NOTICE: &str = include_str!("../../../NOTICE");
pub const LICENSE: &str = include_str!("../../../LICENSE");
/// The components of the program for this system: name, version, license, source, systems.
pub const COMPONENTS: &str = include_str!(concat!(env!("OUT_DIR"), "/components.tsv"));
/// Their license and notice files, each text once, compressed.
const NOTICES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/notices.xz"));
/// A list of hundreds of rows makes the page slow to draw: the rest is found by searching.
const SHOWN: usize = 150;
const SEARCH: &str = "about-search";
const LIST: &str = "about-list";
const COPYRIGHT: &str = "Copyright (C) 2026 Yaroslav Koval";

/// One component of the program.
#[derive(Debug, PartialEq, Eq)]
pub struct Component<'a> {
    pub name: &'a str,
    pub version: &'a str,
    pub license: &'a str,
}

pub fn components() -> impl Iterator<Item = Component<'static>> {
    COMPONENTS.lines().filter_map(|line| {
        let mut fields = line.split('\t');
        Some(Component { name: fields.next()?, version: fields.next()?, license: fields.next()? })
    })
}

/// The components a search names, by name or license.
pub fn matching<'a>(wanted: &str, all: impl Iterator<Item = Component<'a>>) -> Vec<Component<'a>> {
    let wanted = wanted.trim().to_lowercase();
    all.filter(|component| {
        wanted.is_empty() || component.name.to_lowercase().contains(&wanted) || component.license.to_lowercase().contains(&wanted)
    })
    .collect()
}

fn list_rows(ctx: &Ctx) -> Vec<crate::SettingsRow> {
    let found = matching(&ctx.text(SEARCH), components());
    let mut lines: Vec<_> = found
        .iter()
        .take(SHOWN)
        .map(|component| rows::fact(&format!("{} {}", component.name, component.version), component.license))
        .collect();
    if found.len() > SHOWN {
        let more = (found.len() - SHOWN).to_string();
        lines.push(note(&ctx.say_with("about.more", &[("count", &more)])));
    }
    if found.is_empty() {
        lines.push(note(&ctx.say("about.nothing")));
    }
    lines
}

pub fn page(ctx: &Ctx) {
    if ctx.has_row(SEARCH) {
        ctx.replace(LIST, list_rows(ctx));
        return;
    }
    let version = env!("CARGO_PKG_VERSION");
    let count = components().count().to_string();
    ctx.show(vec![
        plain(vec![
            note(&format!("Proton Mail Light v{version}. {}", ctx.say("login.unofficial"))),
            note(COPYRIGHT),
            ghost("repository", REPOSITORY, true),
        ]),
        section("license", &ctx.say("about.licenseTitle"), "", vec![
            note(&ctx.say("about.license")),
            ghost("about-license", &ctx.say("about.readLicense"), true),
        ]),
        section("components", &ctx.say("about.componentsTitle"), "", vec![
            note(&ctx.say_with("about.components", &[("count", &count)])),
            rows::SettingsRow { detail: ctx.say("about.search").into(), ..rows::field(SEARCH, "", "") },
            ghost("about-notices", &ctx.say("about.readNotices"), true),
        ]),
        section(LIST, "", "", list_rows(ctx)),
    ]);
}

pub fn heard(ctx: &Ctx, id: &str) {
    let Some(app) = ctx.app.upgrade() else { return };
    match id {
        "repository" => view::open_link(REPOSITORY),
        "about-license" => preview::show_text(&app, "LICENSE.txt", format!("{NOTICE}\n\n{LICENSE}").into_bytes()),
        "about-notices" => match liblzma::decode_all(NOTICES) {
            Ok(texts) => preview::show_text(&app, "THIRD-PARTY-LICENSES.txt", texts),
            Err(error) => {
                let error = proton_core::error::Error::from(error);
                log::error!("the license texts of the components did not open: {}", error.kind());
                ctx.tell(&ctx.say("errors.internal"));
            }
        },
        _ => {}
    }
}

#[cfg(test)]
mod tests;
