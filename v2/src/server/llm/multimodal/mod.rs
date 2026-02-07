use crate::llm::error::{Error, Result};
use crate::llm::ffi_guard;
use crate::llm::model::Model;
use std::ffi::CString;
use std::ptr::NonNull;
use std::slice;

pub struct MultimodalContext {
    ptr: NonNull<llama_cpp::mtmd_context>,
}

unsafe impl Send for MultimodalContext {}
unsafe impl Sync for MultimodalContext {}

impl MultimodalContext {
    pub fn from_file(path: &str, model: &Model) -> Result<Self> {
        let c_path = CString::new(path)?;
        
        let ptr = unsafe {
            let params = llama_cpp::mtmd_context_params_default();
            llama_cpp::mtmd_init_from_file(c_path.as_ptr(), model.as_ptr(), params)
        };

        let non_null = ffi_guard::ensure_non_null(ptr, &format!("Failed to load mmproj from {}", path))?;

        Ok(Self { ptr: non_null })
    }
    
    pub fn as_ptr(&self) -> *mut llama_cpp::mtmd_context {
        self.ptr.as_ptr()
    }

    /// Tokenize text with images.
    /// Returns:
    /// - Vec<i32>: The sequence of tokens (including LLAMA_TOKEN_NULL/0).
    /// - Vec<Chunk>: List of chunks corresponding to the input.
    /// 
    /// Note: The original generic `tokenize` returns just tokens. Here we have to handle the fact that
    /// some "tokens" are actually placeholders for image chunks.
    /// The `Vec<i32>` returned will effectively be the flattened token stream.
    /// The `Vec<Chunk>` allows us to retrieve the actual content (text or image) for processing.
    pub fn tokenize(&self, text: &str, images: &[Image]) -> Result<(Vec<i32>, std::collections::HashMap<usize, Chunk>)> {
        let c_text = CString::new(text)?;
        
        // Prepare bitmaps array
        let mut bitmap_ptrs: Vec<*const llama_cpp::mtmd_bitmap> = images.iter().map(|img| img.as_ptr() as *const _).collect();
        
        unsafe {
            let input_text = llama_cpp::mtmd_input_text {
                text: c_text.as_ptr(),
                add_special: true,
                parse_special: true,
            };

            let chunks_ptr = llama_cpp::mtmd_input_chunks_init();
            if chunks_ptr.is_null() {
                return Err(Error::BackendError("Failed to init input chunks".into()));
            }
            // Ensure we free the chunks container
            let _chunks_guard = InputChunksGuard(chunks_ptr);

            let ret = llama_cpp::mtmd_tokenize(
                self.ptr.as_ptr(),
                chunks_ptr,
                &input_text,
                bitmap_ptrs.as_mut_ptr(),
                bitmap_ptrs.len(),
            );

            if ret != 0 {
                return Err(Error::BackendError(format!("mtmd_tokenize failed with code {}", ret)));
            }
            
            let n_chunks = llama_cpp::mtmd_input_chunks_size(chunks_ptr);
            let mut result_tokens = Vec::new();
            let mut result_images = std::collections::HashMap::new();

            for i in 0..n_chunks {
                let chunk_ptr = llama_cpp::mtmd_input_chunks_get(chunks_ptr, i);
                if chunk_ptr.is_null() {
                     continue;
                }
                
                // Copy chunk to own it safely
                let copied_chunk_ptr = llama_cpp::mtmd_input_chunk_copy(chunk_ptr);
                let chunk = Chunk { ptr: NonNull::new(copied_chunk_ptr).unwrap() };
                
                // Extract tokens from this chunk to build the flat token stream
                let chunk_type = llama_cpp::mtmd_input_chunk_get_type(chunk.as_ptr());
                
                if chunk_type == llama_cpp::mtmd_input_chunk_type_MTMD_INPUT_CHUNK_TYPE_TEXT {
                    let mut n_tokens = 0;
                    let tokens_ptr = llama_cpp::mtmd_input_chunk_get_tokens_text(chunk.as_ptr(), &mut n_tokens);
                    let tokens_slice = slice::from_raw_parts(tokens_ptr, n_tokens as usize);
                    result_tokens.extend_from_slice(tokens_slice);
                } else {
                    // Image/Audio chunk
                    let start_idx = result_tokens.len();
                    
                    let n_tokens = llama_cpp::mtmd_input_chunk_get_n_tokens(chunk.as_ptr());
                    // Fill with placeholders (NULL token or 0?)
                    // server-common.cpp uses LLAMA_TOKEN_NULL which is usually -1.
                    // But our Token wrapper wraps i32, and typically 0 is UNK or similar, but -1 is safe invalid.
                    let null_token = -1; 
                    for _ in 0..n_tokens {
                        result_tokens.push(null_token);
                    }
                    
                    result_images.insert(start_idx, chunk);
                }
            }

            Ok((result_tokens, result_images))
        }
    }
    
