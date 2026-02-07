use super::token::Token;

/// Standard special tokens.
pub const TOKEN_BOS: Token = Token(1);
pub const TOKEN_EOS: Token = Token(2);
pub const TOKEN_NL: Token = Token(13); // Often newline

// Common end-of-turn tokens for chat models (Gemma, etc)
// Ideally tracked per model, but widely useful constants here
pub const TOKEN_EOT_GEMMA: Token = Token(107);

pub fn is_end_of_turn(t: Token) -> bool {
    t == TOKEN_EOS || t == TOKEN_EOT_GEMMA
}
