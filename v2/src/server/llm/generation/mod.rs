pub mod config;
pub mod stop;

use crate::llm::batch::Batch;
use crate::llm::context::Context;
use crate::llm::error::Result;
use crate::llm::sampling::Sampler;
use crate::llm::session::Session;
use crate::llm::tokenizer::{Token, Tokenizer};
pub use config::GenerationConfig;

/// Helper to run a generation loop.
/// This is a simplified synchronous blocking generator.
pub fn generate_simple(
    ctx: &mut Context,
    session: &mut Session,
    tokenizer: &Tokenizer,
    config: &GenerationConfig,
    prompt_tokens: &[Token],
    mut callback: impl FnMut(Token, String) -> bool, // returns true to continue
) -> Result<()> {
    let mut batch = Batch::new(2048, 0, 1); // capacity, embd (unused), seq_max
    
    // 1. Feed prompt
    batch.clear();
    session.feed_prompt(&mut batch, prompt_tokens, true)?;
    ctx.decode(&mut batch)?;

    let mut sampler = Sampler::new(&config.sampling, Some(tokenizer.vocab_ptr()))?;
    let mut n_gen = 0;
    
    loop {
        // 2. Sample next token (logits are from the last token of prompt or previous step)
        let next_token = sampler.sample(ctx, -1);
        sampler.accept(next_token);

        let piece = tokenizer.decode(&[next_token])?;
        if !callback(next_token, piece) {
            break;
        }

        // Check stop limits
        if let Some(max) = config.max_tokens {
            n_gen += 1;
            if n_gen >= max {
                break;
            }
        }
        
        if crate::llm::tokenizer::special::is_end_of_turn(next_token) {
             break;
        }
        
        // 3. Feed next token
        batch.clear();
        session.feed_token(&mut batch, next_token, true)?;
        
        ctx.decode(&mut batch)?;
    }
    
    Ok(())
}
