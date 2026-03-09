use crate::llm::multimodal::Chunk;
use crate::llm::sampling::SamplingConfig;
use crate::llm::tokenizer::Token;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, atomic::AtomicBool};

#[derive(Debug, Clone)]
pub struct ChunkMeta {
    pub start: usize,
    pub n_tokens: usize,
    pub n_pos: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestState {
    Preparing,
    Waiting,
    Processing,
    Finished,
}

#[derive(Debug, Clone, Default)]
pub struct ThinkTagMarkers {
    pub open: Vec<Token>,
    pub close: Vec<Token>,
}

impl ThinkTagMarkers {
    pub fn has_open(&self) -> bool {
        !self.open.is_empty()
    }

    pub fn has_close(&self) -> bool {
        !self.close.is_empty()
    }

    fn max_len(&self) -> usize {
        self.open.len().max(self.close.len())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThinkingBudgetEvent {
    EnteredThinking { budget: u32 },
    RemainingCheckpoint { remaining: u32 },
    RateLimitReached,
    CollapseThinking,
}

#[derive(Debug, Clone)]
pub struct ThinkingBudgetState {
    in_thinking: bool,
    think_token_count: usize,
    fallback_started: bool,
    reported_remaining_200: bool,
    reported_remaining_100: bool,
    recent_tokens: VecDeque<Token>,
    forced_close_tokens: VecDeque<Token>,
}

impl Default for ThinkingBudgetState {
    fn default() -> Self {
        Self {
            in_thinking: false,
            think_token_count: 0,
            fallback_started: false,
            reported_remaining_200: false,
            reported_remaining_100: false,
            recent_tokens: VecDeque::new(),
            forced_close_tokens: VecDeque::new(),
        }
    }
}

impl ThinkingBudgetState {
    pub fn pop_forced_close_token(&mut self) -> Option<Token> {
        self.forced_close_tokens.pop_front()
    }

    fn reset_session_markers(&mut self) {
        self.reported_remaining_200 = false;
        self.reported_remaining_100 = false;
    }

    pub fn observe_generated_token(
        &mut self,
        token: Token,
        thinking_budget_tokens: Option<u32>,
        markers: &ThinkTagMarkers,
        fallback_without_open: bool,
    ) -> Vec<ThinkingBudgetEvent> {
        let mut events = Vec::new();
        let Some(budget) = thinking_budget_tokens else {
            return events;
        };
        if !markers.has_close() {
            return events;
        }

        let window = markers.max_len();
        if window == 0 {
            return events;
        }

        self.recent_tokens.push_back(token);
        while self.recent_tokens.len() > window {
            self.recent_tokens.pop_front();
        }

        if markers.has_close() && self.ends_with(&markers.close) {
            self.in_thinking = false;
            self.think_token_count = 0;
            self.reset_session_markers();
            return events;
        }

        if !self.in_thinking && markers.has_open() && self.ends_with(&markers.open) {
            events.push(ThinkingBudgetEvent::EnteredThinking { budget });
            self.reset_session_markers();
            if budget == 0 {
                self.in_thinking = false;
                self.think_token_count = 0;
                self.forced_close_tokens = markers.close.iter().copied().collect();
                events.push(ThinkingBudgetEvent::RateLimitReached);
                events.push(ThinkingBudgetEvent::CollapseThinking);
                return events;
            }
            self.in_thinking = true;
            self.think_token_count = 0;
            return events;
        }

        if !self.in_thinking && fallback_without_open && !self.fallback_started {
            self.in_thinking = true;
            self.think_token_count = 0;
            self.fallback_started = true;
            self.reset_session_markers();
            events.push(ThinkingBudgetEvent::EnteredThinking { budget });
            if budget == 0 {
                self.in_thinking = false;
                self.forced_close_tokens = markers.close.iter().copied().collect();
                events.push(ThinkingBudgetEvent::RateLimitReached);
                events.push(ThinkingBudgetEvent::CollapseThinking);
                return events;
            }
        }

        if self.in_thinking {
            self.think_token_count = self.think_token_count.saturating_add(1);
            let remaining = budget.saturating_sub(self.think_token_count as u32);
            if budget >= 200 && !self.reported_remaining_200 && remaining <= 200 {
                self.reported_remaining_200 = true;
                events.push(ThinkingBudgetEvent::RemainingCheckpoint { remaining: 200 });
            }
            if budget >= 100 && !self.reported_remaining_100 && remaining <= 100 {
                self.reported_remaining_100 = true;
                events.push(ThinkingBudgetEvent::RemainingCheckpoint { remaining: 100 });
            }
            if self.think_token_count >= budget as usize {
                self.in_thinking = false;
                self.think_token_count = 0;
                self.forced_close_tokens = markers.close.iter().copied().collect();
                self.reset_session_markers();
                events.push(ThinkingBudgetEvent::RateLimitReached);
                events.push(ThinkingBudgetEvent::CollapseThinking);
                return events;
            }
        }

        events
    }

    fn ends_with(&self, pattern: &[Token]) -> bool {
        if pattern.is_empty() || pattern.len() > self.recent_tokens.len() {
            return false;
        }
        self.recent_tokens
            .iter()
            .skip(self.recent_tokens.len() - pattern.len())
            .zip(pattern.iter())
            .all(|(a, b)| a == b)
    }
}

#[derive(Debug, Clone)]
pub struct RequestParams {
    pub id: String,
    pub prompt: String,
    pub prompt_tokens_override: Option<Vec<Token>>,
    pub images: Vec<Vec<u8>>,
    pub sampling: SamplingConfig,

    pub max_output_tokens: i32,
    pub n_keep: usize,
    pub n_discard: usize,
    pub session_id: Option<String>,
    pub parent_id: Option<String>,
    pub thinking_budget_tokens: Option<u32>,
    pub enable_thinking: bool,
    pub embedding: bool,
}

impl Default for RequestParams {
    fn default() -> Self {
        Self {
            id: String::new(),
            prompt: String::new(),
            prompt_tokens_override: None,
            images: Vec::new(),
            sampling: SamplingConfig::default(),
            max_output_tokens: 32_000,
            n_keep: 0,
            n_discard: 0,
            session_id: None,
            parent_id: None,
            thinking_budget_tokens: None,
            enable_thinking: false,
            embedding: false,
        }
    }
}

pub struct Request {
    pub params: RequestParams,
    pub state: RequestState,
    pub cancel_flag: Arc<AtomicBool>,
    pub(crate) prompt_tokens: Vec<Token>,
    pub(crate) multimodal_chunks: HashMap<usize, Chunk>,
    pub(crate) multimodal_meta: Vec<ChunkMeta>,
    pub(crate) pending_mm_start: Option<usize>,

    pub(crate) generated_tokens: Vec<Token>,
    pub(crate) utf8_buffer: VecDeque<u8>,
    pub(crate) pos_offset: usize,
    pub(crate) thinking_state: ThinkingBudgetState,
    pub response_tx: Option<tokio::sync::mpsc::UnboundedSender<crate::engine::types::Event>>,
}

impl Request {
    pub fn new(params: RequestParams) -> Self {
        Self {
            params,
            state: RequestState::Waiting,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            prompt_tokens: Vec::new(),
            multimodal_chunks: HashMap::new(),
            multimodal_meta: Vec::new(),
            pending_mm_start: None,
            generated_tokens: Vec::new(),
            utf8_buffer: VecDeque::new(),
            pos_offset: 0,
            thinking_state: ThinkingBudgetState::default(),
            response_tx: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ThinkTagMarkers, ThinkingBudgetEvent, ThinkingBudgetState};
    use crate::llm::tokenizer::Token;

    #[test]
    fn forces_close_sequence_when_budget_is_reached() {
        let markers = ThinkTagMarkers {
            open: vec![Token(10), Token(11)],
            close: vec![Token(20), Token(21)],
        };
        let mut state = ThinkingBudgetState::default();

        assert!(
            state
                .observe_generated_token(Token(10), Some(2), &markers, false)
                .is_empty()
        );
        assert_eq!(
            state.observe_generated_token(Token(11), Some(2), &markers, false),
            vec![ThinkingBudgetEvent::EnteredThinking { budget: 2 }]
        );
        assert!(
            state
                .observe_generated_token(Token(42), Some(2), &markers, false)
                .is_empty()
        );
        let events = state.observe_generated_token(Token(43), Some(2), &markers, false);
        assert_eq!(
            events,
            vec![
                ThinkingBudgetEvent::RateLimitReached,
                ThinkingBudgetEvent::CollapseThinking
            ]
        );
        assert_eq!(state.pop_forced_close_token(), Some(Token(20)));
        assert_eq!(state.pop_forced_close_token(), Some(Token(21)));
        assert_eq!(state.pop_forced_close_token(), None);
    }

    #[test]
    fn budget_zero_forces_close_immediately_after_open_tag() {
        let markers = ThinkTagMarkers {
            open: vec![Token(1), Token(2)],
            close: vec![Token(3), Token(4)],
        };
        let mut state = ThinkingBudgetState::default();

        assert!(
            state
                .observe_generated_token(Token(1), Some(0), &markers, false)
                .is_empty()
        );
        let events = state.observe_generated_token(Token(2), Some(0), &markers, false);
        assert_eq!(
            events,
            vec![
                ThinkingBudgetEvent::EnteredThinking { budget: 0 },
                ThinkingBudgetEvent::RateLimitReached,
                ThinkingBudgetEvent::CollapseThinking
            ]
        );
        assert_eq!(state.pop_forced_close_token(), Some(Token(3)));
        assert_eq!(state.pop_forced_close_token(), Some(Token(4)));
    }

    #[test]
    fn natural_close_does_not_queue_forced_tokens() {
        let markers = ThinkTagMarkers {
            open: vec![Token(5)],
            close: vec![Token(6), Token(7)],
        };
        let mut state = ThinkingBudgetState::default();

        assert_eq!(
            state.observe_generated_token(Token(5), Some(8), &markers, false),
            vec![ThinkingBudgetEvent::EnteredThinking { budget: 8 }]
        );
        assert!(
            state
                .observe_generated_token(Token(100), Some(8), &markers, false)
                .is_empty()
        );
        assert!(
            state
                .observe_generated_token(Token(6), Some(8), &markers, false)
                .is_empty()
        );
        assert!(
            state
                .observe_generated_token(Token(7), Some(8), &markers, false)
                .is_empty()
        );
        assert_eq!(state.pop_forced_close_token(), None);
    }

    #[test]
    fn fallback_without_open_enforces_budget() {
        let markers = ThinkTagMarkers {
            open: vec![],
            close: vec![Token(9), Token(10)],
        };
        let mut state = ThinkingBudgetState::default();

        let events = state.observe_generated_token(Token(100), Some(2), &markers, true);
        assert_eq!(
            events,
            vec![ThinkingBudgetEvent::EnteredThinking { budget: 2 }]
        );
        let events = state.observe_generated_token(Token(101), Some(2), &markers, true);
        assert_eq!(
            events,
            vec![
                ThinkingBudgetEvent::RateLimitReached,
                ThinkingBudgetEvent::CollapseThinking
            ]
        );
        assert_eq!(state.pop_forced_close_token(), Some(Token(9)));
        assert_eq!(state.pop_forced_close_token(), Some(Token(10)));
    }

    #[test]
    fn emits_remaining_checkpoints() {
        let markers = ThinkTagMarkers {
            open: vec![Token(1)],
            close: vec![Token(2)],
        };
        let mut state = ThinkingBudgetState::default();
        assert_eq!(
            state.observe_generated_token(Token(1), Some(256), &markers, false),
            vec![ThinkingBudgetEvent::EnteredThinking { budget: 256 }]
        );
        for _ in 0..55 {
            let _ = state.observe_generated_token(Token(42), Some(256), &markers, false);
        }
        assert_eq!(
            state.observe_generated_token(Token(42), Some(256), &markers, false),
            vec![ThinkingBudgetEvent::RemainingCheckpoint { remaining: 200 }]
        );
        for _ in 0..99 {
            let _ = state.observe_generated_token(Token(42), Some(256), &markers, false);
        }
        assert_eq!(
            state.observe_generated_token(Token(42), Some(256), &markers, false),
            vec![ThinkingBudgetEvent::RemainingCheckpoint { remaining: 100 }]
        );
    }
}
