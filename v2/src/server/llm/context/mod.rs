pub mod params;

use crate::llm::batch::Batch;
use crate::llm::error::{Error, Result};
use crate::llm::ffi_guard;
use crate::llm::model::Model;
use std::ptr::NonNull;

pub use params::ContextParams;

pub struct Context {
    ptr: NonNull<llama_cpp::llama_context>,
    // Keep model alive as context depends on it
    // (In C++ llama.cpp, context usually doesn't own model but here we might want to ensure safety.
    // However, users often want to share model across contexts. So we can hold a reference or just rely on lifecycle management.)
    // For this plan, we'll assume the user manages the Model lifetime or we hold an Arc/Rc if we were using it.
    // But since Model is safe wrapper, let's just say Context is tied to a Model's lifetime 'm?
    // Or we just don't store it and assume the user keeps Model alive.
    // Llama.cpp doesn't crash if model is freed? Actually it might.
    // Let's stick to unsafe assumtion or phantom data for now, OR better, require model reference in new.
    // We won't store Model here to allow one model multiple contexts without Arc.
}

unsafe impl Send for Context {}
unsafe impl Sync for Context {}

impl Context {
    pub fn new(model: &Model, params: &ContextParams) -> Result<Self> {
        let c_params = params.to_c_params();
        let ptr = unsafe {
            llama_cpp::llama_new_context_with_model(model.as_ptr(), c_params)
        };
        
        // Error handling: llama_new_context_with_model returns NULL on failure
        let non_null = ffi_guard::ensure_non_null(ptr, "Failed to create context")?;
        
        Ok(Self { ptr: non_null })
    }

    pub fn as_ptr(&self) -> *mut llama_cpp::llama_context {
        self.ptr.as_ptr()
    }
    
    pub fn decode(&mut self, batch: &mut Batch) -> Result<()> {
        let ret = unsafe {
            llama_cpp::llama_decode(self.ptr.as_ptr(), batch.handle)
        };
        
        if ret != 0 {
            return Err(Error::DecodeFailed);
        }
        Ok(())
    }
    
    pub fn encode(&mut self, batch: &mut Batch) -> Result<()> {
        let ret = unsafe {
            llama_cpp::llama_encode(self.ptr.as_ptr(), batch.handle)
        };
        
        if ret != 0 {
            return Err(Error::DecodeFailed);
        }
        Ok(())
    }
    

    /// Get logits for a specific token index in the batch.
    /// Safety: The batch must have had logits enabled for this index, and decode must have run.
    pub fn get_logits(&self, batch_token_index: i32) -> *mut f32 {
        unsafe {
            llama_cpp::llama_get_logits_ith(self.as_ptr(), batch_token_index)
        }
    }

    /// Get embeddings for a specific token index in the batch.
    /// Safety: The batch must have had embeddings enabled for this index, and decode must have run.
    pub fn get_embeddings(&self, batch_token_index: i32) -> *mut f32 {
        unsafe {
            llama_cpp::llama_get_embeddings_ith(self.as_ptr(), batch_token_index)
        }
    }

    pub fn get_embeddings_seq(&self, seq_id: i32) -> *mut f32 {
        unsafe {
            llama_cpp::llama_get_embeddings_seq(self.as_ptr(), seq_id)
        }
    }

    pub fn get_embeddings_all(&self) -> *mut f32 {
        unsafe {
            llama_cpp::llama_get_embeddings(self.as_ptr())
        }
    }

    pub fn kv_cache_seq_rm(&mut self, seq_id: i32, p0: i32, p1: i32) -> bool {
        unsafe {
            let mem = llama_cpp::llama_get_memory(self.as_ptr());
            if mem.is_null() {
                return false;
            }
            llama_cpp::llama_memory_seq_rm(mem, seq_id, p0, p1)
        }
    }

    pub fn kv_cache_seq_cp(&mut self, seq_id_src: i32, seq_id_dst: i32, p0: i32, p1: i32) {
        unsafe {
            let mem = llama_cpp::llama_get_memory(self.as_ptr());
            if mem.is_null() {
                return;
            }
            llama_cpp::llama_memory_seq_cp(mem, seq_id_src, seq_id_dst, p0, p1);
        }
    }

    // New state management functions
    pub fn state_seq_get_data(&self, seq_id: i32) -> Result<Vec<u8>> {
        unsafe {
            let size = llama_cpp::llama_state_seq_get_size(self.as_ptr(), seq_id);
            if size == 0 {
                return Err(Error::ContextSizeInvalid); // Or specific error
            }
            let mut buf = vec![0u8; size];
            let written = llama_cpp::llama_state_seq_get_data(self.as_ptr(), buf.as_mut_ptr(), size, seq_id);
            if written != size {
                // This might happen if size changed?
                return Err(Error::ContextSizeInvalid);
            }
            Ok(buf)
        }
    }

    pub fn state_seq_set_data(&mut self, seq_id: i32, data: &[u8]) -> usize {
        unsafe {
            llama_cpp::llama_state_seq_set_data(self.as_ptr(), data.as_ptr(), data.len(), seq_id)
        }
    }

    pub fn state_seq_save_file(&self, filepath: &str, seq_id: i32, tokens: &[i32]) -> usize {
        let c_filepath = std::ffi::CString::new(filepath).unwrap();
        unsafe {
            // tokens needs to be passed
            llama_cpp::llama_state_seq_save_file(
                self.as_ptr(), 
                c_filepath.as_ptr(), 
                seq_id, 
                tokens.as_ptr(), 
                tokens.len()
            )
        }
    }

    pub fn state_seq_load_file(&mut self, filepath: &str, dest_seq_id: i32, token_capacity: usize) -> Result<(usize, Vec<i32>)> {
        let c_filepath = std::ffi::CString::new(filepath).unwrap();
        let mut tokens = vec![0i32; token_capacity];
        let mut n_token_count_out = 0usize;
        
        unsafe {
            let size = llama_cpp::llama_state_seq_load_file(
                self.as_ptr(),
                c_filepath.as_ptr(),
                dest_seq_id,
                tokens.as_mut_ptr(),
                token_capacity,
                &mut n_token_count_out as *mut usize
            );
            
            if size == 0 {
                return Err(Error::ModelLoadFailed("Failed to load KV state from file".to_string())); // Generic error
            }
            
            tokens.truncate(n_token_count_out);
            Ok((size, tokens))
        }
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        unsafe {
            llama_cpp::llama_free(self.ptr.as_ptr());
        }
    }
}
