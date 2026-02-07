pub mod state;

use crate::llm::batch::Batch;
use crate::llm::error::Result;
use crate::llm::tokenizer::Token;
use state::SessionState;

pub struct Session {
    pub state: SessionState,
}

impl Session {
    pub fn new(seq_id: i32) -> Self {
        Self {
            state: SessionState::new(seq_id),
        }
    }

    
    pub fn feed_prompt(&mut self, batch: &mut Batch, tokens: &[Token], logits_last: bool) -> Result<()> {
        for (i, &tok) in tokens.iter().enumerate() {
            let is_last = i == tokens.len() - 1;
            let want_logits = is_last && logits_last;
            
            batch.add_seq(
                tok.0,
                self.state.pos,
                self.state.seq_id,
                want_logits
            )?;
            
            self.state.pos += 1;
            self.state.history.push(tok);
        }
        Ok(())
    }

    pub fn feed_token(&mut self, batch: &mut Batch, token: Token, logits: bool) -> Result<()> {
        batch.add_seq(
            token.0,
            self.state.pos,
            self.state.seq_id,
            logits
        )?;
        self.state.pos += 1;
        self.state.history.push(token);
        Ok(())
    }
}
