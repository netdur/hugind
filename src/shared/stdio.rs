use std::sync::{Arc, OnceLock};

use parking_lot::RwLock;

pub trait PrintSink: Send + Sync {
    fn print(&self, msg: &str);
    fn print_raw(&self, msg: &str);
}

fn sink_lock() -> &'static RwLock<Option<Arc<dyn PrintSink>>> {
    static LOCK: OnceLock<RwLock<Option<Arc<dyn PrintSink>>>> = OnceLock::new();
    LOCK.get_or_init(|| RwLock::new(None))
}

pub fn set_print_sink(sink: Option<Arc<dyn PrintSink>>) {
    *sink_lock().write() = sink;
}

pub fn print(msg: &str) {
    if let Some(sink) = sink_lock().read().as_ref() {
        sink.print(msg);
    } else {
        println!("{msg}");
    }
}

pub fn print_raw(msg: &str) {
    if let Some(sink) = sink_lock().read().as_ref() {
        sink.print_raw(msg);
    } else {
        use std::io::{self, Write};
        let mut out = io::stdout();
        let _ = out.write_all(msg.as_bytes());
        let _ = out.flush();
    }
}
