//! Settings, a mode of the main window. The mail's webview is closed while they are open, and a page
//! is put together only when its tab is opened. Each page says what it shows, and hears what is done
//! to it, in one of the files here.

mod account_pages;
mod about_page;
mod app_pages;
mod tor_page;
mod filter_pages;
mod guarded_pages;
mod lock_page;
mod mail_pages;
mod print_check;
mod program_pages;
pub mod rows;
mod tabs;

use crate::{App, Say, Settings, SettingsRow, SettingsSection, mail_view, view};
use proton_core::{Service, error::Error};
use slint::{ComponentHandle, Model, ModelRc, VecModel, Weak};
use std::{cell::Cell, future::Future, rc::Rc, sync::Arc};

pub const FIRST_TAB: &str = "overview";
pub use app_pages::{LANGUAGES, choose_language};

thread_local! {
    /// A page that was left must not draw itself when its answer comes in.
    static GENERATION: Cell<u64> = const { Cell::new(0) };
}

/// What a page works with. It can travel to a task and back.
#[derive(Clone)]
pub struct Ctx {
    pub service: Arc<Service>,
    pub app: Weak<App>,
}

impl Ctx {
    pub fn say(&self, key: &str) -> String {
        self.app.upgrade().map_or_else(String::new, |app| say(&app, key))
    }

    /// With its `{name}`s filled in, as the page's `t` does.
    pub fn say_with(&self, key: &str, values: &[(&str, &str)]) -> String {
        values.iter().fold(self.say(key), |text, (name, value)| text.replace(&format!("{{{name}}}"), value))
    }

    /// Draws a page, on the window's thread.
    pub fn show(&self, sections: Vec<SettingsSection>) {
        if let Some(app) = self.app.upgrade() {
            let settings = app.global::<Settings>();
            settings.set_loading(false);
            settings.set_sections(ModelRc::from(Rc::new(VecModel::from(sections))));
        }
    }

