//! Images inside mail, fetched through Proton.

use super::{Attachment, encoded, Error, HashMap, html, IMAGE_CONCURRENCY, is_inline_image, Message, on_worker, Result, safety, Service, stream, StreamExt, to_data_url, Value};
use crate::file_kind::{self, FileKind};

const LOGO: &str = "logo";
const GRAVATAR: &str = "gravatar";
const IMAGE: &str = "image";
const THUMBNAIL: &str = "thumbnail";
const TRACKER: &str = "tracker";
const LOGO_PIXELS: u32 = 128;
const LARGEST_LOGO: usize = 64 * 1024;
const LONGEST_DOMAIN: usize = 253;
const KEPT_LOGOS: usize = 128;

const DRAFTS: &str = "drafts";

impl Service {
    /// Asks Proton which company stands behind each image, without fetching the images. An answer
    /// is kept with the mail, and a mail full of pictures asks only once.
    pub async fn image_trackers(&self, urls: Vec<String>) -> HashMap<String, String> {
        let lookups = urls.into_iter().map(|url| async move {
            let store = self.store();
            if let Some(known) = store.as_ref().and_then(|store| store.get::<Option<String>>(TRACKER, &url)) {
                return known.map(|tracker| (url, tracker));
            }
            let path = format!("/core/v4/images?Url={}&DryRun=1", encoded(&url));
            let (_, headers) = self.client.fetch(&path).await.ok()?;
            let tracker = headers.get("x-pm-tracker-provider").and_then(|value| value.to_str().ok()).filter(|name| !name.is_empty());
            let tracker = tracker.map(str::to_string);
            if let Some(store) = store {
                store.put(TRACKER, &url, &tracker);
            }
            tracker.map(|tracker| (url, tracker))
        });
        stream::iter(lookups).buffer_unordered(IMAGE_CONCURRENCY).filter_map(|r| async { r }).collect().await
    }

    /// A picture shown once is sealed with the mail it came in, so it opens without the network.
    pub(super) async fn remote_image(&self, url: &str) -> Result<String> {
        let store = self.store();
        if let Some(known) = store.as_ref().and_then(|store| store.get::<String>(IMAGE, url)) {
            return Ok(known);
        }
        let path = format!("/core/v4/images?Url={}&DryRun=0", encoded(url));
        let (data, headers) = self.client.fetch(&path).await?;
        let mime = headers.get("content-type").and_then(|v| v.to_str().ok()).unwrap_or_default();
        let mime = mime.split(';').next().unwrap_or_default().trim();
        if !is_inline_image(mime) || !safety::is_displayable_image(mime, &data) {
            return Err(Error::other(format!("Not an image we will show: {mime}")));
        }
        let picture = to_data_url(mime, &data);
        if let Some(store) = store {
            store.put(IMAGE, url, &picture);
        }
        Ok(picture)
    }

    /// The message with its remote images in place, all fetched through Proton's proxy.
    pub async fn with_remote_images(&self, safe_html: &str, urls: Vec<String>) -> String {
        let fetches = urls.into_iter().map(|url| async move {
            self.remote_image(&url).await.ok().map(|data| (url, data))
        });
        let images: HashMap<String, String> =
            stream::iter(fetches).buffer_unordered(IMAGE_CONCURRENCY).filter_map(|r| async { r }).collect().await;
        html::with_images(safe_html, &images)
    }

    /// A tiny picture for a file's row, as a data link, or nothing if the file should keep its icon.
    /// Each costs a download, so it is kept sealed with the mail, and a picture is asked for once.
    pub async fn thumbnail(&self, message_id: &str, file: &Attachment, address_id: &str) -> Option<String> {
        if file_kind::by_name(&file.name, &file.mime) != FileKind::Picture || file.size > file_kind::THUMBNAIL_MOST_BYTES {
            return None;
        }
        let key = format!("{message_id}/{}", file.id);
        let store = self.store();
        if let Some(known) = store.as_ref().and_then(|store| store.get::<String>(THUMBNAIL, &key)) {
            return Some(known);
        }
        let data = self.open_attachment(message_id, file, address_id).await.ok()?;
        let (name, mime) = (file.name.clone(), file.mime.clone());
        // On a worker: a panic on hostile input then costs this one picture, not the app.
        let small = on_worker(move || Ok(file_kind::thumbnail(&name, &mime, &data))).await.ok()??;
        let picture = to_data_url("image/png", &small);
        if let Some(store) = store {
            store.put(THUMBNAIL, &key, &picture);
        }
        Some(picture)
    }

    /// The page to show until the person asks for the pictures, if the account hides this mail's.
    /// Their own drafts and Proton's mail are exempt, as in `transformEmbedded.ts`.
    pub fn with_embedded_held_back(&self, message: &Message) -> Option<String> {
        let exempt = message.from_proton == Some(true) || message.meta.folder == DRAFTS;
        (message.html && !exempt && self.habits().hide_embedded).then(|| html::without_embedded(&message.safe_body)).flatten()
    }

