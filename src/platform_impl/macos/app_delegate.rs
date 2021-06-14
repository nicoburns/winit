use objc2::foundation::NSObject;
use objc2::rc::{Id, Shared};
use objc2::runtime::{Object, Sel};
use objc2::{class, sel};
use objc2::{declare_class, msg_send, msg_send_id, ClassType};

use crate::event::{Event, MacOS, PlatformSpecific};
use crate::platform_impl::platform::{app_state::AppState, event::EventWrapper};

use super::appkit::NSApplicationActivationPolicy;

/// Apple constants
#[allow(non_upper_case_globals)]
pub const kInternetEventClass: u32 = 0x4755524c;
#[allow(non_upper_case_globals)]
pub const kAEGetURL: u32 = 0x4755524c;
#[allow(non_upper_case_globals)]
pub const keyDirectObject: u32 = 0x2d2d2d2d;

declare_class!(
    #[derive(Debug)]
    pub(super) struct ApplicationDelegate {
        activation_policy: NSApplicationActivationPolicy,
        default_menu: bool,
        activate_ignoring_other_apps: bool,
    }

    unsafe impl ClassType for ApplicationDelegate {
        type Super = NSObject;
        const NAME: &'static str = "WinitApplicationDelegate";
    }

    unsafe impl ApplicationDelegate {
        #[sel(initWithActivationPolicy:defaultMenu:activateIgnoringOtherApps:)]
        fn init(
            &mut self,
            activation_policy: NSApplicationActivationPolicy,
            default_menu: bool,
            activate_ignoring_other_apps: bool,
        ) -> Option<&mut Self> {
            let this: Option<&mut Self> = unsafe { msg_send![super(self), init] };
            this.map(|this| {
                *this.activation_policy = activation_policy;
                *this.default_menu = default_menu;
                *this.activate_ignoring_other_apps = activate_ignoring_other_apps;
                this
            })
        }

        #[sel(applicationDidFinishLaunching:)]
        fn did_finish_launching(&self, _sender: *const Object) {
            trace_scope!("applicationDidFinishLaunching:");
            AppState::launched(
                *self.activation_policy,
                *self.default_menu,
                *self.activate_ignoring_other_apps,
            );
        }

        #[sel(applicationWillFinishLaunching:)]
        fn will_finish_launching(&self, _sender: *const Object) {
            trace!("Triggered `applicationWillFinishLaunching`");
            unsafe {
                let event_manager = class!(NSAppleEventManager);
                let shared_manager: *mut Object = msg_send![event_manager, sharedAppleEventManager];
                let () = msg_send![shared_manager,
                    setEventHandler: self,
                    andSelector: sel!(handleEvent:withReplyEvent:)
                    forEventClass: kInternetEventClass
                    andEventID: kAEGetURL
                ];
            }
            trace!("Completed `applicationWillFinishLaunching`");
        }

        #[sel(handleEvent:withReplyEvent:)]
        fn handle_url(&self, _cmd: Sel, event: *mut Object, _reply: u64) {
            if let Some(string) = parse_url(event) {
                AppState::queue_event(EventWrapper::StaticEvent(Event::PlatformSpecific(
                    PlatformSpecific::MacOS(MacOS::ReceivedUrl(string)),
                )));
            }
        }

        #[sel(applicationWillTerminate:)]
        fn will_terminate(&self, _sender: *const Object) {
            trace_scope!("applicationWillTerminate:");
            // TODO: Notify every window that it will be destroyed, like done in iOS?
            AppState::exit();
        }
    }
);

impl ApplicationDelegate {
    pub(super) fn new(
        activation_policy: NSApplicationActivationPolicy,
        default_menu: bool,
        activate_ignoring_other_apps: bool,
    ) -> Id<Self, Shared> {
        unsafe {
            msg_send_id![
                msg_send_id![Self::class(), alloc],
                initWithActivationPolicy: activation_policy,
                defaultMenu: default_menu,
                activateIgnoringOtherApps: activate_ignoring_other_apps,
            ]
        }
    }
}

fn parse_url(event: *mut Object) -> Option<String> {
    unsafe {
        let class: u32 = msg_send![event, eventClass];
        let id: u32 = msg_send![event, eventID];
        if class != kInternetEventClass || id != kAEGetURL {
            return None;
        }
        let subevent: *mut Object =
            msg_send![event, paramDescriptorForKeyword: keyDirectObject];
        let nsstring: *mut Object = msg_send![subevent, stringValue];
        let cstr: *const i8 = msg_send![nsstring, UTF8String];
        if !cstr.is_null() {
            Some(
                std::ffi::CStr::from_ptr(cstr)
                    .to_string_lossy()
                    .into_owned(),
            )
        } else {
            None
        }
    }
}
