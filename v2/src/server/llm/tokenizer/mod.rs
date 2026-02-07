pub mod special;
pub mod token;

pub use token::Token;

use crate::llm::error::{Error, Result};
use std::ffi::CString;

pub struct Tokenizer<'a> {
    model: *const llama_cpp::llama_model, // borrowed from Model
    vocab: *const llama_cpp::llama_vocab,
    _marker: std::marker::PhantomData<&'a ()>,
}

impl<'a> Tokenizer<'a> {
    /// Safe construction is usually done via Model::tokenizer()
    pub unsafe fn new(model: *const llama_cpp::llama_model) -> Self {
        let vocab = unsafe { llama_cpp::llama_model_get_vocab(model) };
        Self {
            model,
            vocab,
            _marker: std::marker::PhantomData,
        }
    }

    pub fn tokenize(&self, text: &str, add_special: bool, parse_special: bool) -> Result<Vec<Token>> {
        let c_text = CString::new(text)?;
        // Upper bound: 1 token per byte + padding
        let mut output = vec![0i32; text.len() + 16];
        
        let n = unsafe {
            llama_cpp::llama_tokenize(
                self.vocab,
                c_text.as_ptr(),
                text.len() as i32,
                output.as_mut_ptr(),
                output.len() as i32,
                add_special,
                parse_special,
            )
        };

        if n < 0 {
            // If negative, it means output buffer was too small (ret value is -n_needed)
            // or other error. Retry with larger buffer if it was size issue?
            // For now, simpler error.
            return Err(Error::TokenizeFailed); 
        }

        output.truncate(n as usize);
        Ok(output.into_iter().map(Token).collect())
    }

    pub fn token_to_piece(&self, token: Token) -> Result<Vec<u8>> {
        let mut buf = vec![0u8; 256];
        unsafe {
            let n = llama_cpp::llama_token_to_piece(
                self.vocab,
                token.0,
                buf.as_mut_ptr() as *mut i8,
                buf.len() as i32,
                0,    // lstrip (optional, usually 0)
                true, // special
            );
            if n < 0 {
                // Buffer too small, returns -n
                let needed = -n;
                buf.resize(needed as usize, 0);
                let n2 = llama_cpp::llama_token_to_piece(
                    self.vocab,
                    token.0,
                    buf.as_mut_ptr() as *mut i8,
                    buf.len() as i32,
                    0,
                    true,
                );
                if n2 < 0 {
                    return Err(Error::BackendError("Token to piece failed twice".into()));
                }
                buf.truncate(n2 as usize);
                Ok(buf)
            } else {
                buf.truncate(n as usize);
                Ok(buf)
            }
        }
    }

    pub fn decode(&self, tokens: &[Token]) -> Result<String> {
        let mut out = Vec::new();
        for &t in tokens {
            let piece = self.token_to_piece(t)?;
            out.extend_from_slice(&piece);
        }
        Ok(String::from_utf8_lossy(&out).into_owned())
    }

    pub fn vocab_ptr(&self) -> *const llama_cpp::llama_vocab {
        self.vocab
    }
}
