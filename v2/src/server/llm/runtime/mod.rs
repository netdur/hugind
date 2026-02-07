pub mod logging;
pub mod threading;

use std::sync::Once;

static BACKEND_INIT: Once = Once::new();

/// Initialize the llama.cpp backend. Safe to call multiple times (idempotent).
pub fn init() {
    BACKEND_INIT.call_once(|| unsafe {
        llama_cpp::llama_backend_init();
    });
}

/// Free the backend resources.
/// Safety: This should only be called when no other llama functions are running.
/// In practice, you might rely on OS cleanup at exit, but this exposes the API.
pub unsafe fn shutdown() {
    unsafe {
        llama_cpp::llama_backend_free();
    }
}
