//! Which of WebKit's own features a page of the app keeps. A mail, the editor and a printout are pages to draw once,
//! and Proton's verification page must not reach past its proxy. The switches are WebKit's private ones on macOS; the
//! other systems have what they need elsewhere (`webviews.rs`, `verify.rs`).

use wry::WebViewBuilder;

/// A mail is a page to draw once: no scripts, no canvas, no video, no history to go back through,
/// and nothing kept on disk. WebKit reads these before the page exists, so they cannot be changed on
/// a page already made. Drawing itself stays WebKit's own, on the GPU: the memory that saves is not
/// worth a page that can come late. The switches are not public: each is asked about first, and
/// without them a mail is still drawn.
#[cfg(target_os = "macos")]
pub fn keep_to_drawing(builder: WebViewBuilder<'_>) -> WebViewBuilder<'_> {
    // LinkPreconnect connects to a clicked link's address before the app has said no to going there: the long run
    // caught one reach a mail tracker around Tor that way (scripts/link-preconnect.py).
    const NOT_NEEDED: [&str; 7] = [
        "LinkPreconnect",
        "UseGPUProcessForCanvasRenderingEnabled",
        "UseGPUProcessForMediaEnabled",
        "UseGPUProcessForWebGLEnabled",
        "UsesBackForwardCache",
        "WebGPUEnabled",
        "CacheAPIEnabled",
    ];
    with_features_off(builder, &NOT_NEEDED)
}

/// For Proton's verification page, which runs its own scripts. WebRTC can send UDP to any server past the proxy,
/// and with it the real address Tor hides, so a peer connection cannot be made at all.
#[cfg(target_os = "macos")]
pub fn without_peer_connections(builder: WebViewBuilder<'_>) -> WebViewBuilder<'_> {
    with_features_off(builder, &PEER_CONNECTIONS)
}

#[cfg(target_os = "macos")]
const PEER_CONNECTIONS: [&str; 3] =
    ["PeerConnectionEnabled", "WebRTCPlatformTCPSocketsEnabled", "WebRTCPlatformUDPSocketsEnabled"];

/// WebKitGTK has WebRTC off unless asked; WebView2 gets it from its environment's arguments, in `verify.rs`.
#[cfg(not(target_os = "macos"))]
pub const fn without_peer_connections(builder: WebViewBuilder<'_>) -> WebViewBuilder<'_> {
    builder
}

/// A configuration with nothing kept on disk and the named features of WebKit off. The switches are private: each
/// is asked about first, and one that is missing is passed over.
#[cfg(target_os = "macos")]
fn with_features_off<'a>(builder: WebViewBuilder<'a>, off: &[&str]) -> WebViewBuilder<'a> {
    use objc2::{
        MainThreadMarker, msg_send,
        rc::Retained,
        runtime::{AnyClass, AnyObject, Bool},
        sel,
    };
    use objc2_foundation::NSArray;
    use objc2_web_kit::{WKWebViewConfiguration, WKWebsiteDataStore};
    use wry::WebViewBuilderExtMacos;

    let Some(marker) = MainThreadMarker::new() else { return builder };
    let Some(kind) = AnyClass::get(c"WKPreferences") else { return builder };
    // Safety: each private selector is asked about before it is sent, and the rest is public.
    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    unsafe {
        let configuration = WKWebViewConfiguration::new(marker);
        configuration.setWebsiteDataStore(&WKWebsiteDataStore::nonPersistentDataStore(marker));
        let preferences = configuration.preferences();
        let listed: Bool = msg_send![kind, respondsToSelector: sel!(_features)];
        let settable: Bool = msg_send![&*preferences, respondsToSelector: sel!(_setEnabled:forFeature:)];
        // Private calls may answer nothing in another version of WebKit: that is a feature left as it is, never a
        // panic, which a result that cannot be nothing would be.
        let features: Option<Retained<NSArray<AnyObject>>> =
            if listed.as_bool() && settable.as_bool() { msg_send![kind, _features] } else { None };
        for feature in features.iter().flat_map(|features| features.iter()) {
            if let Some(key) = key_of(&feature).filter(|key| off.contains(&key.to_string().as_str())) {
                let _: () = msg_send![&*preferences, _setEnabled: false, forFeature: &*feature];
                log::debug!("a WebKit feature turned off: {key}");
            }
        }
        builder.with_webview_configuration(configuration)
    }
}

/// A feature's name, when it has one: `_WKFeature` is a private class, which another version of WebKit may change, so
/// it is asked whether it answers to `key` before it is sent, and what comes back must be a string.
#[cfg(target_os = "macos")]
fn key_of(feature: &objc2::runtime::AnyObject) -> Option<objc2::rc::Retained<objc2_foundation::NSString>> {
    use objc2::{
        msg_send,
        rc::Retained,
        runtime::{AnyObject, Bool},
        sel,
    };
    // Safety: `key` is sent only to an object that says it answers to it, and nothing that comes back is used before
    // it is found to be a string.
    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    let key: Option<Retained<AnyObject>> = unsafe {
        let answers: Bool = msg_send![feature, respondsToSelector: sel!(key)];
        if answers.as_bool() { msg_send![feature, key] } else { None }
    };
    key.and_then(|key| key.downcast::<objc2_foundation::NSString>().ok())
}

#[cfg(not(target_os = "macos"))]
pub const fn keep_to_drawing(builder: WebViewBuilder<'_>) -> WebViewBuilder<'_> {
    builder
}

#[cfg(test)]
mod tests;
