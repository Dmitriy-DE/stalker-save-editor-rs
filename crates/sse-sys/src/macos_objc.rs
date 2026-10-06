//! Objective-C/CoreGraphics ABI for the Cocoa window backend.
//!
//! objc_msgSend is cast at each call site to the exact selector signature. This is valid on arm64 and x86_64
//! for the scalar/object-returning selectors used here; no large-structure return requires objc_msgSend_stret.

use std::{
    ffi::{c_char, c_void, CStr, CString},
    mem,
};
pub type Id = *mut c_void;
pub type Sel = *mut c_void;
pub type Class = *mut c_void;
pub type Bool = i8;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Rect {
    pub origin: Point,
    pub size: Size,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Range {
    pub location: usize,
    pub length: usize,
}

#[link(name = "objc")]
// SAFETY: these declarations use the Objective-C runtime C ABI; callers provide live objects and NUL-terminated names.
unsafe extern "C" {
    fn objc_getClass(n: *const c_char) -> Class;
    fn sel_registerName(n: *const c_char) -> Sel;
    pub fn objc_allocateClassPair(s: Class, n: *const c_char, e: usize) -> Class;
    pub fn objc_registerClassPair(c: Class);
    pub fn class_addMethod(c: Class, s: Sel, i: *const c_void, t: *const c_char) -> Bool;
    fn objc_msgSend();
}
#[link(name = "CoreGraphics", kind = "framework")]
// SAFETY: these declarations match the CoreGraphics C ABI; ownership and pointer lifetimes are checked by the window backend.
unsafe extern "C" {
    pub fn CGColorSpaceCreateDeviceRGB() -> Id;
    pub fn CGColorSpaceRelease(v: Id);
    pub fn CGDataProviderCreateWithData(
        info: *mut c_void,
        data: *const c_void,
        size: usize,
        release: Option<unsafe extern "C" fn(*mut c_void, *const c_void, usize)>,
    ) -> Id;
    pub fn CGDataProviderRelease(v: Id);
    pub fn CGImageCreate(
        w: usize,
        h: usize,
        bpc: usize,
        bpp: usize,
        row: usize,
        space: Id,
        info: u32,
        provider: Id,
        decode: *const f64,
        interpolate: Bool,
        intent: u32,
    ) -> Id;
    pub fn CGImageRelease(v: Id);
}

pub fn class(n: &'static CStr) -> Class {
    // SAFETY: static NUL-terminated class name.
    unsafe { objc_getClass(n.as_ptr()) }
}
pub fn sel(n: &'static CStr) -> Sel {
    // SAFETY: static NUL-terminated selector name.
    unsafe { sel_registerName(n.as_ptr()) }
}
pub fn string(value: &str) -> Option<Id> {
    let c = CString::new(value).ok()?;
    let cls = class(c"NSString");
    type F = unsafe extern "C" fn(Id, Sel, *const c_char) -> Id; // SAFETY: exact +stringWithUTF8String: ABI.
                                                                 // SAFETY: `F` is the exact Objective-C ABI for `+stringWithUTF8String:` on supported targets.
    let f: F = unsafe { mem::transmute(objc_msgSend as *const c_void) };
    // SAFETY: `cls` and selector are live runtime handles and `c` is NUL-terminated and lives through the synchronous call.
    let v = unsafe { f(cls, sel(c"stringWithUTF8String:"), c.as_ptr()) };
    (!v.is_null()).then_some(v)
}
pub fn rust_string(v: Id) -> Option<String> {
    if v.is_null() {
        return None;
    }
    type F = unsafe extern "C" fn(Id, Sel) -> *const c_char;
    // SAFETY: `F` is the exact Objective-C ABI for the `UTF8String` selector.
    let f: F = unsafe { mem::transmute(objc_msgSend as *const c_void) };
    // SAFETY: `v` is a live Objective-C object and `UTF8String` takes no arguments beyond receiver and selector.
    let p = unsafe { f(v, sel(c"UTF8String")) };
    if p.is_null() {
        None
    } else {
        // SAFETY: a non-null `UTF8String` result points to a NUL-terminated string valid while `v` remains alive; it is copied immediately.
        Some(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
    }
}
/// Sends a zero-argument Objective-C message returning an object.
///
/// # Safety
/// `r` must be a live object or class and `s` must select a method with this exact ABI.
pub unsafe fn id(r: Id, s: Sel) -> Id {
    type F = unsafe extern "C" fn(Id, Sel) -> Id;
    // SAFETY: the caller selects the zero-argument object-returning ABI required by `F`.
    let f: F = unsafe { mem::transmute(objc_msgSend as *const c_void) };
    // SAFETY: the caller upholds the receiver and selector requirements documented above.
    unsafe { f(r, s) }
}
/// Sends a zero-argument Objective-C message returning no value.
///
/// # Safety
/// `r` must be a live object or class and `s` must select a method with this exact ABI.
pub unsafe fn void(r: Id, s: Sel) {
    type F = unsafe extern "C" fn(Id, Sel);
    // SAFETY: the caller selects the zero-argument void ABI required by `F`.
    let f: F = unsafe { mem::transmute(objc_msgSend as *const c_void) };
    // SAFETY: the caller upholds the receiver and selector requirements documented above.
    unsafe { f(r, s) }
}
/// Sends an Objective-C message with one object argument and no return value.
///
/// # Safety
/// `r` must be a live object or class, `s` must select a method with this exact ABI, and `a` must satisfy that method's ownership rules.
pub unsafe fn void_id(r: Id, s: Sel, a: Id) {
    type F = unsafe extern "C" fn(Id, Sel, Id);
    // SAFETY: the caller selects the one-object void ABI required by `F`.
    let f: F = unsafe { mem::transmute(objc_msgSend as *const c_void) };
    // SAFETY: the caller upholds the receiver, selector, and object argument requirements documented above.
    unsafe { f(r, s, a) }
}
/// Sends an Objective-C message with one Bool argument and no return value.
///
/// # Safety
/// `r` must be a live object or class and `s` must select a method with this exact ABI.
pub unsafe fn void_bool(r: Id, s: Sel, a: Bool) {
    type F = unsafe extern "C" fn(Id, Sel, Bool);
    // SAFETY: the caller selects the one-Bool void ABI required by `F`.
    let f: F = unsafe { mem::transmute(objc_msgSend as *const c_void) };
    // SAFETY: the caller upholds the receiver and selector requirements documented above.
    unsafe { f(r, s, a) }
}
/// Sends a zero-argument Objective-C message returning a Bool.
///
/// # Safety
/// `r` must be a live object or class and `s` must select a method with this exact ABI.
pub unsafe fn bool_(r: Id, s: Sel) -> Bool {
    type F = unsafe extern "C" fn(Id, Sel) -> Bool;
    // SAFETY: the caller selects the zero-argument Bool-returning ABI required by `F`.
    let f: F = unsafe { mem::transmute(objc_msgSend as *const c_void) };
    // SAFETY: the caller upholds the receiver and selector requirements documented above.
    unsafe { f(r, s) }
}
/// Sends a zero-argument Objective-C message returning `usize`.
///
/// # Safety
/// `r` must be a live object or class and `s` must select a method with this exact ABI.
pub unsafe fn usize_(r: Id, s: Sel) -> usize {
    type F = unsafe extern "C" fn(Id, Sel) -> usize;
    // SAFETY: the caller selects the zero-argument `usize` ABI required by `F`.
    let f: F = unsafe { mem::transmute(objc_msgSend as *const c_void) };
    // SAFETY: the caller upholds the receiver and selector requirements documented above.
    unsafe { f(r, s) }
}
/// Sends a zero-argument Objective-C message returning `isize`.
///
/// # Safety
/// `r` must be a live object or class and `s` must select a method with this exact ABI.
pub unsafe fn isize_(r: Id, s: Sel) -> isize {
    type F = unsafe extern "C" fn(Id, Sel) -> isize;
    // SAFETY: the caller selects the zero-argument `isize` ABI required by `F`.
    let f: F = unsafe { mem::transmute(objc_msgSend as *const c_void) };
    // SAFETY: the caller upholds the receiver and selector requirements documented above.
    unsafe { f(r, s) }
}
/// Sends a zero-argument Objective-C message returning `f64`.
///
/// # Safety
/// `r` must be a live object or class and `s` must select a method with this exact ABI.
pub unsafe fn f64_(r: Id, s: Sel) -> f64 {
    type F = unsafe extern "C" fn(Id, Sel) -> f64;
    // SAFETY: the caller selects the zero-argument `f64` ABI required by `F`.
    let f: F = unsafe { mem::transmute(objc_msgSend as *const c_void) };
    // SAFETY: the caller upholds the receiver and selector requirements documented above.
    unsafe { f(r, s) }
}
/// Sends an Objective-C message with one object argument and returns an object.
///
/// # Safety
/// `r` must be a live object or class, `s` must select a method with this exact ABI, and `a` must satisfy that method's ownership rules.
pub unsafe fn id_id(r: Id, s: Sel, a: Id) -> Id {
    type F = unsafe extern "C" fn(Id, Sel, Id) -> Id;
    // SAFETY: the caller selects the one-object object-returning ABI required by `F`.
    let f: F = unsafe { mem::transmute(objc_msgSend as *const c_void) };
    // SAFETY: the caller upholds the receiver, selector, and object argument requirements documented above.
    unsafe { f(r, s, a) }
}
/// Sends an Objective-C message with a CGRect-compatible argument and returns an object.
///
/// # Safety
/// `r` must be a live object or class and `s` must select a method with this exact ABI.
pub unsafe fn id_rect(r: Id, s: Sel, a: Rect) -> Id {
    type F = unsafe extern "C" fn(Id, Sel, Rect) -> Id;
    // SAFETY: the caller selects the `Rect` object-returning ABI required by `F`.
    let f: F = unsafe { mem::transmute(objc_msgSend as *const c_void) };
    // SAFETY: the caller upholds the receiver and selector requirements documented above.
    unsafe { f(r, s, a) }
}
/// Sends an Objective-C message with one CGSize-compatible argument and no return value.
///
/// # Safety
/// `r` must be a live object or class and `s` must select a method with this exact ABI.
pub unsafe fn void_size(r: Id, s: Sel, a: Size) {
    type F = unsafe extern "C" fn(Id, Sel, Size);
    // SAFETY: the caller selects the `Size` void ABI required by `F`.
    let f: F = unsafe { mem::transmute(objc_msgSend as *const c_void) };
    // SAFETY: the caller upholds the receiver and selector requirements documented above.
    unsafe { f(r, s, a) }
}
/// Sends an Objective-C message with one `isize` argument and no return value.
///
/// # Safety
/// `r` must be a live object or class and `s` must select a method with this exact ABI.
pub unsafe fn void_isize(r: Id, s: Sel, a: isize) {
    type F = unsafe extern "C" fn(Id, Sel, isize);
    // SAFETY: the caller selects the one-`isize` void ABI required by `F`.
    let f: F = unsafe { mem::transmute(objc_msgSend as *const c_void) };
    // SAFETY: the caller upholds the receiver and selector requirements documented above.
    unsafe { f(r, s, a) }
}
/// Sends an Objective-C event method with its full argument list.
///
/// # Safety
/// `r`, `until`, and `mode` must be valid Objective-C handles, and `s` must select a method with this exact ABI.
pub unsafe fn event(r: Id, s: Sel, mask: u64, until: Id, mode: Id, dequeue: Bool) -> Id {
    type F = unsafe extern "C" fn(Id, Sel, u64, Id, Id, Bool) -> Id;
    // SAFETY: the caller selects the exact event ABI required by `F`.
    let f: F = unsafe { mem::transmute(objc_msgSend as *const c_void) };
    // SAFETY: the caller upholds the receiver, selector, and handle requirements documented above.
    unsafe { f(r, s, mask, until, mode, dequeue) }
}
/// Initializes an NSWindow using its structure-valued initializer ABI.
///
/// # Safety
/// `r` must be an allocated NSWindow, and `s` must select `initWithContentRect:styleMask:backing:defer:`.
pub unsafe fn window_init(r: Id, s: Sel, rect: Rect, style: usize, backing: usize, defer: Bool) -> Id {
    type F = unsafe extern "C" fn(Id, Sel, Rect, usize, usize, Bool) -> Id;
    // SAFETY: the caller selects the exact window initializer ABI required by `F`.
    let f: F = unsafe { mem::transmute(objc_msgSend as *const c_void) };
    // SAFETY: the caller upholds the allocated receiver and selector requirements documented above.
    unsafe { f(r, s, rect, style, backing, defer) }
}
/// Sends an Objective-C message returning a CGPoint-compatible structure.
///
/// # Safety
/// `r` must be a live object or class and `s` must select a method with this exact ABI.
pub unsafe fn point(r: Id, s: Sel) -> Point {
    type F = unsafe extern "C" fn(Id, Sel) -> Point;
    // SAFETY: the caller selects the exact `Point`-returning ABI required by `F`.
    let f: F = unsafe { mem::transmute(objc_msgSend as *const c_void) };
    // SAFETY: the caller upholds the receiver and selector requirements documented above.
    unsafe { f(r, s) }
}
/// Sends an Objective-C message with one time interval and returns an object.
///
/// # Safety
/// `r` must be a live object or class and `s` must select a method with this exact ABI.
pub unsafe fn date(r: Id, s: Sel, seconds: f64) -> Id {
    type F = unsafe extern "C" fn(Id, Sel, f64) -> Id;
    // SAFETY: the caller selects the one-`f64` object-returning ABI required by `F`.
    let f: F = unsafe { mem::transmute(objc_msgSend as *const c_void) };
    // SAFETY: the caller upholds the receiver and selector requirements documented above.
    unsafe { f(r, s, seconds) }
}
/// Initializes an Objective-C menu item with title, selector, and key equivalent.
///
/// # Safety
/// `r` must be an allocated NSMenuItem and all object/selector arguments must be valid for the selected initializer.
pub unsafe fn menu_init(r: Id, s: Sel, title: Id, action: Sel, key: Id) -> Id {
    type F = unsafe extern "C" fn(Id, Sel, Id, Sel, Id) -> Id;
    // SAFETY: the caller selects the exact NSMenuItem initializer ABI required by `F`.
    let f: F = unsafe { mem::transmute(objc_msgSend as *const c_void) };
    // SAFETY: the caller upholds the receiver, selector, and argument requirements documented above.
    unsafe { f(r, s, title, action, key) }
}
/// Posts an Objective-C event through NSApplication.
///
/// # Safety
/// `r` must be a live NSApplication, `event` a live NSEvent, and `s` the matching `postEvent:atStart:` selector.
pub unsafe fn post_event(r: Id, s: Sel, event: Id, start: Bool) {
    type F = unsafe extern "C" fn(Id, Sel, Id, Bool);
    // SAFETY: the caller selects the exact `postEvent:atStart:` ABI required by `F`.
    let f: F = unsafe { mem::transmute(objc_msgSend as *const c_void) };
    // SAFETY: the caller upholds the receiver, selector, and event requirements documented above.
    unsafe { f(r, s, event, start) }
}