    /// What a page has to ask for. The skeleton shows until it comes.
    pub fn load<T: Send + 'static>(
        &self,
        task: impl Future<Output = Result<T, Error>> + Send + 'static,
        draw: impl FnOnce(&Self, T) + Send + 'static,
    ) {
        let generation = GENERATION.get();
        let ctx = self.clone();
        tokio::spawn(async move {
            let answer = task.await;
            let app = ctx.app.clone();
            let _ = app.upgrade_in_event_loop(move |_| {
                if GENERATION.get() != generation {
                    return;
                }
                match answer {
                    Ok(value) => draw(&ctx, value),
                    Err(error) => {
                        ctx.show(Vec::new());
                        ctx.report(&error);
                    }
                }
            });
        });
    }

    /// Something that changes the account. A failure is said aloud, and nothing else happens.
    pub fn act(&self, task: impl Future<Output = Result<(), Error>> + Send + 'static, then: impl FnOnce(&Self) + Send + 'static) {
        let generation = GENERATION.get();
        let ctx = self.clone();
        tokio::spawn(async move {
            let answer = task.await;
            let app = ctx.app.clone();
            let _ = app.upgrade_in_event_loop(move |_| match answer {
                Ok(()) if GENERATION.get() == generation => then(&ctx),
                Ok(()) => {}
                Err(error) => ctx.report(&error),
            });
        });
    }

    pub fn tell(&self, text: &str) {
        if let Some(app) = self.app.upgrade() {
            app.global::<Settings>().set_notice(text.into());
        }
    }

    pub fn report(&self, error: &Error) {
        log::warn!("a settings action failed: {}", error.kind());
        self.tell(&self.error_text(error));
    }

    pub fn error_text(&self, error: &Error) -> String {
        match error {
            Error::Untrusted => self.say("app.untrusted"),
            Error::CannotMove => self.say("error.cannotMove"),
            Error::BadTime => self.say("error.badTime"),
            Error::PastTime => self.say("error.pastTime"),
            Error::UnreadableScan => self.say("printCheck.unreadableScan"),
            Error::InvalidAddress(address) => self.say_with("error.badAddress", &[("address", address)]),
            other => other.to_string(),
        }
    }

    fn row(&self, id: &str) -> Option<SettingsRow> {
        let app = self.app.upgrade()?;
        let sections = app.global::<Settings>().get_sections();
        sections.iter().flat_map(|section| section.rows.iter().collect::<Vec<_>>()).find(|row| row.id == id)
    }

    /// What a field on the page holds now.
    pub fn text(&self, id: &str) -> String {
        self.row(id).map_or_else(String::new, |row| row.text.to_string())
    }

    pub fn row_title(&self, id: &str) -> String {
        self.row(id).map_or_else(String::new, |row| row.title.to_string())
    }

    pub fn row_button(&self, id: &str) -> String {
        self.row(id).map_or_else(String::new, |row| row.button.to_string())
    }

    /// Keeps digits only in a code's field, and fills its cells.
    pub fn tidy_code(&self, id: &str) -> String {
        let digits = rows::code_digits(&self.text(id));
        let Some(app) = self.app.upgrade() else { return digits };
        for section in app.global::<Settings>().get_sections().iter() {
            for (index, row) in section.rows.iter().enumerate() {
                if row.id == id {
                    section.rows.set_row_data(index, SettingsRow { text: digits.as_str().into(), options: rows::digit_cells(&digits), ..row });
                }
            }
        }
        digits
    }

    /// Draws a script in colour again. The text stays as it was typed, and the field keeps its place.
    pub fn recolour(&self, id: &str) {
        let Some(app) = self.app.upgrade() else { return };
        for section in app.global::<Settings>().get_sections().iter() {
            for (index, row) in section.rows.iter().enumerate() {
                if row.id == id {
                    let code = rows::code_lines(&row.text);
                    section.rows.set_row_data(index, SettingsRow { code, ..row });
                }
            }
        }
    }

    pub fn has_row(&self, id: &str) -> bool {
        self.row(id).is_some()
    }

    /// New rows for one section, which leaves the fields of the others alone.
    pub fn replace(&self, section: &str, rows: Vec<SettingsRow>) {
        let Some(app) = self.app.upgrade() else { return };
        let sections = app.global::<Settings>().get_sections();
        let Some(position) = sections.iter().position(|known| known.id == section) else { return };
        if let Some(found) = sections.row_data(position) {
            sections.set_row_data(position, SettingsSection { rows: ModelRc::from(Rc::new(VecModel::from(rows))), ..found });
        }
    }

    /// Greys a button in or out as a form fills up, which must not disturb the field being typed in.
    pub fn enable(&self, id: &str, enabled: bool) {
        let Some(app) = self.app.upgrade() else { return };
        for section in app.global::<Settings>().get_sections().iter() {
            for (index, row) in section.rows.iter().enumerate() {
                if row.id == id && row.enabled != enabled {
                    section.rows.set_row_data(index, SettingsRow { enabled, ..row });
                }
            }
        }
    }
}

pub fn say(app: &App, key: &str) -> String {
    app.global::<Say>().invoke_text(key.into()).to_string()
}

fn refresh_tabs(app: &App) {
    let settings = app.global::<Settings>();
    let query = settings.get_query();
    let rows = tabs::shown(&|key| say(app, key), &query);
    settings.set_tabs(ModelRc::from(Rc::new(VecModel::from(rows))));
}

fn web_tab(ctx: &Ctx) {
    ctx.show(vec![rows::plain(vec![rows::note(&ctx.say("tabs.webOnly")), rows::button("site", &ctx.say("tabs.openInProton"), true)])]);
}

fn draw(ctx: &Ctx, id: &str) {
    let Some(tab) = tabs::find(id) else { return };
    if !tab.web.is_empty() {
        web_tab(ctx);
        return;
    }
    match id {
        "overview" => account_pages::overview(ctx),
        "subscription" => account_pages::subscription(ctx),
        "recovery" => guarded_pages::recovery(ctx),
        "account" => guarded_pages::protection(ctx),
        "security" => account_pages::security(ctx),
        "encryption" => account_pages::encryption(ctx),
        "domains" => account_pages::domains(ctx),
        "language" => app_pages::language(ctx),
        "appearance" => app_pages::appearance(ctx),
        "application" => app_pages::application(ctx),
        "connection" => app_pages::connection(ctx),
        "tor" => tor_page::page(ctx),
        "journal" => app_pages::journal(ctx),
        "keyboard" => app_pages::keyboard(ctx),
        "about" => about_page::page(ctx),
        "messages" => mail_pages::messages(ctx),
        "privacy" => mail_pages::privacy(ctx),
        "identity" => mail_pages::identity(ctx),
        "folders" => mail_pages::folders(ctx),
        "forwarding" => mail_pages::forwarding(ctx),
        "filters" => filter_pages::filters(ctx),
        "printCheck" => print_check::page(ctx),
        "referral" => program_pages::referral(ctx),
        "apps" => program_pages::apps(ctx),
        "imap" => program_pages::imap(ctx),
        "backup" => program_pages::backup(ctx),
        "multiUser" => program_pages::organization(ctx),
        _ => {}
    }
}

