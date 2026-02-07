
#[derive(Debug, Clone, Copy)]
pub struct ThreadingConfig {
    pub n_threads: Option<i32>,
    pub n_threads_batch: Option<i32>,
}

impl Default for ThreadingConfig {
    fn default() -> Self {
        Self {
            n_threads: None, 
            n_threads_batch: None,
        }
    }
}
