//! Links opened with Phosphor (telnet://, rlogin://, ...) reach a macOS app
//! as a "get URL" Apple Event rather than as a command-line argument; the
//! schemes are claimed in the bundle's Info.plist.

use objc2::rc::Retained;
use objc2::runtime::NSObject;
use objc2::{AllocAnyThread, DefinedClass, define_class, msg_send, sel};
use objc2_foundation::{NSAppleEventDescriptor, NSAppleEventManager};

/// `kInternetEventClass` and `kAEGetURL` are both 'GURL'.
const GURL: u32 = u32::from_be_bytes(*b"GURL");
/// `keyDirectObject`: the URL itself.
const DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----");

struct Ivars {
    on_url: Box<dyn Fn(String)>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PhosphorURLHandler"]
    #[ivars = Ivars]
    struct UrlHandler;

    impl UrlHandler {
        #[unsafe(method(handleGetURL:withReplyEvent:))]
        fn handle_get_url(&self, event: &NSAppleEventDescriptor, _reply: &NSAppleEventDescriptor) {
            // Called by name: the typed binding needs objc2-core-services.
            let param: Option<Retained<NSAppleEventDescriptor>> =
                unsafe { msg_send![event, paramDescriptorForKeyword: DIRECT_OBJECT] };
            if let Some(url) = param.and_then(|d| d.stringValue()) {
                (self.ivars().on_url)(url.to_string());
            }
        }
    }
);

/// Pass each link opened with the app to `on_url`. Call before the event
/// loop runs, so the link that launched the app is not missed.
pub fn install(on_url: impl Fn(String) + 'static) {
    let handler = UrlHandler::alloc().set_ivars(Ivars { on_url: Box::new(on_url) });
    let handler: Retained<UrlHandler> = unsafe { msg_send![super(handler), init] };
    let manager = NSAppleEventManager::sharedAppleEventManager();
    unsafe {
        let _: () = msg_send![
            &manager,
            setEventHandler: &*handler,
            andSelector: sel!(handleGetURL:withReplyEvent:),
            forEventClass: GURL,
            andEventID: GURL,
        ];
    }
    // The event manager does not retain its handlers, and this one is
    // needed for as long as the app runs.
    std::mem::forget(handler);
}