fn hear(ctx: &Ctx, tab: &str, id: &str, part: i32, value: i32) {
    if id == "site" {
        if let Some(found) = tabs::find(tab) {
            view::open_link(&tabs::web_address(found.web));
        }
        return;
    }
    if let Some(slug) = id.strip_prefix("manage:") {
        view::open_link(&if slug.is_empty() { "https://account.proton.me".to_string() } else { tabs::web_address(slug) });
        return;
    }
    match tab {
        "overview" | "domains" => account_pages::heard(ctx, id, part),
        "recovery" | "account" => guarded_pages::heard(ctx, id, part, value),
        "subscription" => drop(account_pages::heard_account(ctx, id, value)),
        "encryption" => drop(mail_pages::heard_choice(ctx, id, value)),
        "security" => {
            if !lock_page::heard(ctx, id, value) && !guarded_pages::heard_card(ctx, id, part) {
                account_pages::heard_security(ctx, id, value);
            }
        }
        "tor" => tor_page::heard(ctx, id, value),
        "about" => about_page::heard(ctx, id),
        "language" | "appearance" | "application" | "connection" | "journal" | "keyboard" => {
            app_pages::heard(ctx, tab, id, value);
        }
        "messages" | "privacy" | "identity" | "folders" | "forwarding" => mail_pages::heard(ctx, id, part, value),
        "filters" => filter_pages::heard(ctx, id, part, value),
        "printCheck" => print_check::heard(ctx, id),
        "referral" | "apps" | "imap" | "backup" | "multiUser" => program_pages::heard(ctx, id),
        _ => {}
    }
}

fn typed(ctx: &Ctx, tab: &str, id: &str) {
    match tab {
        "security" => {
            lock_page::typed(ctx);
            guarded_pages::typed(ctx, id);
        }
        "journal" if id == "journal-search" => app_pages::journal(ctx),
        "about" if id == "about-search" => about_page::page(ctx),
        "filters" => filter_pages::typed(ctx, id),
        "printCheck" => print_check::typed(ctx),
        "folders" => mail_pages::typed(ctx),
        "identity" => mail_pages::address_typed(ctx, id),
        "imap" => program_pages::typed(ctx),
        "recovery" | "account" => guarded_pages::typed(ctx, id),
        _ => {}
    }
}

fn go(ctx: &Ctx, id: &str) {
    let Some(app) = ctx.app.upgrade() else { return };
    let Some(tab) = tabs::find(id) else { return };
    GENERATION.set(GENERATION.get() + 1);
    program_pages::forget();
    guarded_pages::forget();
    let settings = app.global::<Settings>();
    settings.set_tab(id.into());
    settings.set_title(say(&app, tab.title).into());
    settings.set_page(settings.get_page() + 1);
    settings.set_sections(ModelRc::default());
    settings.set_loading(true);
    draw(ctx, id);
}

/// The account panel's two lines: the plan, and the space used. Worded as the overview page words them.
pub fn account_lines(
    service: &Arc<Service>,
    app: &App,
    account: &proton_core::model::Account,
    storage: Option<&proton_core::model::Storage>,
) -> (String, String) {
    let ctx = Ctx { service: service.clone(), app: app.as_weak() };
    let space = storage.map(|storage| account_pages::storage_text(&ctx, storage)).unwrap_or_default();
    (account_pages::plan_text(&ctx, account), space)
}

/// Tor's state or circuit changed: a page showing it draws its part again.
pub fn tor_changed(service: &Arc<Service>, app: &App) {
    let settings = app.global::<Settings>();
    if !settings.get_open() {
        return;
    }
    let tab = settings.get_tab();
    if tab == "tor" || tab == "connection" {
        tor_page::redraw(&Ctx { service: service.clone(), app: app.as_weak() }, app, &tab);
    }
}

