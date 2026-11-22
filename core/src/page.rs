//! A mail's page drawn into pixels by Blitz, an HTML and CSS engine in Rust. It runs no script, and everything it
//! asks for is answered by `pictures` from the page itself: nothing leaves the machine.
//!
//! Blitz runs in the app's own process, where WebKit had one of its own. Its layout and painting recurse once for
//! every level of nesting, and a stack overflow ends the whole program, so a page is measured before anything is laid
//! out, and a page that is too deep, too large or too long is refused: the reader then shows it as plain text. Use a
//! page only on a thread with `STACK` of stack. The limits come from hostile mail run in Docker (docs/BLITZ.md).

pub mod pictures;

use anyrender::{PaintScene as _, render_to_buffer};
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::{BaseDocument, DocumentConfig, FontContext, NodeData, StyleThreading, local_name};
use blitz_html::HtmlDocument;
use blitz_traits::shell::{ColorScheme, Viewport};
use peniko::{Fill, color::palette::css::WHITE, kurbo::Rect};
use pictures::Pictures;
use std::sync::Arc;

/// The stack a thread needs to lay out and paint a page within the limits below: sixteen times what 2,051 levels
/// took. Only addresses are reserved, not memory.
pub const STACK: usize = 64 * 1024 * 1024;
/// Elements nested deeper than this are refused. Browsers stop at 512 too; on a stack of 1 MB, 515 overflowed.
pub const DEEPEST: usize = 512;
/// Past this many elements a page is refused: 59,000 table cells took 353 MB.
pub const MOST_ELEMENTS: usize = 40_000;
/// Past this much text, in bytes, a page is refused: 3.7 MB of text took 544 MB and seconds to lay out.
pub const MOST_TEXT: usize = 1_000_000;
/// Rows painted at once: the renderer counts pixels in 16 bits.
pub const TALLEST_BAND: u32 = 16_384;
/// A page wider than its pane is drawn smaller to fit, as on a phone, but never below half.
const LEAST_FIT: f64 = 0.5;
/// A base no address joins to anything real, and which `pictures` refuses. Blitz's own is a `data:` address, and a
/// relative address that cannot be joined to its base makes it panic.
const BASE: &str = "https://mail.invalid/";

/// Fonts, found once per thread and shared by every page made on it.
pub struct Fonts(FontContext);

impl Fonts {
    /// The system's fonts. Finding them takes from 0.1 to 0.6 s the first time.
    #[must_use]
    pub fn system() -> Self {
        use parley::fontique::{Blob, Collection, CollectionOptions, SourceCache};
        let mut context = FontContext {
            source_cache: SourceCache::new_shared(),
            collection: Collection::new(CollectionOptions { shared: true, system_fonts: true }),
        };
        context.collection.register_fonts(Blob::new(Arc::new(blitz_dom::BULLET_FONT) as _), None);
        Self(context)
    }

    /// One font for every family: for a system with none, as in the containers of the tests.
    #[must_use]
    pub fn only(font: &[u8]) -> Self {
        Self(blitz_dom::build_single_font_ctx(font))
    }
}

/// The pane a page is drawn for, in the screen's pixels, and how many of them a CSS pixel takes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sight {
    pub width: u32,
    pub height: u32,
    pub scale: f64,
}

/// Why a page was not made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    TooDeep,
    TooManyElements,
    TooMuchText,
}

/// What the pointer shows over a point of the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cursor {
    Default,
    Pointer,
    Text,
}

/// How deep the page nests, how many elements it has and how much text, walked without recursion.
fn measure(document: &BaseDocument) -> (usize, usize, usize) {
    let (mut deepest, mut elements, mut text) = (0, 0, 0usize);
    let mut stack = vec![(document.root_node().id, 0usize)];
    while let Some((id, depth)) = stack.pop() {
        let Some(node) = document.get_node(id) else { continue };
        deepest = deepest.max(depth);
        match &node.data {
            NodeData::Element(_) => elements += 1,
            NodeData::Text(data) => text = text.saturating_add(data.content.len()),
            _ => {}
        }
        stack.extend(node.children.iter().map(|child| (*child, depth + 1)));
    }
    (deepest, elements, text)
}

fn check(document: &BaseDocument) -> Result<(), Refused> {
    let (deepest, elements, text) = measure(document);
    // `html` and `body` are two levels the mail does not write.
    if deepest > DEEPEST + 2 {
        return Err(Refused::TooDeep);
    }
    if elements > MOST_ELEMENTS {
        return Err(Refused::TooManyElements);
    }
    if text > MOST_TEXT {
        return Err(Refused::TooMuchText);
    }
    Ok(())
}

/// A count of pixels from a length in them: rounded up, never negative, and within `most`.
fn whole(value: f64, most: u32) -> u32 {
    if !value.is_finite() || value <= 0.0 {
        return 0;
    }
    #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "clamped to 0..=most just before")]
    let count = value.ceil().min(f64::from(most)) as u32;
    count
}

/// A length of the page as Blitz takes it.
#[expect(clippy::cast_possible_truncation, reason = "a point on a page, far inside an f32")]
const fn narrow(value: f64) -> f32 {
    value as f32
}

pub struct Page {
    document: HtmlDocument,
    pictures: Arc<Pictures>,
    sight: Sight,
    /// Screen pixels to a CSS pixel after fitting: `sight.scale`, or less for a page wider than its pane.
    scale: f64,
}

