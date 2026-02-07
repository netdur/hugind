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


pub fn init_silent_logging() {
    LOG_INIT.call_once(|| unsafe {
        llama_cpp::llama_log_set(Some(silent_log_cb), std::ptr::null_mut());
        llama_cpp::mtmd_log_set(Some(silent_log_cb), std::ptr::null_mut());
    });
}