/// Another tab, from a link on a page. After the event that asked for it: the page's rows are still in use until
/// it returns.
pub fn open_tab(ctx: &Ctx, id: &'static str) {
    let ctx = ctx.clone();
    slint::Timer::single_shot(std::time::Duration::ZERO, move || go(&ctx, id));
}

pub fn all_tabs() -> impl Iterator<Item = &'static tabs::Tab> {
    tabs::all().filter(|tab| tab.web.is_empty())
}

/// Draws the page in view again, for what changed under it.
pub fn redraw(ctx: &Ctx) {
    let Some(app) = ctx.app.upgrade() else { return };
    let tab = app.global::<Settings>().get_tab();
    draw(ctx, &tab);
}

pub fn language_changed(ctx: &Ctx) {
    let Some(app) = ctx.app.upgrade() else { return };
    refresh_tabs(&app);
    let settings = app.global::<Settings>();
    if let Some(tab) = tabs::find(&settings.get_tab()) {
        settings.set_title(say(&app, tab.title).into());
    }
}

/// The session is gone, and what these pages showed with it.
pub fn abandon(app: &App) {
    GENERATION.set(GENERATION.get() + 1);
    let settings = app.global::<Settings>();
    settings.set_open(false);
    settings.set_sections(ModelRc::default());
}

/// The mail goes while settings are open, and comes back after them.
pub fn open(service: &Arc<Service>, app: &App, tab: &str) {
    log::info!("settings: {tab}");
    let settings = app.global::<Settings>();
    if !settings.get_open() {
        mail_view::close();
    }
    settings.set_query("".into());
    refresh_tabs(app);
    settings.set_open(true);
    go(&Ctx { service: service.clone(), app: app.as_weak() }, tab);
}

/// Settings closed from one of its pages, for something that happens in the window behind it.
pub fn close_for(ctx: &Ctx) {
    if let Some(app) = ctx.app.upgrade() {
        close(&ctx.service, &app);
    }
}

fn close(service: &Arc<Service>, app: &App) {
    GENERATION.set(GENERATION.get() + 1);
    program_pages::forget();
    guarded_pages::forget();
    let settings = app.global::<Settings>();
    settings.set_open(false);
    settings.set_sections(ModelRc::default());
    settings.set_tabs(ModelRc::default());
    if let Some(id) = view::current_id() {
        view::open_mail(service, &app.as_weak(), id);
    }
}

pub fn connect(service: &Arc<Service>, app: &App) {
    let settings = app.global::<Settings>();
    let ctx = Ctx { service: service.clone(), app: app.as_weak() };

    let (going, weak) = (ctx.clone(), app.as_weak());
    settings.on_go(move |id| {
        if let Some(app) = weak.upgrade() {
            app.global::<Settings>().set_notice("".into());
        }
        go(&going, &id);
    });
    let (leaving, weak) = (ctx.clone(), app.as_weak());
    settings.on_back(move || {
        if let Some(app) = weak.upgrade() {
            close(&leaving.service, &app);
        }
    });
    let weak = app.as_weak();
    settings.on_search(move |_| {
        if let Some(app) = weak.upgrade() {
            refresh_tabs(&app);
        }
    });
    let (hearing, weak) = (ctx.clone(), app.as_weak());
    settings.on_event(move |id, part, value| {
        if let Some(app) = weak.upgrade() {
            let tab = app.global::<Settings>().get_tab();
            hear(&hearing, &tab, &id, part, value);
        }
    });
    let (typing, weak) = (ctx, app.as_weak());
    settings.on_edited(move |id| {
        if let Some(app) = weak.upgrade() {
            let tab = app.global::<Settings>().get_tab();
            typed(&typing, &tab, &id);
        }
    });
    let (tools, weak) = (service.clone(), app.as_weak());
    app.on_tool_chosen(move |tool| {
        let Some(app) = weak.upgrade() else { return };
        match tool.as_str() {
            "settings" => open(&tools, &app, FIRST_TAB),
            "contacts" => crate::contacts::open(&tools, &app),
            _ => open(&tools, &app, "keyboard"),
        }
    });
}
