pub mod special;
pub mod token;

pub use token::Token;

use crate::llm::error::{Error, Result};
use std::collections::VecDeque;
use std::ffi::CString;

pub struct Tokenizer<'a> {
    model: *const llama_cpp::llama_model,
    vocab: *const llama_cpp::llama_vocab,
    _marker: std::marker::PhantomData<&'a ()>,
}

impl<'a> Tokenizer<'a> {
    pub unsafe fn new(model: *const llama_cpp::llama_model) -> Self {
        let vocab = unsafe { llama_cpp::llama_model_get_vocab(model) };
        Self {
            model,
            vocab,
            _marker: std::marker::PhantomData,
        }
    }

    pub fn tokenize(
        &self,
        text: &str,
        add_special: bool,
        parse_special: bool,
    ) -> Result<Vec<Token>> {
        let c_text = CString::new(text)?;

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
                0,
                true,
            );
            if n < 0 {
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

    pub fn decode_incremental(&self, pending: &mut VecDeque<u8>, token: Token) -> Result<String> {
        let piece = self.token_to_piece(token)?;
        Ok(decode_utf8_incremental(pending, &piece))
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

pub fn decode_utf8_incremental(pending: &mut VecDeque<u8>, bytes: &[u8]) -> String {
    pending.extend(bytes.iter().copied());
    if pending.is_empty() {
        return String::new();
    }

    let buf: Vec<u8> = pending.iter().copied().collect();
    match std::str::from_utf8(&buf) {
        Ok(text) => {
            pending.clear();
            text.to_owned()
        }
        Err(err) => {
            let mut out = String::new();
            let valid_up_to = err.valid_up_to();

            if valid_up_to > 0 {
                let valid = &buf[..valid_up_to];
                if let Ok(text) = std::str::from_utf8(valid) {
                    out.push_str(text);
                }
            }

            let tail_start = if let Some(error_len) = err.error_len() {
                out.push('\u{FFFD}');
                valid_up_to + error_len
            } else {
                valid_up_to
            };

            pending.clear();
            pending.extend(buf[tail_start..].iter().copied());
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::decode_utf8_incremental;
    use std::collections::VecDeque;

    #[test]
    fn incremental_decoder_waits_for_complete_multibyte_sequence() {
        let mut pending = VecDeque::new();
        assert_eq!(decode_utf8_incremental(&mut pending, &[0xF0, 0x9F]), "");
        assert_eq!(decode_utf8_incremental(&mut pending, &[0x98]), "");
        assert_eq!(decode_utf8_incremental(&mut pending, &[0x8A]), "😊");
        assert!(pending.is_empty());
    }
}
