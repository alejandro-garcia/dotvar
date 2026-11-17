//! Narrow macOS bridge for the termination decision missing from pinned GPUI.
//!
//! Install once on the main thread, after GPUI creates its application delegate
//! and before enabling editing. A failed installation must abort startup. The
//! callback must enqueue the existing close flow; it must not synchronously quit.
//! Call `allow_quit` only after all work is resolved and the last window closes.
//!
//! This adds an absent delegate method. It neither replaces the delegate nor
//! swizzles an existing implementation. GPUI's `applicationWillTerminate:` and
//! cleanup remain intact.

#![allow(unsafe_code)]

#[cfg(target_os = "macos")]
// The macos-quit feature also runs policy tests without the native app module.
#[cfg_attr(not(feature = "native"), allow(dead_code))]
mod macos {
    use crate::i18n::tr;
    use objc2::{
        Encode, MainThreadMarker,
        ffi::class_addMethod,
        runtime::{AnyClass, AnyObject, Imp, Sel},
        sel,
    };
    use objc2_app_kit::{NSApplication, NSApplicationTerminateReply};
    use std::{
        cell::RefCell,
        ffi::CString,
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
    };

    #[derive(Default)]
    struct QuitGuard {
        installed: bool,
        allowed: bool,
        callback_active: bool,
        delegate: usize,
        callback: Option<Rc<dyn Fn()>>,
    }

    thread_local! {
        static QUIT_GUARD: RefCell<QuitGuard> = RefCell::new(QuitGuard::default());
    }

    pub fn install_quit_guard(callback: impl Fn() + 'static) -> Result<(), String> {
        let main = MainThreadMarker::new().ok_or(tr!("quit_guard.main_thread"))?;
        let app = NSApplication::sharedApplication(main);
        let delegate = app.delegate().ok_or(tr!("quit_guard.no_delegate"))?;
        let delegate_object = AsRef::<AnyObject>::as_ref(&*delegate);
        let class =
            AnyClass::get(c"GPUIApplicationDelegate").ok_or(tr!("quit_guard.missing_class"))?;
        if !std::ptr::eq(delegate_object.class(), class) {
            return Err(tr!("quit_guard.unexpected_delegate").into());
        }

        let selector = sel!(applicationShouldTerminate:);
        // Includes inherited methods: do not silently override another policy.
        if class.instance_method(selector).is_some() {
            return Err(tr!("quit_guard.existing_policy").into());
        }
        let receiver = std::ptr::from_ref(delegate_object) as usize;
        // The return encoding follows AppKit's NSUInteger ABI on the target.
        // As in objc2's ClassBuilder, class_addMethod copies this encoding.
        let encoding = CString::new(format!("{}@:@", NSApplicationTerminateReply::ENCODING))
            .map_err(|_| tr!("quit_guard.invalid_signature"))?;

        QUIT_GUARD.with(|state| -> Result<(), String> {
            let mut state = state.try_borrow_mut().map_err(|_| tr!("quit_guard.busy"))?;
            if state.installed {
                return Err(tr!("quit_guard.already_installed").into());
            }

            // SAFETY: this is the exact GPUI delegate class, checked above; the
            // selector is absent. The typed implementation has the documented
            // -applicationShouldTerminate: ABI: self, _cmd, application object,
            // returning NSApplicationTerminateReply (NSUInteger). IMP erases
            // that signature only for the Objective-C runtime registration.
            let implementation: Imp = unsafe {
                std::mem::transmute::<
                    extern "C-unwind" fn(
                        &AnyObject,
                        Sel,
                        *mut AnyObject,
                    ) -> NSApplicationTerminateReply,
                    Imp,
                >(should_terminate)
            };
            // SAFETY: class_addMethod only adds an absent method, and reports
            // failure if another installation won a race. `class` is immortal;
            // the function pointer is static and the runtime copies its types.
            let added = unsafe {
                class_addMethod(
                    std::ptr::from_ref(class).cast_mut(),
                    selector,
                    implementation,
                    encoding.as_ptr(),
                )
            };
            if !added.as_bool() {
                return Err(tr!("quit_guard.install_failed").into());
            }
            *state = QuitGuard {
                installed: true,
                delegate: receiver,
                callback: Some(Rc::new(callback)),
                ..QuitGuard::default()
            };
            Ok(())
        })?;
        // Refresh cached optional delegate capabilities: assigning an identical
        // delegate alone may be a no-op. We are on the main thread, keep this
        // Retained alive, and do not yield to the run loop between setters.
        // The final delegate is the exact original object, with all GPUI methods.
        app.setDelegate(None);
        app.setDelegate(Some(&delegate));
        Ok(())
    }

    /// Opens the final termination gate. Calls from other threads do nothing.
    pub fn allow_quit() {
        if MainThreadMarker::new().is_none() {
            return;
        }
        QUIT_GUARD.with(|state| {
            if let Ok(mut state) = state.try_borrow_mut()
                && state.installed
            {
                state.allowed = true;
            }
        });
    }

    /// QA only: keep the ordinary NSWindow type while raising its level above
    /// other apps used during automation. This does not affect normal launches.
    #[cfg(feature = "native")]
    pub fn keep_qa_window_visible() {
        if let Some(main) = MainThreadMarker::new() {
            let app = NSApplication::sharedApplication(main);
            for window in app.windows().iter() {
                window.setLevel(objc2_app_kit::NSFloatingWindowLevel);
            }
        }
    }

