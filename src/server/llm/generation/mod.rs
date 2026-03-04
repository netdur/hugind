pub mod config;
pub mod stop;

use crate::llm::batch::Batch;
use crate::llm::context::Context;
use crate::llm::error::Result;
use crate::llm::sampling::Sampler;
use crate::llm::session::Session;
use crate::llm::tokenizer::{Token, Tokenizer};
pub use config::GenerationConfig;

pub fn generate_simple(
    ctx: &mut Context,
    session: &mut Session,
    tokenizer: &Tokenizer,
    config: &GenerationConfig,
    prompt_tokens: &[Token],
    mut callback: impl FnMut(Token, String) -> bool,
) -> Result<()> {
    let mut batch = Batch::new(2048, 0, 1);
    let mut utf8_pending = std::collections::VecDeque::new();

    batch.clear();
    session.feed_prompt(&mut batch, prompt_tokens, true)?;
    ctx.decode(&mut batch)?;

    let mut sampler = Sampler::new(&config.sampling, Some(tokenizer.vocab_ptr()))?;
    let mut n_gen = 0;

    loop {
        let next_token = sampler.sample(ctx, -1);
        sampler.accept(next_token);

        let piece = tokenizer.decode_incremental(&mut utf8_pending, next_token)?;
        if !piece.is_empty() && !callback(next_token, piece) {
            break;
        }

        if let Some(max) = config.max_tokens {
            n_gen += 1;
            if n_gen >= max {
                break;
            }
        }

        if crate::llm::tokenizer::special::is_end_of_turn(next_token) {
            break;
        }

        batch.clear();
        session.feed_token(&mut batch, next_token, true)?;

        ctx.decode(&mut batch)?;
    }

    Ok(())
}