    pub fn eval_chunk(
        &self, 
        chunk: &Chunk,
        ctx: &crate::llm::context::Context,
        n_past: i32,
        seq_id: i32,
        n_batch: i32,
        logits_last: bool,
    ) -> Result<(i32, i32)> { // (status, new_n_past)
        let mut new_n_past_c = 0;
        let ret = unsafe {
            llama_cpp::mtmd_helper_eval_chunk_single(
                self.ptr.as_ptr(),
                ctx.as_ptr(),
                chunk.as_ptr(),
                n_past,
                seq_id,
                n_batch,
                logits_last,
                &mut new_n_past_c
            )
        };
        Ok((ret, new_n_past_c))
    }

    pub fn eval_chunk_range(
        &self,
        _chunk: &Chunk,
        _ctx: &crate::llm::context::Context,
        _n_past: i32,
        _seq_id: i32,
        _start: usize,
        _len: usize,
        _n_batch: i32,
        _logits_last: bool,
    ) -> Result<(i32, i32)> {
        // TODO: Add llama.cpp/llava-side support to evaluate partial chunk ranges.
        // This should call a backend function that processes only [start, start+len)
        // tokens/positions for the chunk and returns (status, n_done).
        Err(Error::BackendError("partial mm eval not supported".to_string()))
    }
}

impl Drop for MultimodalContext {
    fn drop(&mut self) {
        unsafe {
            llama_cpp::mtmd_free(self.ptr.as_ptr());
        }
    }
}

pub struct Image {
    ptr: NonNull<llama_cpp::mtmd_bitmap>,
}

impl Image {
    pub fn from_bytes(ctx: &MultimodalContext, bytes: &[u8]) -> Result<Self> {
        let ptr = unsafe {
            llama_cpp::mtmd_helper_bitmap_init_from_buf(
                ctx.as_ptr(),
                bytes.as_ptr(),
                bytes.len()
            )
        };
        let non_null = ffi_guard::ensure_non_null(ptr, "Failed to load image from bytes")?;
        Ok(Self { ptr: non_null })
    }
    
    pub fn as_ptr(&self) -> *mut llama_cpp::mtmd_bitmap {
        self.ptr.as_ptr()
    }
}

impl Drop for Image {
    fn drop(&mut self) {
        unsafe {
            llama_cpp::mtmd_bitmap_free(self.ptr.as_ptr());
        }
    }
}

pub struct Chunk {
    ptr: NonNull<llama_cpp::mtmd_input_chunk>,
}

unsafe impl Send for Chunk {}
unsafe impl Sync for Chunk {}

impl Chunk {
    pub fn as_ptr(&self) -> *const llama_cpp::mtmd_input_chunk {
        self.ptr.as_ptr()
    }
    
    pub fn is_text(&self) -> bool {
        unsafe {
            llama_cpp::mtmd_input_chunk_get_type(self.ptr.as_ptr()) == llama_cpp::mtmd_input_chunk_type_MTMD_INPUT_CHUNK_TYPE_TEXT
        }
    }

    pub fn n_tokens(&self) -> usize {
        unsafe {
            llama_cpp::mtmd_input_chunk_get_n_tokens(self.ptr.as_ptr()) as usize
        }
    }

    pub fn n_pos(&self) -> usize {
        unsafe {
            llama_cpp::mtmd_input_chunk_get_n_pos(self.ptr.as_ptr()) as usize
        }
    }
}

impl Drop for Chunk {
    fn drop(&mut self) {
        unsafe {
            llama_cpp::mtmd_input_chunk_free(self.ptr.as_ptr());
        }
    }
}

// Helper guard to ensure chunks list is freed
struct InputChunksGuard(*mut llama_cpp::mtmd_input_chunks);

impl Drop for InputChunksGuard {
    fn drop(&mut self) {
        unsafe {
             if !self.0.is_null() {
                 llama_cpp::mtmd_input_chunks_free(self.0);
             }
        }
    }
}
