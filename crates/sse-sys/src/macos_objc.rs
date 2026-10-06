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
unsafe extern "C" {
    fn objc_getClass(n: *const c_char) -> Class;
    fn sel_registerName(n: *const c_char) -> Sel;
    pub fn objc_allocateClassPair(s: Class, n: *const c_char, e: usize) -> Class;
    pub fn objc_registerClassPair(c: Class);
    pub fn class_addMethod(c: Class, s: Sel, i: *const c_void, t: *const c_char) -> Bool;
    fn objc_msgSend();
}
#[link(name = "CoreGraphics", kind = "framework")]
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
    let f: F = unsafe { mem::transmute(objc_msgSend as *const c_void) };
    let v = unsafe { f(cls, sel(c"stringWithUTF8String:"), c.as_ptr()) };
    (!v.is_null()).then_some(v)
}
pub fn rust_string(v: Id) -> Option<String> {
    if v.is_null() {
        return None;
    }
    type F = unsafe extern "C" fn(Id, Sel) -> *const c_char;
    let f: F = unsafe { mem::transmute(objc_msgSend as *const c_void) };
    let p = unsafe { f(v, sel(c"UTF8String")) };
    if p.is_null() {
        None
    } else {
        Some(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
    }
}
pub unsafe fn id(r: Id, s: Sel) -> Id {
    type F = unsafe extern "C" fn(Id, Sel) -> Id;
    let f: F = unsafe { mem::transmute(objc_msgSend as *const c_void) };
    unsafe { f(r, s) }
}
pub unsafe fn void(r: Id, s: Sel) {
    type F = unsafe extern "C" fn(Id, Sel);
    let f: F = unsafe { mem::transmute(objc_msgSend as *const c_void) };
    unsafe { f(r, s) }
}
pub unsafe fn void_id(r: Id, s: Sel, a: Id) {
    type F = unsafe extern "C" fn(Id, Sel, Id);
    let f: F = unsafe { mem::transmute(objc_msgSend as *const c_void) };
    unsafe { f(r, s, a) }
}
pub unsafe fn void_bool(r: Id, s: Sel, a: Bool) {
    type F = unsafe extern "C" fn(Id, Sel, Bool);
    let f: F = unsafe { mem::transmute(objc_msgSend as *const c_void) };
    unsafe { f(r, s, a) }
}
pub unsafe fn bool_(r: Id, s: Sel) -> Bool {
    type F = unsafe extern "C" fn(Id, Sel) -> Bool;
    let f: F = unsafe { mem::transmute(objc_msgSend as *const c_void) };
    unsafe { f(r, s) }
}
pub unsafe fn usize_(r: Id, s: Sel) -> usize {
    type F = unsafe extern "C" fn(Id, Sel) -> usize;
    let f: F = unsafe { mem::transmute(objc_msgSend as *const c_void) };
    unsafe { f(r, s) }
}
pub unsafe fn isize_(r: Id, s: Sel) -> isize {
    type F = unsafe extern "C" fn(Id, Sel) -> isize;
    let f: F = unsafe { mem::transmute(objc_msgSend as *const c_void) };
    unsafe { f(r, s) }
}
pub unsafe fn f64_(r: Id, s: Sel) -> f64 {
    type F = unsafe extern "C" fn(Id, Sel) -> f64;
    let f: F = unsafe { mem::transmute(objc_msgSend as *const c_void) };
    unsafe { f(r, s) }
}
pub unsafe fn id_id(r: Id, s: Sel, a: Id) -> Id {
    type F = unsafe extern "C" fn(Id, Sel, Id) -> Id;
    let f: F = unsafe { mem::transmute(objc_msgSend as *const c_void) };
    unsafe { f(r, s, a) }
}
pub unsafe fn id_usize(r: Id, s: Sel, a: usize) -> Id {
    type F = unsafe extern "C" fn(Id, Sel, usize) -> Id;
    // SAFETY: objc_msgSend is called with the exact object/object/NSUInteger ABI for NSArray::objectAtIndex:.
    let f: F = unsafe { mem::transmute(objc_msgSend as *const c_void) };
    // SAFETY: the caller supplies a live NSArray receiver and an in-range index.
    unsafe { f(r, s, a) }
}
pub unsafe fn id_rect(r: Id, s: Sel, a: Rect) -> Id {
    type F = unsafe extern "C" fn(Id, Sel, Rect) -> Id;
    let f: F = unsafe { mem::transmute(objc_msgSend as *const c_void) };
    unsafe { f(r, s, a) }
}
pub unsafe fn void_size(r: Id, s: Sel, a: Size) {
    type F = unsafe extern "C" fn(Id, Sel, Size);
    let f: F = unsafe { mem::transmute(objc_msgSend as *const c_void) };
    unsafe { f(r, s, a) }
}
pub unsafe fn void_isize(r: Id, s: Sel, a: isize) {
    type F = unsafe extern "C" fn(Id, Sel, isize);
    let f: F = unsafe { mem::transmute(objc_msgSend as *const c_void) };
    unsafe { f(r, s, a) }
}
pub unsafe fn event(r: Id, s: Sel, mask: u64, until: Id, mode: Id, dequeue: Bool) -> Id {
    type F = unsafe extern "C" fn(Id, Sel, u64, Id, Id, Bool) -> Id;
    let f: F = unsafe { mem::transmute(objc_msgSend as *const c_void) };
    unsafe { f(r, s, mask, until, mode, dequeue) }
}
pub unsafe fn window_init(r: Id, s: Sel, rect: Rect, style: usize, backing: usize, defer: Bool) -> Id {
    type F = unsafe extern "C" fn(Id, Sel, Rect, usize, usize, Bool) -> Id;
    let f: F = unsafe { mem::transmute(objc_msgSend as *const c_void) };
    unsafe { f(r, s, rect, style, backing, defer) }
}
pub unsafe fn point(r: Id, s: Sel) -> Point {
    type F = unsafe extern "C" fn(Id, Sel) -> Point;
    let f: F = unsafe { mem::transmute(objc_msgSend as *const c_void) };
    unsafe { f(r, s) }
}
pub unsafe fn date(r: Id, s: Sel, seconds: f64) -> Id {
    type F = unsafe extern "C" fn(Id, Sel, f64) -> Id;
    let f: F = unsafe { mem::transmute(objc_msgSend as *const c_void) };
    unsafe { f(r, s, seconds) }
}
pub unsafe fn menu_init(r: Id, s: Sel, title: Id, action: Sel, key: Id) -> Id {
    type F = unsafe extern "C" fn(Id, Sel, Id, Sel, Id) -> Id;
    let f: F = unsafe { mem::transmute(objc_msgSend as *const c_void) };
    unsafe { f(r, s, title, action, key) }
}
pub unsafe fn post_event(r: Id, s: Sel, event: Id, start: Bool) {
    type F = unsafe extern "C" fn(Id, Sel, Id, Bool);
    let f: F = unsafe { mem::transmute(objc_msgSend as *const c_void) };
    unsafe { f(r, s, event, start) }
}
