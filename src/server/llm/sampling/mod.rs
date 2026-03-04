pub mod config;

use crate::llm::context::Context;
use crate::llm::error::{Error, Result};
use crate::llm::tokenizer::Token;
pub use config::GrammarParams;
pub use config::SamplingConfig;
use std::ffi::CString;

pub struct Sampler {
    chain: *mut llama_cpp::llama_sampler,
    grammar: *mut llama_cpp::llama_sampler,
    n_vocab: i32,
}

unsafe impl Send for Sampler {}

impl Sampler {
    pub fn new(
        config: &SamplingConfig,
        vocab: Option<*const llama_cpp::llama_vocab>,
    ) -> Result<Self> {
        unsafe {
            let chain_params = llama_cpp::llama_sampler_chain_default_params();
            let chain = llama_cpp::llama_sampler_chain_init(chain_params);

            let mut grammar_sampler: *mut llama_cpp::llama_sampler = std::ptr::null_mut();

            if config.greedy {
                let greedy = llama_cpp::llama_sampler_init_greedy();
                llama_cpp::llama_sampler_chain_add(chain, greedy);
            } else {
                if config.temp <= 0.0 {
                    let greedy = llama_cpp::llama_sampler_init_greedy();
                    llama_cpp::llama_sampler_chain_add(chain, greedy);
                } else {
                    let top_k = llama_cpp::llama_sampler_init_top_k(config.top_k);
                    llama_cpp::llama_sampler_chain_add(chain, top_k);

                    let top_p = llama_cpp::llama_sampler_init_top_p(config.top_p, 1);
                    llama_cpp::llama_sampler_chain_add(chain, top_p);

                    let temp = llama_cpp::llama_sampler_init_temp(config.temp);
                    llama_cpp::llama_sampler_chain_add(chain, temp);

                    let dist = llama_cpp::llama_sampler_init_dist(1234);
                    llama_cpp::llama_sampler_chain_add(chain, dist);
                }
            }

            if let Some(grammar) = &config.grammar {
                if let Some(vocab) = vocab {
                    let grammar_c = CString::new(grammar.grammar.clone())?;
                    let root_c = CString::new(grammar.root.clone())?;
                    let sampler = llama_cpp::llama_sampler_init_grammar(
                        vocab,
                        grammar_c.as_ptr(),
                        root_c.as_ptr(),
                    );
                    if sampler.is_null() {
                        return Err(Error::BackendError("Failed to init grammar sampler".into()));
                    }
                    grammar_sampler = sampler;
                }
            }

            let n_vocab = if let Some(vocab) = vocab {
                llama_cpp::llama_vocab_n_tokens(vocab)
            } else {
                0
            };

            Ok(Self {
                chain,
                grammar: grammar_sampler,
                n_vocab,
            })
        }
    }

    pub fn sample(&mut self, ctx: &Context, idx: i32) -> Token {
        unsafe {
            if self.grammar.is_null() || self.n_vocab <= 0 {
                let id = llama_cpp::llama_sampler_sample(self.chain, ctx.as_ptr(), idx);
                return Token(id);
            }

            llama_cpp::llama_synchronize(ctx.as_ptr());

            let logits = ctx.get_logits(idx);
            let n_vocab = self.n_vocab as usize;
            let mut data: Vec<llama_cpp::llama_token_data> = Vec::with_capacity(n_vocab);
            for i in 0..n_vocab {
                data.push(llama_cpp::llama_token_data {
                    id: i as i32,
                    logit: *logits.add(i),
                    p: 0.0,
                });
            }

            let mut arr = llama_cpp::llama_token_data_array {
                data: data.as_mut_ptr(),
                size: data.len(),
                selected: -1,
                sorted: false,
            };

            llama_cpp::llama_sampler_apply(self.grammar, &mut arr);
            llama_cpp::llama_sampler_apply(self.chain, &mut arr);

            if arr.selected < 0 {
                let id = llama_cpp::llama_sampler_sample(self.chain, ctx.as_ptr(), idx);
                return Token(id);
            }

            let id = (*arr.data.add(arr.selected as usize)).id;
            Token(id)
        }
    }

    pub fn accept(&mut self, token: Token) {
        unsafe {
            if !self.grammar.is_null() {
                llama_cpp::llama_sampler_accept(self.grammar, token.0);
            }
            llama_cpp::llama_sampler_accept(self.chain, token.0);
        }
    }
}

impl Drop for Sampler {
    fn drop(&mut self) {
        unsafe {
            llama_cpp::llama_sampler_free(self.chain);
            if !self.grammar.is_null() {
                llama_cpp::llama_sampler_free(self.grammar);
            }
        }
    }
}
