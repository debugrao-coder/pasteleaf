use crate::{envelope, Error, Runtime};
use std::{
    ffi::{c_char, c_void, CStr, CString},
    panic::{catch_unwind, AssertUnwindSafe},
};
#[no_mangle]
pub extern "C" fn pl_api_version() -> u32 {
    2
}
/// # Safety
/// `root` must point to a valid NUL-terminated UTF-8 path for this call.
#[no_mangle]
pub unsafe extern "C" fn pl_open(root: *const c_char) -> *mut c_void {
    catch_unwind(AssertUnwindSafe(|| {
        if root.is_null() {
            return std::ptr::null_mut();
        }
        // SAFETY: caller supplies a valid live C string, per header contract.
        let Ok(root) = (unsafe { CStr::from_ptr(root) }).to_str() else {
            return std::ptr::null_mut();
        };
        match Runtime::open(root) {
            Ok(runtime) => Box::into_raw(Box::new(runtime)).cast(),
            Err(_) => std::ptr::null_mut(),
        }
    }))
    .unwrap_or(std::ptr::null_mut())
}
/// # Safety
/// `handle` is a live pl_open handle; `input` a live NUL-terminated UTF-8 string.
/// No concurrent pl_close is permitted. Returned string must be pl_free'd once.
#[no_mangle]
pub unsafe extern "C" fn pl_request(handle: *mut c_void, input: *const c_char) -> *mut c_char {
    let response = catch_unwind(AssertUnwindSafe(|| {
        if handle.is_null() || input.is_null() {
            return envelope(Err(Error::new(
                "invalid_pointer",
                "A valid core handle and request are required",
            )));
        }
        let input = match unsafe { CStr::from_ptr(input) }.to_str() {
            Ok(s) => s,
            Err(_) => {
                return envelope(Err(Error::new(
                    "invalid_utf8",
                    "Request must be UTF-8 JSON",
                )))
            }
        };
        // SAFETY: handle comes from pl_open, kept alive by caller for this call.
        unsafe { &*handle.cast::<Runtime>() }.request_json(input)
    }))
    .unwrap_or_else(|_| {
        envelope(Err(Error::new(
            "internal_panic",
            "Internal core error; restart and retain existing history",
        )))
    });
    CString::new(response)
        .expect("JSON serializer escapes interior NULs")
        .into_raw()
}
/// # Safety
/// `json` is NULL or a live pointer returned by pl_request, not previously freed.
#[no_mangle]
pub unsafe extern "C" fn pl_free(json: *mut c_char) {
    if !json.is_null() {
        drop(unsafe { CString::from_raw(json) });
    }
}
/// # Safety
/// `handle` is NULL or a live pl_open handle; all request calls have finished.
#[no_mangle]
pub unsafe extern "C" fn pl_close(handle: *mut c_void) {
    if !handle.is_null() {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            drop(unsafe { Box::from_raw(handle.cast::<Runtime>()) })
        }));
    }
}
