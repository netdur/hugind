use std::ffi::CStr;
use std::ffi::c_void;
use std::os::raw::c_char;
use std::sync::Once;

static LOG_INIT: Once = Once::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Error = 2,
    Warn = 3,
    Info = 4,
    Debug = 5,
}

pub type LogCallback = unsafe extern "C" fn(
    level: llama_cpp::ggml_log_level,
    text: *const c_char,
    user_data: *mut c_void,
);

unsafe extern "C" fn silent_log_cb(
    _level: llama_cpp::ggml_log_level,
    _text: *const c_char,
    _user_data: *mut c_void,
) {
}

unsafe extern "C" fn stderr_log_cb(
    level: llama_cpp::ggml_log_level,
    text: *const c_char,
    _user_data: *mut c_void,
) {
    if text.is_null() {
        return;
    }
    let msg = unsafe { CStr::from_ptr(text) }.to_string_lossy();
    eprint!("[llama:{}] {}", level, msg);
}

fn debug_enabled() -> bool {
    std::env::var("HUGIND_LLAMA_DEBUG")
        .ok()
        .map(|v| {
            let v = v.trim().to_ascii_lowercase();
            matches!(v.as_str(), "1" | "true" | "yes" | "on" | "debug")
        })
        .unwrap_or(false)
}

pub fn init_silent_logging() {
    LOG_INIT.call_once(|| unsafe {
        let cb: LogCallback = if debug_enabled() {
            stderr_log_cb
        } else {
            silent_log_cb
        };
        llama_cpp::llama_log_set(Some(cb), std::ptr::null_mut());
        llama_cpp::mtmd_log_set(Some(cb), std::ptr::null_mut());
    });
}
