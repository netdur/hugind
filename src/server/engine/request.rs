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

#[derive(Debug, Clone)]
pub struct RequestParams {
    pub id: String, 
    pub prompt: String,
    pub images: Vec<Vec<u8>>, 
    pub sampling: SamplingConfig,
    
    pub max_output_tokens: i32,
    pub n_keep: usize,
    pub n_discard: usize,
    pub session_id: Option<String>,
    pub parent_id: Option<String>,
    pub embedding: bool,
}

impl Default for RequestParams {
    fn default() -> Self {
        Self {
            id: String::new(),
            prompt: String::new(),
            images: Vec::new(),
            sampling: SamplingConfig::default(),
            max_output_tokens: 32_000,
            n_keep: 0,
            n_discard: 0,
            session_id: None,
            parent_id: None,
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
    pub(crate) _buffer: VecDeque<u8>, 
    pub(crate) pos_offset: usize, 
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
            _buffer: VecDeque::new(),
            pos_offset: 0,
            response_tx: None,
        }
    }
}
