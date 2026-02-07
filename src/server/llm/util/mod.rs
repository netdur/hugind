use std::ffi::{CStr, CString};
use crate::llm::error::Result;

pub fn to_c_string(s: &str) -> Result<CString> {
    Ok(CString::new(s)?)
}

pub fn from_c_str(s: *const std::os::raw::c_char) -> String {
    if s.is_null() {
        return String::new();
    }
    unsafe {
        CStr::from_ptr(s).to_string_lossy().into_owned()
    }
}
