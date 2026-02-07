use crate::llm::tokenizer::{Token, special};

pub trait StopCondition {
    fn should_stop(&mut self, token: Token) -> bool;
}

pub struct EosStopCondition;

impl StopCondition for EosStopCondition {
    fn should_stop(&mut self, token: Token) -> bool {
        special::is_end_of_turn(token)
    }
}
