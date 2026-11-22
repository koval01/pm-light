//! Proton's categories of the inbox, as its settings show them under "Email categories": whether the inbox is shown by
//! category, and which categories have a tab of their own. Primary always has one, and so does the last other one
//! while categories are on: hiding them all is turning categories off, which is the switch above them.

use super::{
    Ctx,
    rows::{note, plain, section, switch},
};
use crate::{SettingsRow, mail_view, view};
use proton_core::categories::{Categories, PRIMARY};
use slint::ComponentHandle;

const VIEW: &str = "categoryView";
const SHOWN: &str = "category:";

pub fn page(ctx: &Ctx) {
    let service = ctx.service.clone();
    ctx.load(
        async move {
            service.read_categories().await;
            Ok(service.categories())
        },
        |ctx, categories| draw(ctx, &categories),
    );
}

fn title(ctx: &Ctx, name: &str) -> String {
    ctx.app.upgrade().map_or_else(String::new, |app| app.global::<crate::Categories>().invoke_title(name.into()).into())
}

/// What a category holds, in the words of Proton's settings.
fn about(ctx: &Ctx, name: &str) -> String {
    match name {
        "primary" => ctx.say("categories.primary"),
        "social" => ctx.say("categories.social"),
        "promotions" => ctx.say("categories.promotions"),
        "newsletters" => ctx.say("categories.newsletters"),
        "transactions" => ctx.say("categories.transactions"),
        "updates" => ctx.say("categories.updates"),
        _ => String::new(),
    }
}

/// The switch for each category, greyed where Proton's settings do not let it change.
#[must_use]
pub fn category_rows(ctx: &Ctx, categories: &Categories) -> Vec<SettingsRow> {
    let others_shown = categories.tabs().iter().filter(|tab| tab.id != PRIMARY).count();
    categories
        .list
        .iter()
        .map(|category| {
            let shown = category.shown || category.id == PRIMARY;
            let fixed = category.id == PRIMARY || (shown && others_shown <= 1);
            let about = about(ctx, category.name);
            SettingsRow {
                enabled: !fixed,
                ..switch(&format!("{SHOWN}{}", category.id), &title(ctx, category.name), &about, shown)
            }
        })
        .collect()
}

fn draw(ctx: &Ctx, categories: &Categories) {
    if !categories.allowed {
        ctx.show(vec![plain(vec![note(&ctx.say("categories.notAllowed"))])]);
        return;
    }
    let mut sections =
        vec![plain(vec![switch(VIEW, &ctx.say("categories.use"), &ctx.say("categories.useHint"), categories.chosen)])];
    if categories.chosen && !categories.list.is_empty() {
        let mut rows = category_rows(ctx, categories);
        rows.push(note(&ctx.say("categories.lastShown")));
        sections.push(section("categories", &ctx.say("categories.list"), "", rows));
    }
    ctx.show(sections);
}

pub fn heard(ctx: &Ctx, id: &str, value: i32) {
    let (service, on) = (ctx.service.clone(), value == 1);
    if id == VIEW {
        log::info!("categories turned {}", if on { "on" } else { "off" });
        ctx.act(async move { service.set_categories_on(on).await }, changed);
    } else if let Some(category) = id.strip_prefix(SHOWN) {
        let category = category.to_string();
        let allowed = category_rows(ctx, &ctx.service.categories()).iter().any(|row| row.id == id && row.enabled);
        if allowed {
            log::info!("the category {} {}", proton_core::categories::name_of(&category).unwrap_or("unknown"), on);
            ctx.act(async move { service.show_category(&category, on).await }, changed);
        }
    }
}

/// The page, the tabs and the inbox follow a change. A tab that went is Primary again, and the mail open in the inbox
/// is let go, as the inbox opens anew.
fn changed(ctx: &Ctx) {
    page(ctx);
    crate::auth::refresh_folders(&ctx.service, &ctx.app);
    if ctx.app.upgrade().is_some_and(|app| app.get_folder_id() == "inbox") {
        mail_view::close();
        view::let_go_of_mail();
        if let Some(app) = ctx.app.upgrade() {
            app.set_has_mail(false);
        }
        let service = ctx.service.clone();
        tokio::spawn(async move { service.view_resume().await });
    }
}