    extern "C-unwind" fn should_terminate(
        receiver: &AnyObject,
        _: Sel,
        _: *mut AnyObject,
    ) -> NSApplicationTerminateReply {
        if MainThreadMarker::new().is_none() {
            return NSApplicationTerminateReply::TerminateCancel;
        }
        QUIT_GUARD.with(|state| decide(state, std::ptr::from_ref(receiver) as usize))
    }

    fn decide(state: &RefCell<QuitGuard>, receiver: usize) -> NSApplicationTerminateReply {
        let callback = {
            let Ok(mut guard) = state.try_borrow_mut() else {
                return NSApplicationTerminateReply::TerminateCancel;
            };
            if !guard.installed || guard.delegate != receiver || guard.callback_active {
                return NSApplicationTerminateReply::TerminateCancel;
            }
            if guard.allowed {
                return NSApplicationTerminateReply::TerminateNow;
            }
            let Some(callback) = guard.callback.clone() else {
                return NSApplicationTerminateReply::TerminateCancel;
            };
            guard.callback_active = true;
            callback
        };

        // The callback may update GPUI, open a prompt, or re-enter terminate:.
        // It runs without holding the state borrow. Never let a Rust panic
        // escape this delegate method or accidentally open the gate afterwards.
        let panicked = catch_unwind(AssertUnwindSafe(|| callback())).is_err();
        if let Ok(mut guard) = state.try_borrow_mut() {
            guard.callback_active = false;
            if panicked {
                guard.allowed = false;
            }
        }
        NSApplicationTerminateReply::TerminateCancel
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::cell::Cell;

        #[test]
        fn native_quit_guard_requires_explicit_permission_and_matching_delegate() {
            let calls = Rc::new(Cell::new(0));
            let count = calls.clone();
            let state = RefCell::new(QuitGuard {
                installed: true,
                delegate: 7,
                callback: Some(Rc::new(move || count.set(count.get() + 1))),
                ..QuitGuard::default()
            });
            assert_eq!(
                decide(&state, 7),
                NSApplicationTerminateReply::TerminateCancel
            );
            assert_eq!(calls.get(), 1);
            state.borrow_mut().allowed = true;
            assert_eq!(
                decide(&state, 8),
                NSApplicationTerminateReply::TerminateCancel
            );
            assert_eq!(decide(&state, 7), NSApplicationTerminateReply::TerminateNow);
            assert_eq!(calls.get(), 1);
        }

        #[test]
        fn native_quit_guard_cancels_reentrancy_without_invoking_callback_twice() {
            let state = Rc::new(RefCell::new(QuitGuard {
                installed: true,
                delegate: 7,
                ..QuitGuard::default()
            }));
            let weak = Rc::downgrade(&state);
            let calls = Rc::new(Cell::new(0));
            let count = calls.clone();
            state.borrow_mut().callback = Some(Rc::new(move || {
                count.set(count.get() + 1);
                let state = weak.upgrade().unwrap();
                state.borrow_mut().allowed = true;
                assert_eq!(
                    decide(&state, 7),
                    NSApplicationTerminateReply::TerminateCancel
                );
            }));
            assert_eq!(
                decide(&state, 7),
                NSApplicationTerminateReply::TerminateCancel
            );
            assert_eq!(calls.get(), 1);
            assert_eq!(decide(&state, 7), NSApplicationTerminateReply::TerminateNow);
        }

        #[test]
        fn native_quit_guard_keeps_the_gate_closed_if_the_callback_panics() {
            let state = Rc::new(RefCell::new(QuitGuard {
                installed: true,
                delegate: 7,
                ..QuitGuard::default()
            }));
            let weak = Rc::downgrade(&state);
            state.borrow_mut().callback = Some(Rc::new(move || {
                weak.upgrade().unwrap().borrow_mut().allowed = true;
                panic!("synthetic close callback failure");
            }));
            assert_eq!(
                decide(&state, 7),
                NSApplicationTerminateReply::TerminateCancel
            );
            assert!(!state.borrow().allowed);
            assert!(!state.borrow().callback_active);
        }

        #[test]
        fn native_quit_guard_fails_closed_without_a_callback_or_during_a_borrow() {
            let state = RefCell::new(QuitGuard::default());
            assert_eq!(
                decide(&state, 0),
                NSApplicationTerminateReply::TerminateCancel
            );
            state.borrow_mut().installed = true;
            assert_eq!(
                decide(&state, 0),
                NSApplicationTerminateReply::TerminateCancel
            );
            let _borrow = state.borrow_mut();
            assert_eq!(
                decide(&state, 0),
                NSApplicationTerminateReply::TerminateCancel
            );
        }
    }
}

#[cfg(target_os = "macos")]
// Policy tests can enable this module without linking the native app consumer.
#[cfg_attr(not(feature = "native"), allow(unused_imports))]
pub use macos::{allow_quit, install_quit_guard};

#[cfg(all(target_os = "macos", feature = "native"))]
pub use macos::keep_qa_window_visible;

#[cfg(not(target_os = "macos"))]
pub fn install_quit_guard(_: impl Fn() + 'static) -> Result<(), String> {
    Err(crate::i18n::tr!("quit_guard.unsupported").into())
}

#[cfg(not(target_os = "macos"))]
pub fn allow_quit() {}

#[cfg(all(not(target_os = "macos"), feature = "native"))]
pub fn keep_qa_window_visible() {}