impl Page {
    /// Parses `html` and lays it out for `sight`, unless it is past the limits. Nothing is laid out before it is
    /// measured: parsing does not recurse, laying out does.
    pub fn open(html: &str, fonts: &Fonts, sight: Sight) -> Result<Self, Refused> {
        let pictures = Arc::new(Pictures::default());
        let document = HtmlDocument::from_html(
            html,
            DocumentConfig {
                viewport: Some(viewport(sight, sight.scale)),
                base_url: Some(BASE.to_string()),
                net_provider: Some(pictures.clone()),
                font_ctx: Some(fonts.0.clone()),
                style_threading: StyleThreading::Sequential,
                ..DocumentConfig::default()
            },
        );
        check(&document)?;
        let mut page = Self { document, pictures, sight, scale: sight.scale };
        page.lay_out();
        Ok(page)
    }

    /// Lays the page out at the pane's scale, and again smaller when it is wider than the pane.
    fn lay_out(&mut self) {
        self.scale = self.sight.scale;
        self.resolve();
        let have = f64::from(self.sight.width) / self.sight.scale;
        let root = self.document.root_element();
        let need = f64::from(root.scroll_width()).max(f64::from(root.final_layout().size.width));
        if have > 0.0 && need > have + 0.5 {
            self.scale = self.sight.scale * (have / need).max(LEAST_FIT);
            self.resolve();
        }
    }

    fn resolve(&mut self) {
        self.document.set_viewport(viewport(self.sight, self.scale));
        self.document.resolve(0.0);
        // Pictures named in CSS are asked for while styles resolve, and arrive for the next pass.
        self.document.resolve(0.0);
    }

    /// The pane changed: laid out again only if its width or scale did.
    pub fn resize(&mut self, sight: Sight) {
        let width_changed = sight.width != self.sight.width || (sight.scale - self.sight.scale).abs() > f64::EPSILON;
        self.sight = sight;
        if width_changed {
            self.lay_out();
        } else {
            self.document.set_viewport(viewport(sight, self.scale));
        }
    }

    pub const fn sight(&self) -> Sight {
        self.sight
    }

    /// The page's height in screen pixels.
    #[must_use]
    pub fn height(&self) -> u32 {
        let root = self.document.root_element();
        let css = f64::from(root.final_layout().size.height).max(f64::from(root.scroll_height()));
        whole(css * self.scale, u32::MAX)
    }

    /// `rows` rows of the page from `top` down, as RGBA, the pane's width across, on white.
    #[must_use]
    pub fn paint(&mut self, top: u32, rows: u32) -> Vec<u8> {
        let (width, rows) = (self.sight.width.max(1), rows.clamp(1, TALLEST_BAND));
        let scale = self.scale;
        let document: &mut BaseDocument = &mut self.document;
        // Blitz's own offsets move the page's origin; a band further down is the page scrolled to it. Hit tests do not
        // read the scroll, and it goes back to the top once painted.
        document.set_viewport_scroll(blitz_dom::Point { x: 0.0, y: f64::from(top) / scale });
        let rgba = render_to_buffer::<VelloCpuImageRenderer, _>(
            |scene| {
                let all = Rect::new(0.0, 0.0, f64::from(width), f64::from(rows));
                scene.fill(Fill::NonZero, peniko::kurbo::Affine::IDENTITY, WHITE, None, &all);
                blitz_paint::paint_scene(scene, document, scale, width, rows, 0, 0);
            },
            width,
            rows,
        );
        document.set_viewport_scroll(blitz_dom::Point { x: 0.0, y: 0.0 });
        rgba
    }

    /// A point in screen pixels from the page's top left corner, as Blitz measures it.
    fn at(&self, x: f64, y: f64) -> (f32, f32) {
        (narrow(x / self.scale), narrow(y / self.scale))
    }

    /// The address of the link under a point, as the mail wrote it, if it is a whole address.
    #[must_use]
    pub fn link_at(&self, x: f64, y: f64) -> Option<String> {
        let (x, y) = self.at(x, y);
        let mut id = Some(self.document.hit(x, y)?.node_id);
        while let Some(node) = id.and_then(|id| self.document.get_node(id)) {
            if node.data.is_element_with_tag_name(&local_name!("a"))
                && let Some(address) = node.attr(local_name!("href"))
            {
                return url::Url::parse(address.trim()).ok().map(|url| url.to_string());
            }
            id = node.parent;
        }
        None
    }

    /// What the pointer shows over a point: a hand over a link, a caret over text.
    pub fn cursor_at(&mut self, x: f64, y: f64) -> Cursor {
        if self.link_at(x, y).is_some() {
            return Cursor::Pointer;
        }
        let (x, y) = self.at(x, y);
        if self.document.find_text_position(x, y).is_some() { Cursor::Text } else { Cursor::Default }
    }

    /// A press: a selection starts where it lands, and the old one goes.
    pub fn press(&mut self, x: f64, y: f64) {
        let (x, y) = self.at(x, y);
        self.document.clear_text_selection();
        if let Some((node, offset)) = self.document.find_text_position(x, y) {
            self.document.set_text_selection(node, offset, node, offset);
        }
    }

    /// The pointer moved while pressed: the selection reaches to it. Whether it changed.
    pub fn drag(&mut self, x: f64, y: f64) -> bool {
        let (x, y) = self.at(x, y);
        self.document.extend_text_selection_to_point(x, y)
    }

    /// The selected text, if any is selected.
    #[must_use]
    pub fn selected_text(&self) -> Option<String> {
        self.document.has_text_selection().then(|| self.document.get_selected_text()).flatten()
    }

    pub fn clear_selection(&mut self) {
        self.document.clear_text_selection();
    }

    /// Pictures the page carried and was given, and requests answered with nothing.
    #[must_use]
    pub fn pictures(&self) -> (usize, usize) {
        (self.pictures.served(), self.pictures.refused())
    }
}

fn viewport(sight: Sight, scale: f64) -> Viewport {
    Viewport::new(sight.width.max(1), sight.height.max(1), narrow(scale), ColorScheme::Light)
}

#[cfg(test)]
mod tests;
