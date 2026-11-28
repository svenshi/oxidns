// SPDX-FileCopyrightText: 2026 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::ffi::{CStr, c_char, c_void};
use std::io;
use std::ptr::{self, NonNull};
use std::sync::Arc;

use tokio::sync::mpsc::{self, Receiver, Sender};

type CfRef = *const c_void;
type Queue = *mut c_void;

#[repr(C)]
struct StoreContext {
    version: isize,
    info: *const c_void,
    retain: Option<unsafe extern "C" fn(CfRef) -> CfRef>,
    release: Option<unsafe extern "C" fn(CfRef)>,
    copy_description: Option<unsafe extern "C" fn(CfRef) -> CfRef>,
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFTypeArrayCallBacks: u8;
    fn CFRelease(object: CfRef);
    fn CFStringCreateWithCString(allocator: CfRef, value: *const c_char, encoding: u32) -> CfRef;
    fn CFArrayCreate(
        allocator: CfRef,
        values: *const CfRef,
        count: isize,
        callbacks: CfRef,
    ) -> CfRef;
}

#[link(name = "SystemConfiguration", kind = "framework")]
unsafe extern "C" {
    fn SCDynamicStoreCreate(
        allocator: CfRef,
        name: CfRef,
        callback: unsafe extern "C" fn(CfRef, CfRef, *mut c_void),
        context: *mut StoreContext,
    ) -> CfRef;
    fn SCDynamicStoreSetNotificationKeys(store: CfRef, keys: CfRef, patterns: CfRef) -> u8;
    fn SCDynamicStoreSetDispatchQueue(store: CfRef, queue: Queue) -> u8;
}

unsafe extern "C" {
    fn dispatch_queue_create(label: *const c_char, attrs: Queue) -> Queue;
    fn dispatch_release(queue: Queue);
}

unsafe extern "C" fn retain(context: CfRef) -> CfRef {
    // SAFETY: The pointer comes from Arc::as_ptr and the caller owns a
    // reference.
    unsafe { Arc::increment_strong_count(context.cast::<Sender<()>>()) };
    context
}

unsafe extern "C" fn release(context: CfRef) {
    // SAFETY: This balances a retain performed by the dynamic store.
    unsafe { Arc::decrement_strong_count(context.cast::<Sender<()>>()) };
}

unsafe extern "C" fn changed(_: CfRef, _: CfRef, context: *mut c_void) {
    // SAFETY: The dynamic store retains the sender for the callback's lifetime.
    let sender = unsafe { &*context.cast::<Sender<()>>() };
    let _ = sender.try_send(());
}

struct Cf(NonNull<c_void>);

impl Cf {
    fn new(raw: CfRef, error: &'static str) -> io::Result<Self> {
        NonNull::new(raw.cast_mut())
            .map(Self)
            .ok_or_else(|| io::Error::other(error))
    }

    fn string(value: &CStr) -> io::Result<Self> {
        // SAFETY: value is NUL-terminated; 0x08000100 denotes UTF-8.
        let raw = unsafe { CFStringCreateWithCString(ptr::null(), value.as_ptr(), 0x0800_0100) };
        Self::new(raw, "could not create dynamic store string")
    }

    fn as_ptr(&self) -> CfRef {
        self.0.as_ptr()
    }
}

impl Drop for Cf {
    fn drop(&mut self) {
        // SAFETY: Cf owns a reference returned by a Create function.
        unsafe { CFRelease(self.as_ptr()) };
    }
}

struct Registration {
    store: Cf,
    queue: NonNull<c_void>,
}

// SAFETY: The store uses a serial dispatch queue, and its retained context
// exclusively exposes a thread-safe Sender to callbacks.
unsafe impl Send for Registration {}

impl Drop for Registration {
    fn drop(&mut self) {
        // SAFETY: We still own the store and queue; the framework retains the
        // callback context until it is no longer in use.
        unsafe {
            SCDynamicStoreSetDispatchQueue(self.store.as_ptr(), ptr::null_mut());
            dispatch_release(self.queue.as_ptr());
        }
    }
}

pub(crate) struct Monitor {
    _registration: Registration,
    events: Receiver<()>,
}

impl Monitor {
    pub(crate) fn new() -> io::Result<Self> {
        let (sender, events) = mpsc::channel(1);
        let sender = Arc::new(sender);
        let mut context = StoreContext {
            version: 0,
            info: Arc::as_ptr(&sender).cast(),
            retain: Some(retain),
            release: Some(release),
            copy_description: None,
        };
        let name = Cf::string(c"OxiDNS")?;
        let regex = Cf::string(c"^State:/Network/(Global/(IPv4|DNS)|Interface/[^/]+/(Link|IPv4|AirPort)|Service/[^/]+/(IPv4|DHCP|DNS))$")?;
        // SAFETY: All pointers are live and the context has matching ownership
        // callbacks.
        let store = Cf::new(
            unsafe { SCDynamicStoreCreate(ptr::null(), name.as_ptr(), changed, &mut context) },
            "could not open the dynamic store",
        )?;
        let pattern = regex.as_ptr();
        // SAFETY: The callback table is the CoreFoundation-provided static.
        let patterns = Cf::new(
            unsafe {
                CFArrayCreate(
                    ptr::null(),
                    &pattern,
                    1,
                    (&raw const kCFTypeArrayCallBacks).cast(),
                )
            },
            "could not create the dynamic store patterns",
        )?;
        if unsafe {
            SCDynamicStoreSetNotificationKeys(store.as_ptr(), ptr::null(), patterns.as_ptr())
        } == 0
        {
            return Err(io::Error::other(
                "could not subscribe to dynamic store notifications",
            ));
        }
        // SAFETY: The label is NUL-terminated and a null attribute selects a
        // serial queue.
        let queue = NonNull::new(unsafe {
            dispatch_queue_create(c"oxidns.network".as_ptr(), ptr::null_mut())
        })
        .ok_or_else(|| io::Error::other("could not create the dynamic store queue"))?;
        let registration = Registration { store, queue };
        if unsafe { SCDynamicStoreSetDispatchQueue(registration.store.as_ptr(), queue.as_ptr()) }
            == 0
        {
            return Err(io::Error::other(
                "could not start dynamic store notifications",
            ));
        }
        Ok(Self {
            _registration: registration,
            events,
        })
    }

    pub(crate) async fn changed(&mut self) -> io::Result<()> {
        self.events
            .recv()
            .await
            .ok_or_else(|| io::Error::other("dynamic store notification stream stopped"))
    }
}