    fn logo_domain(email: &str) -> Option<String> {
        let (_, domain) = email.rsplit_once('@')?;
        let domain = domain.to_ascii_lowercase();
        let clean = domain.contains('.')
            && domain.len() <= LONGEST_DOMAIN
            && domain.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'));
        clean.then_some(domain)
    }

    /// The logo of the company behind an address, from Proton itself: no third party learns who
    /// writes to this person. Nothing is asked if the account turned sender images off.
    pub async fn sender_logo(&self, email: &str, dark: bool) -> Option<String> {
        let domain = Self::logo_domain(email)?;
        let mode = if dark { "dark" } else { "light" };
        let key = format!("{mode}/{domain}");
        if let Some(known) = self.logos.lock().get(&key) {
            return known.clone();
        }
        if let Some(store) = self.store()
            && let Some(known) = store.get::<Option<String>>(LOGO, &key)
        {
            self.keep_logo(key, known.clone());
            return known;
        }
        let settings: Value = self.client.get("/mail/v4/settings").await.ok()?;
        if settings.pointer("/MailSettings/HideSenderImages").and_then(Value::as_u64) == Some(1) {
            return None;
        }
        let path = format!("/core/v4/images/logo?Domain={}&Size={LOGO_PIXELS}&Mode={mode}", encoded(&domain));
        let logo = match self.client.fetch(&path).await {
            Ok((data, headers)) => {
                let mime = headers.get("content-type").and_then(|value| value.to_str().ok()).unwrap_or_default();
                let mime = mime.split(';').next().unwrap_or_default().trim().to_string();
                (is_inline_image(&mime) && safety::is_displayable_image(&mime, &data) && data.len() <= LARGEST_LOGO)
                    .then(|| to_data_url(&mime, &data))
            }
            // Proton knows no logo for this name, which is worth remembering. A lost connection is not.
            Err(Error::Api { .. }) => None,
            Err(_) => return None,
        };
        if let Some(store) = self.store() {
            store.put(LOGO, &key, &logo);
        }
        self.keep_logo(key, logo.clone());
        logo
    }

    /// A person's picture from Gravatar, for a sender with no logo, only when the person turned it on. It goes
    /// through Proton's image proxy, so Gravatar never sees this computer's address; it does get the sender's address
    /// in coded form, which is why it is off unless asked for. An answer, a picture or none, is kept.
    pub async fn sender_gravatar(&self, email: &str) -> Option<String> {
        let url = gravatar_url(email)?;
        let key = format!("{GRAVATAR}/{url}");
        if let Some(known) = self.logos.lock().get(&key) {
            return known.clone();
        }
        if let Some(store) = self.store()
            && let Some(known) = store.get::<Option<String>>(GRAVATAR, &url)
        {
            self.keep_logo(key, known.clone());
            return known;
        }
        let path = format!("/core/v4/images?Url={}&DryRun=0", encoded(&url));
        let picture = match self.client.fetch(&path).await {
            Ok((data, headers)) => {
                let mime = headers.get("content-type").and_then(|value| value.to_str().ok()).unwrap_or_default();
                let mime = mime.split(';').next().unwrap_or_default().trim().to_string();
                (is_inline_image(&mime) && safety::is_displayable_image(&mime, &data) && data.len() <= LARGEST_LOGO)
                    .then(|| to_data_url(&mime, &data))
            }
            // Gravatar has no picture for this address, which is worth remembering. A lost connection is not.
            Err(Error::Api { .. }) => None,
            Err(_) => return None,
        };
        log::debug!("a Gravatar picture asked for: {}", if picture.is_some() { "found" } else { "none" });
        if let Some(store) = self.store() {
            store.put(GRAVATAR, &url, &picture);
        }
        self.keep_logo(key, picture.clone());
        picture
    }

    fn keep_logo(&self, key: String, logo: Option<String>) {
        let mut logos = self.logos.lock();
        if logos.len() >= KEPT_LOGOS {
            logos.clear();
        }
        logos.insert(key, logo);
    }
}

/// Gravatar's address for a person's picture: the SHA-256 of the address, trimmed and in small letters, as Gravatar
/// takes it, at the size of a logo, and a 404 rather than a made-up picture when there is none.
#[must_use]
pub fn gravatar_url(email: &str) -> Option<String> {
    use sha2::{Digest, Sha256};
    let email = email.trim().to_lowercase();
    let (local, domain) = email.split_once('@')?;
    if local.is_empty() || domain.is_empty() || email.chars().any(char::is_whitespace) {
        return None;
    }
    let hash = crate::hex(&Sha256::digest(email.as_bytes()));
    Some(format!("https://www.gravatar.com/avatar/{hash}?s={LOGO_PIXELS}&d=404"))
}
