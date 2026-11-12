//! Files opened from Finder ("Open With", a double click, a drop on the Dock icon).
//!
//! macOS does not put them on the command line: it sends the running app an "open documents"
//! Apple event, at launch as well as later. Without a handler AppKit hands the event to its
//! document controller, which knows no document classes and shows "BGPaint cannot open files in
//! the … format". The handler is installed when the app is about to finish launching, the moment
//! Apple documents for overriding AppKit's own handlers and before the launch event is delivered.
//! (winit is the application delegate, so the `application:openURLs:` route is taken.)
//!
//! The image types BGPaint offers to open are declared in `packaging/macos/Info.plist`.
#![allow(unsafe_code)] // Objective-C: a class of our own and Apple event handlers.

use crate::files::DialogResult;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::NSApplicationWillFinishLaunchingNotification;
use objc2_foundation::{NSAppleEventDescriptor, NSAppleEventManager, NSNotification, NSNotificationCenter};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::mpsc::Sender;

const fn four_cc(code: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*code)
}
const K_CORE_EVENT_CLASS: u32 = four_cc(b"aevt");
const K_AE_OPEN_DOCUMENTS: u32 = four_cc(b"odoc");
const KEY_DIRECT_OBJECT: u32 = four_cc(b"----");

/// Where the opened files go: kept until the app window exists, then sent to the app.
static SINK: Mutex<Sink> = Mutex::new(Sink { pending: Vec::new(), app: None });

struct Sink {
    pending: Vec<PathBuf>,
    app: Option<(egui::Context, Sender<DialogResult>)>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements and this class does not implement Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BGPaintOpenDocuments"]
    struct OpenDocuments;

    impl OpenDocuments {
        #[unsafe(method(applicationWillFinishLaunching:))]
        fn will_finish_launching(&self, _notification: &NSNotification) {
            let manager = NSAppleEventManager::sharedAppleEventManager();
            // SAFETY: `self` stays alive for the life of the process (see `install`) and
            // implements the selector with the signature the manager calls.
            let () = unsafe {
                msg_send![&manager, setEventHandler: self as &AnyObject,
                    andSelector: sel!(handleOpenDocuments:withReplyEvent:),
                    forEventClass: K_CORE_EVENT_CLASS,
                    andEventID: K_AE_OPEN_DOCUMENTS]
            };
        }

        #[unsafe(method(handleOpenDocuments:withReplyEvent:))]
        fn handle_open_documents(&self, event: &NSAppleEventDescriptor, _reply: &NSAppleEventDescriptor) {
            let paths = file_paths(event);
            if !paths.is_empty() {
                deliver(paths);
            }
        }
    }
);

/// Starts listening for files opened from Finder. Call once, on the main thread, before the
/// event loop runs.
pub fn install() {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let observer: Retained<OpenDocuments> = {
        let this = OpenDocuments::alloc(mtm).set_ivars(());
        // SAFETY: NSObject's designated initializer.
        unsafe { msg_send![super(this), init] }
    };
    // SAFETY: the observer implements the selector, taking the notification; reading the
    // AppKit constant is sound (it is initialized before `main`).
    unsafe {
        NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
            &observer,
            sel!(applicationWillFinishLaunching:),
            Some(NSApplicationWillFinishLaunchingNotification),
            None,
        );
    }
    // Neither the notification center nor the Apple event manager retains it.
    std::mem::forget(observer);
}

/// Hands the files opened so far (at launch) to the caller, and the later ones to `tx`, as if
/// another launch had passed them (see [`crate::single_instance`]).
pub fn connect(ctx: &egui::Context, tx: Sender<DialogResult>) -> Vec<PathBuf> {
    let mut sink = SINK.lock().unwrap_or_else(|e| e.into_inner());
    sink.app = Some((ctx.clone(), tx));
    std::mem::take(&mut sink.pending)
}

fn deliver(paths: Vec<PathBuf>) {
    let mut sink = SINK.lock().unwrap_or_else(|e| e.into_inner());
    match &sink.app {
        Some((ctx, tx)) => {
            if tx.send(DialogResult::Launched(paths)).is_ok() {
                ctx.request_repaint();
            }
        }
        None => sink.pending.extend(paths),
    }
}

/// The files in an "open documents" event: a list of file references in its direct object.
fn file_paths(event: &NSAppleEventDescriptor) -> Vec<PathBuf> {
    // SAFETY: `paramDescriptorForKeyword:` takes an AEKeyword (a four-character code).
    let list: Option<Retained<NSAppleEventDescriptor>> = unsafe { msg_send![event, paramDescriptorForKeyword: KEY_DIRECT_OBJECT] };
    let Some(list) = list else { return Vec::new() };
    let path = |d: &NSAppleEventDescriptor| d.fileURLValue().and_then(|url| url.to_file_path());
    match list.numberOfItems() {
        // A single reference instead of a list.
        0 => path(&list).into_iter().collect(),
        // Apple event lists count from 1.
        n => (1..=n).filter_map(|i| list.descriptorAtIndex(i)).filter_map(|d| path(&d)).collect(),
    }
}
