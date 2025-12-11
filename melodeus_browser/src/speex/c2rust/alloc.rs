use std::alloc::{alloc_zeroed, dealloc, realloc, Layout};
use std::ffi::{c_char, c_int, c_void, CStr};
use std::ptr;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsValue;

// Tiny replacement for the libc allocators used by the translated Speex code.
// We stash the payload size in a prefix so `free`/`realloc` can recover it.
const HEADER_SIZE: usize = std::mem::size_of::<usize>();
// Align generously (16 bytes) to avoid misalignment traps in wasm for wider types.
const DEFAULT_ALIGN: usize = 16;

#[inline]
fn layout_for_payload(size: usize) -> Option<Layout> {
    let total = size.checked_add(HEADER_SIZE)?;
    Layout::from_size_align(total, DEFAULT_ALIGN).ok()
}

#[inline]
unsafe fn write_len(base: *mut u8, len: usize) {
    ptr::write(base as *mut usize, len);
}

#[inline]
unsafe fn read_len(base: *const u8) -> usize {
    ptr::read(base as *const usize)
}

/// calloc replacement that keeps track of the allocation size so we can free/realloc safely.
pub unsafe fn calloc(nmemb: usize, size: usize) -> *mut c_void {
    let payload = match nmemb.checked_mul(size) {
        Some(sz) if sz > 0 => sz,
        // Some callers pass a zero-sized alloc; emulate libc by handing out a unique pointer.
        _ => 1,
    };
    let layout = match layout_for_payload(payload) {
        Some(l) => l,
        None => return ptr::null_mut(),
    };
    let raw = alloc_zeroed(layout);
    if raw.is_null() {
        return ptr::null_mut();
    }
    write_len(raw, payload);
    raw.add(HEADER_SIZE) as *mut c_void
}

/// realloc replacement that preserves the recorded size header.
pub unsafe fn realloc_bytes(ptr: *mut c_void, size: usize) -> *mut c_void {
    if ptr.is_null() {
        return calloc(1, size.max(1));
    }
    let payload = size.max(1);
    let new_layout = match layout_for_payload(payload) {
        Some(l) => l,
        None => return ptr::null_mut(),
    };

    let base = (ptr as *mut u8).sub(HEADER_SIZE);
    let old_size = read_len(base);
    let old_layout = match layout_for_payload(old_size) {
        Some(l) => l,
        None => return ptr::null_mut(),
    };

    let raw = realloc(base, old_layout, new_layout.size());
    if raw.is_null() {
        return ptr::null_mut();
    }

    // Zero any newly allocated tail to match calloc semantics.
    if new_layout.size() > old_layout.size() {
        let extra = new_layout.size() - old_layout.size();
        ptr::write_bytes(raw.add(old_layout.size()), 0, extra);
    }

    write_len(raw, payload);
    raw.add(HEADER_SIZE) as *mut c_void
}

#[inline]
pub unsafe fn free(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    let base = (ptr as *mut u8).sub(HEADER_SIZE);
    let size = read_len(base);
    if let Some(layout) = layout_for_payload(size) {
        dealloc(base, layout);
    }
}

fn cstr_to_string(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return "<null>".into();
    }
    // Lossy but sufficient for diagnostics.
    unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned()
}

fn log_error(text: &str) {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::console::error_1(&JsValue::from_str(text));
    }
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("{text}");
}

pub unsafe fn warn(msg: *const c_char) {
    let text = cstr_to_string(msg);
    log_error(&format!("speex warning: {}", text));
}

pub unsafe fn warn_int(msg: *const c_char, val: c_int) {
    let text = cstr_to_string(msg);
    log_error(&format!("speex warning: {} {}", text, val));
}

pub unsafe fn fatal(msg: *const c_char, file: *const c_char, line: c_int) -> ! {
    let m = cstr_to_string(msg);
    let f = cstr_to_string(file);
    log_error(&format!("Speex fatal error in {} line {}: {}", f, line, m));
    panic!("Speex fatal error in {} line {}: {}", f, line, m);
}
