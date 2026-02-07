pub mod params;

use crate::llm::error::{Result};
use crate::llm::ffi_guard;
use crate::llm::tokenizer::Tokenizer;
use std::ffi::CString;
use std::ptr::NonNull;

pub use params::ModelParams;

pub struct Model {
    ptr: NonNull<llama_cpp::llama_model>,
}

unsafe impl Send for Model {}
unsafe impl Sync for Model {}

impl Model {
    pub fn from_file(path: &str, params: &ModelParams) -> Result<Self> {
        let c_path = CString::new(path)?;
        let c_params = params.to_c_params();

        let ptr = unsafe {
            llama_cpp::llama_load_model_from_file(c_path.as_ptr(), c_params)
        };

        let non_null = ffi_guard::ensure_non_null(ptr, &format!("Failed to load model from {}", path))?;

        Ok(Self { ptr: non_null })
    }

    pub fn as_ptr(&self) -> *mut llama_cpp::llama_model {
        self.ptr.as_ptr()
    }

    pub fn tokenizer(&self) -> Tokenizer<'_> {
        unsafe { Tokenizer::new(self.as_ptr()) } 
    }
    
    pub fn vocab(&self) -> *const llama_cpp::llama_vocab {
        unsafe { llama_cpp::llama_model_get_vocab(self.ptr.as_ptr()) }
    }

    pub fn get_metadata(&self, key: &str) -> Result<Option<String>> {
        let c_key = CString::new(key)?;
        let mut buf = vec![0u8; 1024];
        
        let res = unsafe {
            llama_cpp::llama_model_meta_val_str(
                self.ptr.as_ptr(),
                c_key.as_ptr(),
                buf.as_mut_ptr() as *mut i8,
                buf.len(),
            )
        };
        
        if res < 0 {
            // Key not found or error
            return Ok(None);
        }
        
        // If result > buffer len, we need to resize?
        // documentation says: returns length on success.
        // If >= buf_size, it was truncated?
        // Let's assume 1024 is enough for now or resize.
        
        let len = res as usize;
        if len >= buf.len() {
             // Resize and retry
             buf.resize(len + 1, 0);
             unsafe {
                llama_cpp::llama_model_meta_val_str(
                    self.ptr.as_ptr(),
                    c_key.as_ptr(),
                    buf.as_mut_ptr() as *mut i8,
                    buf.len(),
                )
             };
        }
        
        // Convert to string
        let c_str = unsafe { std::ffi::CStr::from_ptr(buf.as_ptr() as *const i8) };
        Ok(Some(c_str.to_string_lossy().into_owned()))
    }

    pub fn chat_template(&self) -> Result<String> {
        // Try tokenizer.chat_template
        if let Some(tmpl) = self.get_metadata("tokenizer.chat_template")? {
            return Ok(tmpl);
        }
        // Fallback?
        Ok("".to_string())
    }

    pub fn is_eog_token(&self, token: llama_cpp::llama_token) -> bool {
        unsafe {
            let vocab = llama_cpp::llama_model_get_vocab(self.ptr.as_ptr());
            llama_cpp::llama_vocab_is_eog(vocab, token)
        }
    }

    pub fn n_embd(&self) -> i32 {
        unsafe { llama_cpp::llama_model_n_embd(self.ptr.as_ptr()) }
    }

    pub fn has_encoder(&self) -> bool {
        unsafe { llama_cpp::llama_model_has_encoder(self.ptr.as_ptr()) }
    }

    pub fn has_decoder(&self) -> bool {
        unsafe { llama_cpp::llama_model_has_decoder(self.ptr.as_ptr()) }
    }

    pub fn token_bos(&self) -> llama_cpp::llama_token {
        unsafe { 
            let vocab = llama_cpp::llama_model_get_vocab(self.ptr.as_ptr());
            llama_cpp::llama_vocab_bos(vocab) 
        }
    }
}

impl Drop for Model {
    fn drop(&mut self) {
        unsafe {
            llama_cpp::llama_free_model(self.ptr.as_ptr());
        }
    }
}
