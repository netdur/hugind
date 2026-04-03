use std::ffi::CStr;
use std::ffi::c_void;
use std::os::raw::c_char;
use std::sync::{Mutex, Once};

static LOG_INIT: Once = Once::new();
static LOG_BUFFER: Mutex<Vec<String>> = Mutex::new(Vec::new());
static DEBUG_MODE: Mutex<bool> = Mutex::new(false);

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

unsafe extern "C" fn buffered_log_cb(
    level: llama_cpp::ggml_log_level,
    text: *const c_char,
    _user_data: *mut c_void,
) {
    if text.is_null() {
        return;
    }
    let msg = unsafe { CStr::from_ptr(text) }.to_string_lossy();
    let is_debug = DEBUG_MODE.lock().map(|g| *g).unwrap_or(false);
    if is_debug {
        eprint!("[llama:{}] {}", level, msg);
    }
    if let Ok(mut buf) = LOG_BUFFER.lock() {
        buf.push(msg.into_owned());
    }
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
        if let Ok(mut g) = DEBUG_MODE.lock() {
            *g = debug_enabled();
        }
        llama_cpp::llama_log_set(Some(buffered_log_cb), std::ptr::null_mut());
        llama_cpp::mtmd_log_set(Some(buffered_log_cb), std::ptr::null_mut());
    });
}

/// Drain and return all buffered log messages, clearing the buffer.
pub fn drain_log_buffer() -> Vec<String> {
    LOG_BUFFER
        .lock()
        .map(|mut buf| buf.drain(..).collect())
        .unwrap_or_default()
}
