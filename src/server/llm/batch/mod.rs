mod fill;

use crate::llm::error::{Error, Result};

pub struct Batch {
    pub(crate) handle: llama_cpp::llama_batch,
    capacity: i32,
    n_seq_max: i32,
}

impl Batch {
    
    pub fn new(capacity: i32, embd: i32, n_seq_max: i32) -> Self {
        unsafe {
            let handle = llama_cpp::llama_batch_init(capacity, embd, n_seq_max);
            Self {
                handle,
                capacity,
                n_seq_max,
            }
        }
    }

    
    pub fn clear(&mut self) {
        self.handle.n_tokens = 0;
    }

    
    pub fn add(&mut self, token: llama_cpp::llama_token, pos: llama_cpp::llama_pos, seq_ids: &[llama_cpp::llama_seq_id], logits: bool) -> Result<()> {
        let i = self.handle.n_tokens as usize;
        if i >= self.capacity as usize {
            return Err(Error::BackendError(format!("Batch definition full: capacity {}", self.capacity)));
        }
        if seq_ids.len() > self.n_seq_max as usize {
            return Err(Error::BackendError(format!("Too many seq_ids: {} > {}", seq_ids.len(), self.n_seq_max)));
        }

        unsafe {
            fill::batch_set(&mut self.handle, i, token, pos, seq_ids, logits);
        }

        self.handle.n_tokens += 1;
        Ok(())
    }

    
    pub fn add_seq(&mut self, token: llama_cpp::llama_token, pos: llama_cpp::llama_pos, seq_id: llama_cpp::llama_seq_id, logits: bool) -> Result<()> {
        self.add(token, pos, &[seq_id], logits)
    }
}

impl Drop for Batch {
    fn drop(&mut self) {
        unsafe {
            llama_cpp::llama_batch_free(self.handle);
        }
    }
}
