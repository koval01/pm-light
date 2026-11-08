//! What reaches the app from outside it: a `mailto:` link and a click on its icon in the Dock. A link
//! is often the very thing that started the app, or may come while it is locked, so links wait here
//! until the mailbox is open: no letter shows above a locked window.

use crate::{actions, composer};
use parking_lot::Mutex;
use proton_core::{
    compose,
    mailto::{self, Mailto},
};
use slint::ComponentHandle;
use std::sync::atomic::{AtomicBool, Ordering};

static OPEN: AtomicBool = AtomicBool::new(false);
static WAITING: Mutex<Vec<Mailto>> = Mutex::new(Vec::new());

fn accept(link: &str) {
    let Some(link) = mailto::parse(link) else {
        log::info!("an argument that is no mailto link was left alone");
        return;
    };
    // Only that one came: its addresses and text are the person's.
    log::info!("a mailto link arrived");
    WAITING.lock().push(link);
    let _ = slint::invoke_from_event_loop(|| {
        show_window();
        deliver();
    });
}

fn show_window() {
    if let Some(app) = actions::main_window() {
        drop(app.show());
    }
}

fn deliver() {
    let Some(service) = actions::service().filter(|_| OPEN.load(Ordering::Relaxed)) else { return };
    let waiting = std::mem::take(&mut *WAITING.lock());
    for link in waiting {
        composer::start_prepared(&service, compose::compose_prepared(&link.to, &link.subject, &link.body));
    }
}

pub fn mailbox_open() {
    OPEN.store(true, Ordering::Relaxed);
    deliver();
}

pub fn mailbox_closed() {
    OPEN.store(false, Ordering::Relaxed);
}

/// Another start of the program, which came with these arguments and left.
pub fn second_start<'a>(arguments: impl Iterator<Item = &'a str>) {
    log::info!("a second start handed over");
    arguments.for_each(accept);
    let _ = slint::invoke_from_event_loop(show_window);
}

/// Windows and Linux hand a link to the program as an argument, on macOS it comes as an event.
pub fn listen() {
    std::env::args().skip(1).for_each(|argument| accept(&argument));
    system::listen();
}

#[cfg(target_os = "macos")]
mod system {
    use objc2::{
        MainThreadMarker,
        encode::Encode,
        ffi,
        runtime::{AnyClass, AnyObject, Bool, Imp, Sel},
        sel,
    };
    use objc2_app_kit::NSApplication;
    use objc2_foundation::{NSArray, NSURL};
    use std::{ffi::CString, mem::transmute};

    unsafe extern "C-unwind" fn open_urls(_: *mut AnyObject, _: Sel, _: *mut AnyObject, urls: *mut NSArray<NSURL>) {
        // Safety: AppKit passes an array that lives for the length of this call.
        let urls = unsafe { urls.as_ref() }.map_or_else(Vec::new, NSArray::to_vec);
        for text in urls.iter().filter_map(|url| url.absoluteString()) {
            super::accept(&text.to_string());
        }
    }

    unsafe extern "C-unwind" fn reopen(_: *mut AnyObject, _: Sel, _: *mut AnyObject, _: Bool) -> Bool {
        let _ = slint::invoke_from_event_loop(super::show_window);
        Bool::YES
    }

    fn add(class: *const AnyClass, selector: Sel, method: Imp, types: &str) {
        let Ok(types) = CString::new(types) else { return };
        // Safety: the types match the function, and the name it is added under is the one AppKit calls.
        if !unsafe { ffi::class_addMethod(class.cast_mut(), selector, method, types.as_ptr()) }.as_bool() {
            log::warn!("the application delegate already answers this call");
        }
    }

    /// `winit` panics if the delegate is not its own, so these two are added to its class. It has
    /// neither of them, and it is what AppKit gets a link and a click on the icon from.
    pub fn listen() {
        let Some(marker) = MainThreadMarker::new() else { return };
        let Some(delegate) = NSApplication::sharedApplication(marker).delegate() else {
            log::warn!("the application has no delegate yet");
            return;
        };
        let delegate: &AnyObject = (*delegate).as_ref();
        let class: *const AnyClass = delegate.class();
        // Safety: an `Imp` is only a pointer, and the runtime calls it with the types given below.
        let (open_urls, reopen) = unsafe {
            (
                transmute::<unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, *mut NSArray<NSURL>), Imp>(open_urls),
                transmute::<unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, Bool) -> Bool, Imp>(reopen),
            )
        };
        add(class, sel!(application:openURLs:), open_urls, "v@:@@");
        let answer = Bool::ENCODING;
        add(class, sel!(applicationShouldHandleReopen:hasVisibleWindows:), reopen, &format!("{answer}@:@{answer}"));
    }
}

#[cfg(not(target_os = "macos"))]
mod system {
    pub const fn listen() {}
}
