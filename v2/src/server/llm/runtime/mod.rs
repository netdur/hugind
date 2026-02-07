pub mod logging;
pub mod threading;

use std::sync::Once;

static BACKEND_INIT: Once = Once::new();


pub fn init() {
    BACKEND_INIT.call_once(|| unsafe {
        llama_cpp::llama_backend_init();
    });
}




pub unsafe fn shutdown() {
    unsafe {
        llama_cpp::llama_backend_free();
    }
}
