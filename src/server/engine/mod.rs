pub mod kv_cache;
pub mod queue;
pub mod request;
pub mod types;

use crate::llm::batch::Batch;
use crate::llm::context::{Context, ContextParams};
use crate::llm::error::Result;
use crate::llm::model::Model;
use crate::llm::multimodal::{Chunk, Image, MultimodalContext};
use crate::llm::sampling::Sampler;
use crate::llm::tokenizer::{Token, Tokenizer};
use request::{ChunkMeta, Request, RequestState, ThinkTagMarkers};
use types::{Event, EventKind, RequestHandle, StopReason};

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::{Arc, atomic::Ordering, mpsc};
use std::thread;
use std::time::Instant;
use uuid::Uuid;

struct PrepJob {
    request_id: String,
    prompt: String,
    images: Vec<Vec<u8>>,
}

struct PreparedMultimodal {
    prompt_tokens: Vec<Token>,
    multimodal_chunks: HashMap<usize, Chunk>,
    multimodal_meta: Vec<ChunkMeta>,
}

struct PrepResult {
    request_id: String,
    result: std::result::Result<PreparedMultimodal, String>,
}

struct Slot {
    request_id: String,
    sampler: Sampler,
    n_decoded: usize,
    n_prompt_processed: usize,
    sample_from_cache: bool,
}

pub struct LlmEngine<'a> {
    _model: &'a Model,
    ctx: Context,
    mmproj: Option<Arc<MultimodalContext>>,
    tokenizer: Tokenizer<'a>,
    requests: HashMap<String, Request>,
    slots: HashMap<i32, Slot>,
    n_seq_max: i32,
    n_batch: usize,
    n_ubatch: usize,
    n_ctx: usize,
    batch: Batch,
    pending_events: Vec<Event>,

    pub input_rx: tokio::sync::mpsc::Receiver<Request>,
    pub request_queue: crate::engine::queue::RequestQueue,
    pub kv_manager: Arc<crate::engine::kv_cache::KvCacheManager>,
    pub engine_stats: Arc<RwLock<EngineStats>>,

    prep_tx: Option<mpsc::Sender<PrepJob>>,
    prep_rx: mpsc::Receiver<PrepResult>,
    _prep_handle: Option<thread::JoinHandle<()>>,
    embeddings_mode: bool,
    trace_flow: bool,
    trace_flow_verbose: bool,
    last_flow_pull_start: Option<(usize, usize, usize)>,
    last_flow_post_schedule: Option<(usize, usize, usize)>,
    last_flow_batch_built: Option<(i32, usize)>,
    eval_diag_request_logs: u32,
    eval_diag_encode_logs: u32,
    eval_diag_decode_logs: u32,
    eval_diag_decode_unexpected_logs: u32,
    think_tag_markers: ThinkTagMarkers,
}

#[derive(Debug, Default, Clone)]
pub struct EngineStats {
    pub requests_processing: usize,
    pub requests_waiting: usize,
    pub slots_active: usize,
    pub slots_total: usize,
    pub tokens_per_sec_total: f64,
    pub tokens_per_sec_per_active: f64,
    pub tps_ema: f64,
    pub last_tps_at: Option<Instant>,
}

impl<'a> LlmEngine<'a> {
    fn flow_mode() -> (bool, bool) {
        let Some(value) = std::env::var("HUGIND_ENGINE_TRACE").ok() else {
            return (false, false);
        };
        let value = value.trim().to_ascii_lowercase();
        if matches!(value.as_str(), "debug" | "verbose" | "all") {
            return (true, true);
        }
        if matches!(value.as_str(), "1" | "true" | "yes" | "on" | "important") {
            return (true, false);
        }
        (false, false)
    }

    fn important_flow_message(msg: &str) -> bool {
        msg.starts_with("push error")
            || msg.starts_with("prep failed")
            || msg.starts_with("schedule pause")
    }

    fn flow_log(&self, msg: impl AsRef<str>) {
        if !self.trace_flow {
            return;
        }
        let msg = msg.as_ref();
        if self.trace_flow_verbose || Self::important_flow_message(msg) {
            eprintln!("[Flow] {}", msg);
        }
    }

    fn flow_log_pull_start(&mut self) {
        if !self.trace_flow_verbose {
            return;
        }
        let snapshot = (
            self.requests.len(),
            self.slots.len(),
            self.request_queue.len(),
        );
        if self.last_flow_pull_start != Some(snapshot) {
            eprintln!(
                "[Flow] pull start requests={} slots={} queue={}",
                snapshot.0, snapshot.1, snapshot.2
            );
            self.last_flow_pull_start = Some(snapshot);
        }
    }

    fn flow_log_post_schedule(&mut self) {
        if !self.trace_flow_verbose {
            return;
        }
        let snapshot = (
            self.requests.len(),
            self.slots.len(),
            self.request_queue.len(),
        );
        if self.last_flow_post_schedule != Some(snapshot) {
            eprintln!(
                "[Flow] post-schedule requests={} slots={} queue={}",
                snapshot.0, snapshot.1, snapshot.2
            );
            self.last_flow_post_schedule = Some(snapshot);
        }
    }

    fn flow_log_batch_built(&mut self, n_tokens: i32, tracked_logits_seqs: usize) {
        if !self.trace_flow_verbose {
            return;
        }
        let snapshot = (n_tokens, tracked_logits_seqs);
        if self.last_flow_batch_built != Some(snapshot) {
            eprintln!(
                "[Flow] batch built n_tokens={} tracked_logits_seqs={}",
                snapshot.0, snapshot.1
            );
            self.last_flow_batch_built = Some(snapshot);
        }
    }

    fn log_decode_unexpected(
        &mut self,
        slot_batch_idx: &HashMap<i32, i32>,
        batch_embedding_mode: Option<bool>,
        batch_has_embedding: bool,
    ) {
        if self.eval_diag_decode_unexpected_logs >= 32 {
            return;
        }

        let model_has_encoder = self._model.has_encoder();
        let model_has_decoder = self._model.has_decoder();

        let mut seq_items: Vec<(i32, i32)> = slot_batch_idx
            .iter()
            .map(|(&seq_id, &batch_idx)| (seq_id, batch_idx))
            .collect();
        seq_items.sort_by_key(|(seq_id, _)| *seq_id);

        let mut seq_summaries = Vec::new();
        for (seq_id, batch_idx) in seq_items.into_iter().take(8) {
            if let Some(slot) = self.slots.get(&seq_id) {
                if let Some(req) = self.requests.get(&slot.request_id) {
                    let req_short = req.params.id.chars().take(8).collect::<String>();
                    seq_summaries.push(format!(
                        "seq={} batch_idx={} req={} state={:?} emb={} prompt_processed={}/{} generated={}",
                        seq_id,
                        batch_idx,
                        req_short,
                        req.state,
                        req.params.embedding,
                        slot.n_prompt_processed,
                        req.prompt_tokens.len(),
                        req.generated_tokens.len()
                    ));
                }
            }
        }
        let seq_summary = if seq_summaries.is_empty() {
            "none".to_string()
        } else {
            seq_summaries.join(" | ")
        };

        eprintln!(
            "[EvalDiag] decode-unexpected cfg_embeddings={} model_has_encoder={} model_has_decoder={} batch_mode={:?} batch_has_embedding={} n_tokens={} tracked_logits_seqs={} active={}",
            self.embeddings_mode,
            model_has_encoder,
            model_has_decoder,
            batch_embedding_mode,
            batch_has_embedding,
            self.batch.handle.n_tokens,
            slot_batch_idx.len(),
            seq_summary
        );

        self.eval_diag_decode_unexpected_logs += 1;
        if self.eval_diag_decode_unexpected_logs == 32 {
            eprintln!("[EvalDiag] decode-unexpected further logs suppressed");
        }
    }

    fn tokenize_tag(tokenizer: &Tokenizer<'_>, tag: &str) -> Vec<Token> {
        for parse_special in [true, false] {
            if let Ok(tokens) = tokenizer.tokenize(tag, false, parse_special) {
                if !tokens.is_empty() {
                    return tokens;
                }
            }
        }
        Vec::new()
    }

    fn detect_think_tag_markers(tokenizer: &Tokenizer<'_>) -> ThinkTagMarkers {
        let open = Self::tokenize_tag(tokenizer, "<think>");
        let close = Self::tokenize_tag(tokenizer, "</think>");
        ThinkTagMarkers { open, close }
    }

    pub fn new(
        model: &'a Model,
        ctx_params: &ContextParams,
        mmproj_path: Option<&str>,
        input_rx: tokio::sync::mpsc::Receiver<Request>,
        kv_manager: Arc<crate::engine::kv_cache::KvCacheManager>,
        engine_stats: Arc<RwLock<EngineStats>>,
    ) -> Result<Self> {
        let ctx = Context::new(model, ctx_params)?;
        let tokenizer = model.tokenizer();
        let batch = Batch::new(ctx_params.n_batch as i32, 0, ctx_params.n_seq_max as i32);
        let think_tag_markers = Self::detect_think_tag_markers(&tokenizer);

        let mmproj = if let Some(path) = mmproj_path {
            Some(Arc::new(MultimodalContext::from_file(path, model)?))
        } else {
            None
        };

        let (prep_tx, prep_job_rx) = mpsc::channel::<PrepJob>();
        let (prep_result_tx, prep_rx) = mpsc::channel::<PrepResult>();
        let (trace_flow, trace_flow_verbose) = Self::flow_mode();
        let embeddings_mode = ctx_params.embeddings;

        eprintln!(
            "[EvalDiag] init cfg_embeddings={} model_has_encoder={} model_has_decoder={} n_ctx={} n_batch={} n_ubatch={} n_seq_max={}",
            embeddings_mode,
            model.has_encoder(),
            model.has_decoder(),
            ctx_params.n_ctx,
            ctx_params.n_batch,
            ctx_params.n_ubatch,
            ctx_params.n_seq_max
        );

        let worker_mmproj = mmproj.clone();
        let mm_debug = std::env::var("HUGIND_LLAMA_DEBUG")
            .ok()
            .map(|v| {
                let v = v.trim().to_ascii_lowercase();
                matches!(v.as_str(), "1" | "true" | "yes" | "on" | "debug")
            })
            .unwrap_or(false);
        let _prep_handle = Some(thread::spawn(move || {
            while let Ok(job) = prep_job_rx.recv() {
                let result = match &worker_mmproj {
                    Some(mmctx) => {
                        let mut images = Vec::new();
                        let mut load_error: Option<String> = None;
                        for img_data in &job.images {
                            match Image::from_bytes(mmctx, img_data) {
                                Ok(img) => images.push(img),
                                Err(e) => {
                                    load_error = Some(format!("Failed to load image: {}", e));
                                    break;
                                }
                            }
                        }

                        if let Some(err) = load_error {
                            Err(err)
                        } else {
                            match mmctx.tokenize(&job.prompt, &images) {
                                Ok((tokens, chunks)) => {
                                    let prompt_tokens = tokens.into_iter().map(Token).collect();
                                    let multimodal_chunks = chunks;
                                    let mut multimodal_meta: Vec<ChunkMeta> = multimodal_chunks
                                        .iter()
                                        .map(|(start, chunk)| ChunkMeta {
                                            start: *start,
                                            n_tokens: chunk.n_tokens(),
                                            n_pos: chunk.n_pos(),
                                        })
                                        .collect();
                                    multimodal_meta.sort_by_key(|meta| meta.start);

                                    Ok(PreparedMultimodal {
                                        prompt_tokens,
                                        multimodal_chunks,
                                        multimodal_meta,
                                    })
                                }
                                Err(e) => {
                                    let media_marker_count =
                                        job.prompt.matches("<__media__>").count();
                                    let deprecated_marker_count =
                                        job.prompt.matches("<__image__>").count();
                                    if mm_debug {
                                        let snippet: String =
                                            job.prompt.chars().take(300).collect();
                                        eprintln!(
                                            "[MM DEBUG] tokenize failed: images={}, <__media__>={}, <__image__>={}, prompt_chars={}, prompt_prefix={:?}",
                                            images.len(),
                                            media_marker_count,
                                            deprecated_marker_count,
                                            job.prompt.chars().count(),
                                            snippet
                                        );
                                    }
                                    Err(format!(
                                        "Multimodal tokenization failed: {} (images={}, <__media__> markers={}, <__image__> markers={}, prompt_chars={})",
                                        e,
                                        images.len(),
                                        media_marker_count,
                                        deprecated_marker_count,
                                        job.prompt.chars().count()
                                    ))
                                }
                            }
                        }
                    }
                    None => Err("Multimodal context not available".to_string()),
                };

                let _ = prep_result_tx.send(PrepResult {
                    request_id: job.request_id,
                    result,
                });
            }
        }));

        Ok(Self {
            _model: model,
            ctx,
            mmproj,
            tokenizer,
            requests: HashMap::new(),
            slots: HashMap::new(),
            n_seq_max: ctx_params.n_seq_max as i32,
            n_batch: ctx_params.n_batch as usize,
            n_ubatch: ctx_params.n_ubatch as usize,
            n_ctx: ctx_params.n_ctx as usize,
            batch,
            pending_events: Vec::new(),
            input_rx,
            request_queue: crate::engine::queue::RequestQueue::new(None),
            kv_manager,
            engine_stats,
            prep_tx: Some(prep_tx),
            prep_rx,
            _prep_handle,
            embeddings_mode,
            trace_flow,
            trace_flow_verbose,
            last_flow_pull_start: None,
            last_flow_post_schedule: None,
            last_flow_batch_built: None,
            eval_diag_request_logs: 0,
            eval_diag_encode_logs: 0,
            eval_diag_decode_logs: 0,
            eval_diag_decode_unexpected_logs: 0,
            think_tag_markers,
        })
    }

    pub fn push(&mut self, request: Request) -> String {
        let id = if request.params.id.is_empty() {
            Uuid::new_v4().to_string()
        } else {
            request.params.id.clone()
        };

        let mut req = request;
        req.params.id = id.clone();
        if req.params.embedding != self.embeddings_mode && self.eval_diag_request_logs < 32 {
            eprintln!(
                "[EvalDiag] request-mode-mismatch request={} req_embedding={} cfg_embeddings={} state={:?} prompt_override_tokens={} prompt_chars={} images={}",
                id,
                req.params.embedding,
                self.embeddings_mode,
                req.state,
                req.params
                    .prompt_tokens_override
                    .as_ref()
                    .map(|tokens| tokens.len())
                    .unwrap_or(0),
                req.params.prompt.chars().count(),
                req.params.images.len()
            );
            self.eval_diag_request_logs += 1;
            if self.eval_diag_request_logs == 32 {
                eprintln!("[EvalDiag] request-mode-mismatch further logs suppressed");
            }
        }
        self.flow_log(format!(
            "push request={} session_id={:?} parent_id={:?} prompt_chars={} images={}",
            id,
            req.params.session_id,
            req.params.parent_id,
            req.params.prompt.chars().count(),
            req.params.images.len()
        ));
        let cancel_flag = req.cancel_flag.clone();
        let response_tx = req.response_tx.clone();
        let mut submission_error = None;

        if req.params.embedding != self.embeddings_mode {
            submission_error = Some(if self.embeddings_mode {
                "Server is running in embedding mode; non-embedding requests are rejected"
                    .to_string()
            } else {
                "Server is not running in embedding mode; embedding requests are rejected"
                    .to_string()
            });
        }

        if submission_error.is_none() && self.mmproj.is_some() && !req.params.images.is_empty() {
            req.state = RequestState::Preparing;
            let job = PrepJob {
                request_id: id.clone(),
                prompt: req.params.prompt.clone(),
                images: std::mem::take(&mut req.params.images),
            };

            self.requests.insert(id.clone(), req);

            match &self.prep_tx {
                Some(tx) => {
                    if tx.send(job).is_err() {
                        submission_error =
                            Some("Failed to enqueue multimodal preparation".to_string());
                        self.requests.remove(&id);
                    }
                }
                None => {
                    submission_error = Some("Prep worker unavailable".to_string());
                    self.requests.remove(&id);
                }
            }
        } else if submission_error.is_none() {
            if let Some(tokens) = req.params.prompt_tokens_override.take() {
                req.prompt_tokens = tokens;
                req.state = RequestState::Waiting;
                self.flow_log(format!(
                    "push pretokenized request={} prompt_tokens={}",
                    id,
                    req.prompt_tokens.len()
                ));
                self.requests.insert(id.clone(), req);
                if let Err(msg) = self.request_queue.push(id.clone()) {
                    submission_error = Some(msg);
                    self.requests.remove(&id);
                }
            } else {
                match self.tokenizer.tokenize(&req.params.prompt, true, true) {
                    Ok(tokens) => {
                        req.prompt_tokens = tokens;
                        req.state = RequestState::Waiting;
                        self.flow_log(format!(
                            "push tokenized request={} prompt_tokens={}",
                            id,
                            req.prompt_tokens.len()
                        ));
                        self.requests.insert(id.clone(), req);
                        if let Err(msg) = self.request_queue.push(id.clone()) {
                            submission_error = Some(msg);
                            self.requests.remove(&id);
                        }
                    }
                    Err(_) => {
                        submission_error = Some("Tokenization failed".to_string());
                    }
                }
            }
        }

        if let Some(err_msg) = submission_error {
            self.flow_log(format!("push error request={} message={}", id, err_msg));
            let event = Event {
                id: Uuid::new_v4().to_string(),
                kind: EventKind::Error {
                    message: err_msg,
                    request: RequestHandle::new(id.clone(), cancel_flag),
                },
            };

            if let Some(tx) = &response_tx {
                let _ = tx.send(event.clone());
            }
            self.pending_events.push(event);
        }
        id
    }

    pub fn is_active(&self) -> bool {
        !self.requests.is_empty() || !self.pending_events.is_empty()
    }

    pub fn pull(&mut self) -> Result<Vec<Event>> {
        let mut events = Vec::new();
        self.flow_log_pull_start();

        while let Ok(req) = self.input_rx.try_recv() {
            self.push(req);
        }

        events.append(&mut self.pending_events);

        let mut abort_seqs = Vec::new();
        let trace_flow = self.trace_flow;
        let trace_flow_verbose = self.trace_flow_verbose;

        for prep in self.prep_rx.try_iter() {
            if let Some(req) = self.requests.get_mut(&prep.request_id) {
                match prep.result {
                    Ok(prepared) => {
                        req.prompt_tokens = prepared.prompt_tokens;
                        req.multimodal_chunks = prepared.multimodal_chunks;
                        req.multimodal_meta = prepared.multimodal_meta;
                        req.state = RequestState::Waiting;
                        if trace_flow_verbose {
                            eprintln!(
                                "[Flow] prep ready request={} prompt_tokens={} mm_chunks={}",
                                prep.request_id,
                                req.prompt_tokens.len(),
                                req.multimodal_meta.len()
                            );
                        }
                        if let Err(msg) = self.request_queue.push(prep.request_id.clone()) {
                            req.state = RequestState::Finished;
                            Self::emit_event(
                                &mut events,
                                req,
                                EventKind::Error {
                                    message: msg,
                                    request: RequestHandle::new(
                                        prep.request_id.clone(),
                                        req.cancel_flag.clone(),
                                    ),
                                },
                            );
                            self.requests.remove(&prep.request_id);
                        }
                    }
                    Err(msg) => {
                        if trace_flow {
                            eprintln!(
                                "[Flow] prep failed request={} message={}",
                                prep.request_id, msg
                            );
                        }
                        req.state = RequestState::Finished;
                        Self::emit_event(
                            &mut events,
                            req,
                            EventKind::Error {
                                message: msg,
                                request: RequestHandle::new(
                                    prep.request_id.clone(),
                                    req.cancel_flag.clone(),
                                ),
                            },
                        );
                        self.requests.remove(&prep.request_id);
                    }
                }
            }
        }

        self.process_state_actions();

        self.schedule_requests();
        self.flow_log_post_schedule();

        self.apply_context_shifts(&mut events)?;

        self.batch.clear();

        let mut slot_batch_idx = HashMap::new();
        let mut eval_tokens: HashMap<i32, Vec<Token>> = HashMap::new();
        let mut prefill_progress: HashMap<i32, usize> = HashMap::new();
        let mut batch_embedding_mode: Option<bool> = None;
        {
            let slots = &mut self.slots;
            let requests = &mut self.requests;
            let batch = &mut self.batch;
            let n_batch = self.n_batch;
            let n_ubatch = self.n_ubatch;

            for (&seq_id, slot) in slots.iter_mut() {
                let req = match requests.get_mut(&slot.request_id) {
                    Some(req) => req,
                    None => {
                        eprintln!(
                            "[Step] Missing request {} for slot {}, aborting seq",
                            slot.request_id, seq_id
                        );
                        abort_seqs.push(seq_id);
                        continue;
                    }
                };

                if req.cancel_flag.load(Ordering::Acquire) {
                    req.state = RequestState::Finished;
                    abort_seqs.push(seq_id);
                    Self::emit_event(
                        &mut events,
                        req,
                        EventKind::Finish {
                            request: RequestHandle::new(
                                slot.request_id.clone(),
                                req.cancel_flag.clone(),
                            ),
                            reason: StopReason::Cancelled,
                        },
                    );
                    continue;
                }

                if req.state != RequestState::Processing {
                    continue;
                }
                let req_is_embedding = req.params.embedding;
                if let Some(mode) = batch_embedding_mode {
                    if mode != req_is_embedding {
                        continue;
                    }
                }

                if req.state == RequestState::Processing
                    && slot.n_prompt_processed < req.prompt_tokens.len()
                {
                    if trace_flow_verbose {
                        eprintln!(
                            "[Flow] force waiting request={} seq={} processed={} prompt_len={}",
                            slot.request_id,
                            seq_id,
                            slot.n_prompt_processed,
                            req.prompt_tokens.len()
                        );
                    }
                    req.state = RequestState::Waiting;
                    continue;
                }

                if slot.sample_from_cache {
                    if batch_embedding_mode.is_none() {
                        batch_embedding_mode = Some(req_is_embedding);
                    }
                    if trace_flow_verbose {
                        eprintln!(
                            "[Flow] sample-from-cache request={} seq={}",
                            slot.request_id, seq_id
                        );
                    }
                    slot_batch_idx.insert(seq_id, -1);
                    slot.sample_from_cache = false;
                    continue;
                }

                if batch.handle.n_tokens as usize >= n_batch {
                    break;
                }

                if req.generated_tokens.is_empty() && req.prompt_tokens.is_empty() {
                    Self::emit_event(
                        &mut events,
                        req,
                        EventKind::Error {
                            message: "Processing request has no prompt tokens".to_string(),
                            request: RequestHandle::new(
                                slot.request_id.clone(),
                                req.cancel_flag.clone(),
                            ),
                        },
                    );
                    req.state = RequestState::Finished;
                    abort_seqs.push(seq_id);
                    continue;
                }

                let last_tok = match req.generated_tokens.last() {
                    Some(tok) => {
                        eval_tokens.entry(seq_id).or_default().push(*tok);
                        tok
                    }
                    None => req.prompt_tokens.last().unwrap(),
                };
                if batch_embedding_mode.is_none() {
                    batch_embedding_mode = Some(req_is_embedding);
                }
                let pos_last = Self::pos_for_last_token(req);
                let idx = batch.handle.n_tokens;
                batch.add_seq(last_tok.0, pos_last, seq_id, true)?;
                if trace_flow_verbose {
                    eprintln!(
                        "[Flow] processing add request={} seq={} token={} pos={} idx={}",
                        slot.request_id, seq_id, last_tok.0, pos_last, idx
                    );
                }
                slot_batch_idx.insert(seq_id, idx);
            }

            let batch_limit = n_batch.min(n_ubatch);
            let mut prefill_budget = batch_limit.saturating_sub(batch.handle.n_tokens as usize);
            for (&seq_id, slot) in slots.iter_mut() {
                if prefill_budget == 0 {
                    break;
                }

                let req = match requests.get_mut(&slot.request_id) {
                    Some(req) => req,
                    None => {
                        eprintln!(
                            "[Step] Missing request {} for slot {}, aborting seq",
                            slot.request_id, seq_id
                        );
                        abort_seqs.push(seq_id);
                        continue;
                    }
                };

                if req.cancel_flag.load(Ordering::Acquire) {
                    req.state = RequestState::Finished;
                    abort_seqs.push(seq_id);
                    Self::emit_event(
                        &mut events,
                        req,
                        EventKind::Finish {
                            request: RequestHandle::new(
                                slot.request_id.clone(),
                                req.cancel_flag.clone(),
                            ),
                            reason: StopReason::Cancelled,
                        },
                    );
                    continue;
                }

                if req.state != RequestState::Waiting {
                    continue;
                }
                let req_is_embedding = req.params.embedding;
                if let Some(mode) = batch_embedding_mode {
                    if mode != req_is_embedding {
                        continue;
                    }
                }

                if req.prompt_tokens.is_empty() {
                    Self::emit_event(
                        &mut events,
                        req,
                        EventKind::Error {
                            message: "Waiting request has empty prompt tokens".to_string(),
                            request: RequestHandle::new(
                                slot.request_id.clone(),
                                req.cancel_flag.clone(),
                            ),
                        },
                    );
                    req.state = RequestState::Finished;
                    abort_seqs.push(seq_id);
                    continue;
                }

                if req.pending_mm_start.is_some() {
                    continue;
                }

                let total_len = req.prompt_tokens.len();
                let mut processed = slot.n_prompt_processed;
                if processed >= total_len {
                    req.state = RequestState::Processing;
                    continue;
                }

                while processed < total_len && prefill_budget > 0 {
                    let tok_idx = processed;

                    if req.multimodal_chunks.get(&tok_idx).is_some() {
                        req.pending_mm_start = Some(tok_idx);
                        break;
                    }

                    let tok = req.prompt_tokens[tok_idx];
                    if tok.0 == -1 {
                        processed += 1;
                        continue;
                    }

                    let is_last = tok_idx == total_len - 1;
                    let logits_flag = req.params.embedding || is_last;
                    let pos = Self::pos_for_prompt_index(req, tok_idx);
                    if batch_embedding_mode.is_none() {
                        batch_embedding_mode = Some(req_is_embedding);
                    }
                    let idx = batch.handle.n_tokens;
                    batch.add_seq(tok.0, pos, seq_id, logits_flag)?;
                    if trace_flow_verbose {
                        eprintln!(
                            "[Flow] prefill add request={} seq={} tok_idx={} token={} pos={} logits={}",
                            slot.request_id, seq_id, tok_idx, tok.0, pos, logits_flag
                        );
                    }
                    if is_last {
                        slot_batch_idx.insert(seq_id, idx);
                    }
                    eval_tokens.entry(seq_id).or_default().push(tok);
                    processed += 1;
                    prefill_budget = prefill_budget.saturating_sub(1);
                }

                prefill_progress.insert(seq_id, processed);
            }
        }

        self.flow_log_batch_built(self.batch.handle.n_tokens, slot_batch_idx.len());

        if self.batch.handle.n_tokens == 0 && slot_batch_idx.is_empty() {
            self.eval_one_pending_mm(&mut events, &mut abort_seqs)?;
            for seq_id in abort_seqs.drain(..) {
                if let Some(slot) = self.slots.remove(&seq_id) {
                    self.ctx.kv_cache_seq_rm(seq_id, -1, -1);
                    self.kv_manager.release_sequence(seq_id, None);
                    self.requests.remove(&slot.request_id);
                }
            }
            self.update_stats(0.0, Instant::now());
            return Ok(events);
        }

        if batch_embedding_mode.is_none() {
            batch_embedding_mode = slot_batch_idx.keys().find_map(|seq_id| {
                self.slots
                    .get(seq_id)
                    .and_then(|slot| self.requests.get(&slot.request_id))
                    .map(|req| req.params.embedding)
            });
        }
        let model_has_encoder = self._model.has_encoder();
        let model_has_decoder = self._model.has_decoder();
        let batch_has_embedding = batch_embedding_mode.unwrap_or(false);

        if self.embeddings_mode && batch_has_embedding {
            // In embedding-only mode, we do not preserve conversational KV state.
            // Clearing memory each step mirrors llama.cpp embedding examples and
            // avoids stale sequence state across independent requests.
            self.ctx.memory_clear(true);
        }

        let should_use_encode = batch_has_embedding || (model_has_encoder && !model_has_decoder);

        if self.batch.handle.n_tokens == 0 {
        } else if should_use_encode {
            if self.eval_diag_encode_logs < 16 {
                let (embedding_reqs, generation_reqs) =
                    self.requests
                        .values()
                        .fold((0usize, 0usize), |(e, g), req| {
                            if req.params.embedding {
                                (e + 1, g)
                            } else {
                                (e, g + 1)
                            }
                        });
                eprintln!(
                    "[EvalDiag] branch=encode n_tokens={} slot_batch_idx={} batch_mode={:?} cfg_embeddings={} has_encoder={} has_decoder={} req_embedding={} req_generation={}",
                    self.batch.handle.n_tokens,
                    slot_batch_idx.len(),
                    batch_embedding_mode,
                    self.embeddings_mode,
                    model_has_encoder,
                    model_has_decoder,
                    embedding_reqs,
                    generation_reqs
                );
                self.eval_diag_encode_logs += 1;
                if self.eval_diag_encode_logs == 16 {
                    eprintln!("[EvalDiag] branch=encode further logs suppressed");
                }
            }
            if let Err(e) = self.ctx.encode(&mut self.batch) {
                let can_retry_decode =
                    batch_has_embedding && !model_has_encoder && model_has_decoder;
                if can_retry_decode {
                    eprintln!(
                        "[EvalDiag] encode failed on embedding batch; retrying decode (model_has_encoder={}, model_has_decoder={})",
                        model_has_encoder, model_has_decoder
                    );
                    self.ctx.decode(&mut self.batch)?;
                } else {
                    return Err(e);
                }
            }
        } else {
            let decode_unexpected = self.embeddings_mode
                || batch_has_embedding
                || (model_has_encoder && !model_has_decoder);
            if decode_unexpected {
                self.log_decode_unexpected(
                    &slot_batch_idx,
                    batch_embedding_mode,
                    batch_has_embedding,
                );
            }
            if self.eval_diag_decode_logs < 64 {
                let (embedding_reqs, generation_reqs) =
                    self.requests
                        .values()
                        .fold((0usize, 0usize), |(e, g), req| {
                            if req.params.embedding {
                                (e + 1, g)
                            } else {
                                (e, g + 1)
                            }
                        });
                eprintln!(
                    "[EvalDiag] branch=decode n_tokens={} slot_batch_idx={} batch_mode={:?} batch_has_embedding={} cfg_embeddings={} has_encoder={} has_decoder={} req_embedding={} req_generation={}",
                    self.batch.handle.n_tokens,
                    slot_batch_idx.len(),
                    batch_embedding_mode,
                    batch_has_embedding,
                    self.embeddings_mode,
                    model_has_encoder,
                    model_has_decoder,
                    embedding_reqs,
                    generation_reqs
                );
                self.eval_diag_decode_logs += 1;
                if self.eval_diag_decode_logs == 64 {
                    eprintln!("[EvalDiag] branch=decode further logs suppressed");
                }
            }
            self.flow_log(format!(
                "decode start n_tokens={}",
                self.batch.handle.n_tokens
            ));
            if let Err(e) = self.ctx.decode(&mut self.batch) {
                eprintln!("[Step] Decode FAILED: {}", e);

                {
                    eprintln!("[Step] Batch n_tokens: {}", self.batch.handle.n_tokens);
                    if self.batch.handle.n_tokens > 0
                        && !self.batch.handle.pos.is_null()
                        && !self.batch.handle.seq_id.is_null()
                    {
                        let pos = unsafe { *self.batch.handle.pos.add(0) };
                        let seq = unsafe {
                            let seq_ptr = *self.batch.handle.seq_id.add(0);
                            if seq_ptr.is_null() { -1 } else { *seq_ptr }
                        };
                        eprintln!("[Step] First Token: Pos={}, Seq={}", pos, seq);

                        if let Some(slot) = self.slots.get(&seq) {
                            if let Some(req) = self.requests.get(&slot.request_id) {
                                let total_tokens = req.pos_offset
                                    + req.prompt_tokens.len()
                                    + req.generated_tokens.len();
                                let n_seq = usize::try_from(self.n_seq_max)
                                    .ok()
                                    .filter(|v| *v > 0)
                                    .unwrap_or(1);
                                let seq_n_ctx = (self.n_ctx / n_seq).max(1);
                                eprintln!(
                                    "[Step] Seq Debug: request={} state={:?} total_tokens={} n_ctx_total={} n_ctx_seq={} pos_offset={} prompt_tokens={} generated_tokens={} n_prompt_processed={} n_decoded={} pending_mm_start={:?} mm_chunks={}",
                                    slot.request_id,
                                    req.state,
                                    total_tokens,
                                    self.n_ctx,
                                    seq_n_ctx,
                                    req.pos_offset,
                                    req.prompt_tokens.len(),
                                    req.generated_tokens.len(),
                                    slot.n_prompt_processed,
                                    slot.n_decoded,
                                    req.pending_mm_start,
                                    req.multimodal_meta.len()
                                );
                            }
                        }
                    }
                }
                return Err(e);
            }
        }

        if !prefill_progress.is_empty() {
            for (seq_id, processed) in prefill_progress.drain() {
                if let Some(slot) = self.slots.get_mut(&seq_id) {
                    slot.n_prompt_processed = processed;
                    if let Some(req) = self.requests.get_mut(&slot.request_id) {
                        if slot.n_prompt_processed >= req.prompt_tokens.len() {
                            req.state = RequestState::Processing;
                        }
                    }
                }
            }
        }

        if !eval_tokens.is_empty() {
            for (seq_id, tokens) in eval_tokens.drain() {
                if let Some(slot) = self.slots.get(&seq_id) {
                    if let Some(req) = self.requests.get(&slot.request_id) {
                        if let Some(sid) = &req.params.session_id {
                            let mut sessions = self.kv_manager.sessions.write();
                            if let Some(session) = sessions.get_mut(sid) {
                                session.kv_head = session.kv_head.saturating_add(tokens.len());
                                session.tokens.extend(tokens.into_iter());
                                session.last_used = std::time::Instant::now();
                            }
                        }
                    }
                }
            }
        }

        let mut finished_seqs = Vec::new();
        let mut embedding_seqs = Vec::new();

        for (&seq_id, &batch_idx) in &slot_batch_idx {
            if let Some(slot) = self.slots.get(&seq_id) {
                if let Some(req) = self.requests.get(&slot.request_id) {
                    if req.params.embedding {
                        let mut emb_ptr = self.ctx.get_embeddings_seq(seq_id);
                        if emb_ptr.is_null() {
                            emb_ptr = self.ctx.get_embeddings(batch_idx);
                        }
                        if emb_ptr.is_null() {
                            emb_ptr = self.ctx.get_embeddings_all();
                        }

                        if !emb_ptr.is_null() {
                            let n_embd = self._model.n_embd_out() as usize;
                            let slice = unsafe { std::slice::from_raw_parts(emb_ptr, n_embd) };
                            let mut embedding = slice.to_vec();

                            let mut sum = 0.0f32;
                            for &v in &embedding {
                                sum += v * v;
                            }
                            let norm = sum.sqrt();
                            if norm > 0.0 {
                                for v in &mut embedding {
                                    *v /= norm;
                                }
                            }

                            Self::emit_event(
                                &mut events,
                                req,
                                EventKind::Embedding {
                                    embedding,
                                    prompt_tokens: req.prompt_tokens.len() as u32,
                                    request: RequestHandle::new(
                                        req.params.id.clone(),
                                        req.cancel_flag.clone(),
                                    ),
                                },
                            );

                            Self::emit_event(
                                &mut events,
                                req,
                                EventKind::Finish {
                                    request: RequestHandle::new(
                                        req.params.id.clone(),
                                        req.cancel_flag.clone(),
                                    ),
                                    reason: StopReason::Eos,
                                },
                            );

                            embedding_seqs.push(seq_id);
                        } else {
                            Self::emit_event(
                                &mut events,
                                req,
                                EventKind::Error {
                                    message: "Failed to retrieve embeddings (null ptr)".to_string(),
                                    request: RequestHandle::new(
                                        req.params.id.clone(),
                                        req.cancel_flag.clone(),
                                    ),
                                },
                            );
                            embedding_seqs.push(seq_id);
                        }
                    }
                }
            }
        }

        for seq_id in &embedding_seqs {
            slot_batch_idx.remove(seq_id);
            if let Some(slot) = self.slots.get(seq_id) {
                if let Some(req) = self.requests.get_mut(&slot.request_id) {
                    req.state = RequestState::Finished;
                }
            }
            finished_seqs.push(*seq_id);
        }

        let sampled_tokens = self.sample_batch(&slot_batch_idx, &mut events, &mut finished_seqs);

        self.eval_one_pending_mm(&mut events, &mut abort_seqs)?;

        for seq_id in finished_seqs {
            if let Some(slot) = self.slots.remove(&seq_id) {
                self.requests.remove(&slot.request_id);
            }
        }

        for seq_id in abort_seqs {
            if let Some(slot) = self.slots.remove(&seq_id) {
                self.ctx.kv_cache_seq_rm(seq_id, -1, -1);
                self.kv_manager.release_sequence(seq_id, None);
                self.requests.remove(&slot.request_id);
            }
        }

        self.update_stats(sampled_tokens as f64, Instant::now());
        self.flow_log(format!(
            "pull done events={} sampled_tokens={} requests={} slots={}",
            events.len(),
            sampled_tokens,
            self.requests.len(),
            self.slots.len()
        ));

        Ok(events)
    }

    fn update_stats(&mut self, generated_tokens: f64, now: Instant) {
        let mut stats = self.engine_stats.write();
        stats.requests_processing = self
            .requests
            .values()
            .filter(|r| r.state == RequestState::Processing)
            .count();
        stats.requests_waiting = self
            .requests
            .values()
            .filter(|r| matches!(r.state, RequestState::Waiting | RequestState::Preparing))
            .count();
        stats.slots_active = self.slots.len();
        stats.slots_total = self.n_seq_max as usize;

        if let Some(last) = stats.last_tps_at {
            let dt = now.duration_since(last).as_secs_f64();
            if dt > 0.0 {
                let tps_inst = generated_tokens / dt;
                let alpha = 1.0 - (-dt / 30.0).exp();
                stats.tps_ema += alpha * (tps_inst - stats.tps_ema);
            }
        }
        stats.last_tps_at = Some(now);
        stats.tokens_per_sec_total = stats.tps_ema;
        stats.tokens_per_sec_per_active = if stats.requests_processing > 0 {
            stats.tps_ema / stats.requests_processing as f64
        } else {
            0.0
        };
    }

    fn sample_batch(
        &mut self,
        slot_batch_idx: &HashMap<i32, i32>,
        events: &mut Vec<Event>,
        finished_seqs: &mut Vec<i32>,
    ) -> usize {
        let think_tag_markers = self.think_tag_markers.clone();
        let mut sampled_tokens = 0usize;
        for (&seq_id, &batch_idx) in slot_batch_idx {
            let slot = match self.slots.get_mut(&seq_id) {
                Some(slot) => slot,
                None => {
                    eprintln!("[Sample] Missing slot {}, skipping", seq_id);
                    finished_seqs.push(seq_id);
                    continue;
                }
            };
            let req_id = slot.request_id.clone();
            let req = match self.requests.get_mut(&req_id) {
                Some(req) => req,
                None => {
                    eprintln!(
                        "[Sample] Missing request {} for slot {}, skipping",
                        req_id, seq_id
                    );
                    finished_seqs.push(seq_id);
                    continue;
                }
            };

            let (next_token, forced_close_token) =
                if let Some(forced) = req.thinking_state.pop_forced_close_token() {
                    (forced, true)
                } else {
                    (slot.sampler.sample(&self.ctx, batch_idx), false)
                };
            slot.sampler.accept(next_token);

            let fallback_without_open = req.params.enable_thinking
                && req.params.thinking_budget_tokens.is_some()
                && think_tag_markers.has_close();

            let _thinking_events = req.thinking_state.observe_generated_token(
                next_token,
                req.params.thinking_budget_tokens,
                &think_tag_markers,
                fallback_without_open,
            );

            let mut stop_reason = None;
            let is_eog = self._model.is_eog_token(next_token.0);
            if is_eog {
                stop_reason = Some(StopReason::Eos);
            } else if req.params.max_output_tokens >= 0 {
                let max = req.params.max_output_tokens as usize;
                if slot.n_decoded >= max {
                    stop_reason = Some(StopReason::MaxTokens);
                }
            }

            let allow_over_max_for_forced_close =
                forced_close_token && matches!(stop_reason, Some(StopReason::MaxTokens));
            if stop_reason.is_none() || allow_over_max_for_forced_close {
                req.generated_tokens.push(next_token);
                slot.n_decoded += 1;
                sampled_tokens += 1;
                if let Some(sid) = &req.params.session_id {
                    let mut sessions = self.kv_manager.sessions.write();
                    if let Some(session) = sessions.get_mut(sid) {
                        session.last_used = std::time::Instant::now();
                    }
                }

                let piece = self
                    .tokenizer
                    .decode_incremental(&mut req.utf8_buffer, next_token)
                    .unwrap_or_default();

                if !piece.is_empty() {
                    Self::emit_event(
                        events,
                        req,
                        EventKind::Text {
                            text: piece,
                            request: RequestHandle::new(req_id.clone(), req.cancel_flag.clone()),
                        },
                    );
                }
            }

            if let Some(reason) = stop_reason {
                let tail = Self::flush_utf8_buffer(req);
                if !tail.is_empty() {
                    Self::emit_event(
                        events,
                        req,
                        EventKind::Text {
                            text: tail,
                            request: RequestHandle::new(req_id.clone(), req.cancel_flag.clone()),
                        },
                    );
                }

                req.state = RequestState::Finished;
                finished_seqs.push(seq_id);
                Self::emit_event(
                    events,
                    req,
                    EventKind::Finish {
                        request: RequestHandle::new(req_id, req.cancel_flag.clone()),
                        reason,
                    },
                );
            }
        }
        sampled_tokens
    }

    fn schedule_requests(&mut self) {
        if self.request_queue.is_empty() {
            return;
        }
        self.flow_log(format!(
            "schedule start queue={} slots={}/{}",
            self.request_queue.len(),
            self.slots.len(),
            self.n_seq_max
        ));

        loop {
            if self.slots.len() >= self.n_seq_max as usize {
                break;
            }
            let req_id = match self.request_queue.pop() {
                Some(id) => id,
                None => break,
            };
            let should_schedule = match self.requests.get(&req_id) {
                Some(r) => r.state == RequestState::Waiting && !self.is_request_active(&req_id),
                None => false,
            };
            if !should_schedule {
                self.flow_log(format!(
                    "schedule skip request={} reason=not-waiting-or-active",
                    req_id
                ));
                continue;
            }

            let (session_id, parent_id, mut prompt_tokens, n_keep, sampling) = {
                let req = match self.requests.get(&req_id) {
                    Some(req) => req,
                    None => {
                        eprintln!("[Schedule] Missing request {}, skipping", req_id);
                        continue;
                    }
                };
                (
                    req.params.session_id.clone(),
                    req.params.parent_id.clone(),
                    req.prompt_tokens.clone(),
                    req.params.n_keep,
                    req.params.sampling.clone(),
                )
            };

            let mut target_seq_id = None;

            if let Some(sid) = &session_id {
                let sessions = self.kv_manager.sessions.read();
                if let Some(sess) = sessions.get(sid) {
                    if let Some(seq) = sess.vram_seq_id {
                        if !self.slots.contains_key(&seq) {
                            target_seq_id = Some(seq);
                        }
                    }
                }
            }

            if target_seq_id.is_none() {
                for seq_id in 0..self.n_seq_max {
                    if !self.slots.contains_key(&seq_id) {
                        target_seq_id = Some(seq_id);
                        break;
                    }
                }
            }

            if target_seq_id.is_none() {
                self.request_queue.push_front(req_id);
                self.flow_log("schedule pause reason=no-free-seq");
                break;
            }

            let seq_id = target_seq_id.unwrap();
            self.flow_log(format!("schedule assign request={} seq={}", req_id, seq_id));

            match self
                .kv_manager
                .evict_seq_owner(&mut self.ctx, seq_id, session_id.as_deref())
            {
                Ok(Some(owner_id)) => {
                    eprintln!(
                        "[State] Evicted session {} from seq {} to preserve state",
                        owner_id, seq_id
                    );
                }
                Ok(None) => {}
                Err(e) => {
                    eprintln!("[State] Failed to evict owner of seq {}: {}", seq_id, e);
                    self.request_queue.push_front(req_id);
                    break;
                }
            }

            let mut restored = false;

            if let Some(pid) = &parent_id {
                if self.slots.values().any(|s| s.request_id == *pid) {
                    let parent_seq_id = self
                        .slots
                        .iter()
                        .find(|(_, s)| s.request_id == *pid)
                        .map(|(id, _)| *id);

                    if let Some(p_seq) = parent_seq_id {
                        self.ctx.kv_cache_seq_cp(p_seq, seq_id, -1, -1);
                        restored = true;

                        if let Some(sid) = &session_id {
                            let _ = self.kv_manager.set_vram_seq(sid, seq_id);
                        }
                    }
                }
            }

            if !restored {
                if let Some(sid) = &session_id {
                    self.kv_manager
                        .register_session(sid.clone(), prompt_tokens.clone(), n_keep);

                    let mut reused_vram = false;
                    let mut session_len = 0usize;
                    {
                        let sessions = self.kv_manager.sessions.read();
                        if let Some(s) = sessions.get(sid) {
                            session_len = s.kv_head;
                            if s.vram_seq_id == Some(seq_id) {
                                reused_vram = true;
                            }
                        }
                    }

                    if reused_vram {
                        restored = true;
                        self.kv_manager.touch(sid);
                    } else {
                        match self.kv_manager.restore(&mut self.ctx, seq_id, sid) {
                            Ok(_) => {
                                restored = true;
                            }
                            Err(_) => {
                                let filename = format!("cache/{}.bin", sid);
                                if std::path::Path::new(&filename).exists() {}
                            }
                        }
                    }

                    if restored {
                        // Stateful contract: treat incoming prompt as new input only.
                        // Do not attempt delta/history token matching against restored KV.
                        let session_len = {
                            let sessions = self.kv_manager.sessions.read();
                            if let Some(s) = sessions.get(sid) {
                                s.kv_head
                            } else {
                                session_len
                            }
                        };

                        if !prompt_tokens.is_empty()
                            && prompt_tokens[0].0 == self._model.token_bos()
                        {
                            prompt_tokens.remove(0);
                        }

                        if let Some(req_mut) = self.requests.get_mut(&req_id) {
                            req_mut.prompt_tokens = prompt_tokens.clone();
                            req_mut.pos_offset = session_len;
                        }
                    }
                }
            }

            if !restored {
                self.ctx.kv_cache_seq_rm(seq_id, -1, -1);

                if let Some(sid) = &session_id {
                    let _ = self.kv_manager.set_vram_seq(sid, seq_id);
                }
            }

            if let Ok(sampler) = Sampler::new(&sampling, Some(self._model.vocab())) {
                self.slots.insert(
                    seq_id,
                    Slot {
                        request_id: req_id.clone(),
                        sampler,
                        n_decoded: 0,
                        n_prompt_processed: 0,
                        sample_from_cache: false,
                    },
                );
                self.flow_log(format!(
                    "schedule active request={} seq={} restored={} pos_offset={} prompt_tokens={}",
                    req_id,
                    seq_id,
                    restored,
                    self.requests
                        .get(&req_id)
                        .map(|r| r.pos_offset)
                        .unwrap_or(0),
                    self.requests
                        .get(&req_id)
                        .map(|r| r.prompt_tokens.len())
                        .unwrap_or(0)
                ));
            }
        }
    }

    fn flush_utf8_buffer(req: &mut Request) -> String {
        if req.utf8_buffer.is_empty() {
            return String::new();
        }

        let tail: Vec<u8> = req.utf8_buffer.drain(..).collect();
        String::from_utf8_lossy(&tail).into_owned()
    }

    fn process_state_actions(&mut self) {
        use crate::engine::kv_cache::Action;

        let actions: Vec<(String, Action)> = {
            let sessions = self.kv_manager.sessions.read();
            sessions
                .iter()
                .filter_map(|(id, s)| s.pending_action.clone().map(|a| (id.clone(), a)))
                .collect()
        };
        if actions.is_empty() {
            return;
        }

        for (session_id, action) in actions {
            match action {
                Action::Save { path } => {
                    let mut found_seq = None;
                    for (seq_id, slot) in &self.slots {
                        if let Some(req) = self.requests.get(&slot.request_id) {
                            if req.params.session_id.as_deref() == Some(&session_id) {
                                found_seq = Some(*seq_id);
                                break;
                            }
                        }
                    }

                    if let Some(seq_id) = found_seq {
                        if let Err(e) =
                            self.kv_manager
                                .save_to_disk(&self.ctx, seq_id, &session_id, &path)
                        {
                            eprintln!("Failed to save session {}: {}", session_id, e);
                        }
                    } else {
                        let mut saved = false;
                        let vram_seq_id = {
                            let sessions = self.kv_manager.sessions.read();
                            sessions.get(&session_id).and_then(|s| s.vram_seq_id)
                        };
                        if let Some(seq_id) = vram_seq_id {
                            if let Err(e) =
                                self.kv_manager
                                    .save_to_disk(&self.ctx, seq_id, &session_id, &path)
                            {
                                eprintln!(
                                    "Failed to save session {} from VRAM seq {}: {}",
                                    session_id, seq_id, e
                                );
                            } else {
                                saved = true;
                            }
                        }

                        if !saved {
                            if let Err(e) = self.kv_manager.save_ram_to_disk(&session_id, &path) {
                                eprintln!("Failed to save session {} from RAM: {}", session_id, e);
                            }
                        }
                    }
                }
                Action::Idle => {
                    eprintln!("[State] Idle requested for session {}", session_id);
                    let mut found_seq = None;
                    for (seq_id, slot) in &self.slots {
                        if let Some(req) = self.requests.get(&slot.request_id) {
                            if req.params.session_id.as_deref() == Some(&session_id) {
                                found_seq = Some(*seq_id);
                                break;
                            }
                        }
                    }
                    if let Some(seq_id) = found_seq {
                        if let Err(e) = self.kv_manager.evict(&mut self.ctx, seq_id, &session_id) {
                            eprintln!("Failed to idle session {}: {}", session_id, e);
                        } else {
                            self.slots.remove(&seq_id);
                        }
                    } else {
                        let vram_seq_id = {
                            let sessions = self.kv_manager.sessions.read();
                            sessions.get(&session_id).and_then(|s| s.vram_seq_id)
                        };
                        if let Some(seq_id) = vram_seq_id {
                            if let Err(e) =
                                self.kv_manager.evict(&mut self.ctx, seq_id, &session_id)
                            {
                                eprintln!(
                                    "Failed to idle session {} from VRAM seq {}: {}",
                                    session_id, seq_id, e
                                );
                            }
                        }
                    }
                }
                Action::Delete => {
                    eprintln!("[State] Delete requested for session {}", session_id);
                    let mut found_seq = None;
                    for (seq_id, slot) in &self.slots {
                        if let Some(req) = self.requests.get(&slot.request_id) {
                            if req.params.session_id.as_deref() == Some(&session_id) {
                                found_seq = Some(*seq_id);
                                break;
                            }
                        }
                    }
                    if let Some(seq_id) = found_seq {
                        self.ctx.kv_cache_seq_rm(seq_id, -1, -1);
                        self.slots.remove(&seq_id);
                    }
                    if found_seq.is_none() {
                        let vram_seq_id = {
                            let sessions = self.kv_manager.sessions.read();
                            sessions.get(&session_id).and_then(|s| s.vram_seq_id)
                        };
                        if let Some(seq_id) = vram_seq_id {
                            self.ctx.kv_cache_seq_rm(seq_id, -1, -1);
                        }
                    }
                    let mut sessions = self.kv_manager.sessions.write();
                    let disk_path = sessions.get(&session_id).and_then(|s| s.disk_path.clone());
                    sessions.remove(&session_id);
                    drop(sessions);
                    if let Some(path) = disk_path {
                        if let Err(e) = std::fs::remove_file(&path) {
                            eprintln!("Failed to remove disk file {}: {}", path.display(), e);
                        } else {
                            eprintln!("Removed disk file {}", path.display());
                        }
                    }
                }
            }

            {
                let mut sessions = self.kv_manager.sessions.write();
                if let Some(s) = sessions.get_mut(&session_id) {
                    s.pending_action = None;
                }
            }
        }
    }

    fn is_request_active(&self, id: &str) -> bool {
        self.slots.values().any(|s| &s.request_id == id)
    }

    fn pos_for_prompt_index(req: &Request, idx: usize) -> i32 {
        if req.multimodal_meta.is_empty() {
            return idx as i32 + req.pos_offset as i32;
        }
        let mut pos = idx as i32;
        for meta in &req.multimodal_meta {
            if meta.start >= idx {
                break;
            }
            let delta = meta.n_pos as i32 - meta.n_tokens as i32;
            pos += delta;
        }
        pos + req.pos_offset as i32
    }

    fn pos_for_last_token(req: &Request) -> i32 {
        let prompt_pos_end = Self::pos_for_prompt_index(req, req.prompt_tokens.len());
        if req.generated_tokens.is_empty() {
            return prompt_pos_end.saturating_sub(1);
        }
        prompt_pos_end + req.generated_tokens.len() as i32 - 1
    }

    fn apply_context_shifts(&mut self, events: &mut Vec<Event>) -> Result<()> {
        let n_ctx = self.n_ctx.max(1);
        let ctx = &self.ctx;

        let mut finished = Vec::new();

        for (&seq_id, slot) in self.slots.iter_mut() {
            let req = match self.requests.get_mut(&slot.request_id) {
                Some(req) => req,
                None => continue,
            };

            if req.state != RequestState::Processing && req.state != RequestState::Waiting {
                continue;
            }
            if req.params.embedding {
                // Embedding requests are single-pass encode jobs and should not
                // trigger conversational context-shift behavior.
                continue;
            }

            let total_tokens =
                req.pos_offset + req.prompt_tokens.len() + req.generated_tokens.len();
            if total_tokens + 1 < n_ctx {
                continue;
            }

            let mut n_keep = req.params.n_keep;
            if !req.multimodal_meta.is_empty() {
                // Keep restored prefix + full multimodal prompt intact.
                // This allows shifting on multimodal requests without slicing image placeholder spans.
                let mm_keep_floor = req.pos_offset.saturating_add(req.prompt_tokens.len());
                n_keep = n_keep.max(mm_keep_floor);
            }
            if n_keep > n_ctx.saturating_sub(4) {
                n_keep = n_ctx.saturating_sub(4);
            }

            let n_left = total_tokens.saturating_sub(n_keep);
            if n_left == 0 {
                continue;
            }

            let mut n_discard = if req.params.n_discard > 0 {
                req.params.n_discard
            } else {
                n_left / 2
            };
            n_discard = n_discard.min(n_left);
            if n_discard == 0 {
                eprintln!(
                    "[Shift] Context full but n_discard=0 request={} seq={} total_tokens={} n_ctx={} n_keep={}",
                    slot.request_id, seq_id, total_tokens, n_ctx, n_keep
                );
                Self::emit_event(
                    events,
                    req,
                    EventKind::Error {
                        message: "Context full and n_discard is 0. Cannot shift.".to_string(),
                        request: RequestHandle::new(
                            slot.request_id.clone(),
                            req.cancel_flag.clone(),
                        ),
                    },
                );
                finished.push(seq_id);
                continue;
            }

            eprintln!(
                "[Shift] Triggered request={} seq={} total_tokens={} n_ctx={} n_keep={} n_discard={} pos_offset={} prompt_tokens={} generated_tokens={}",
                slot.request_id,
                seq_id,
                total_tokens,
                n_ctx,
                n_keep,
                n_discard,
                req.pos_offset,
                req.prompt_tokens.len(),
                req.generated_tokens.len()
            );
            if !req.multimodal_meta.is_empty() {
                eprintln!(
                    "[Shift] Multimodal mode: preserving prompt spans (mm_chunks={})",
                    req.multimodal_meta.len()
                );
            }

            if req.state == RequestState::Processing && req.generated_tokens.is_empty() {
                slot.sample_from_cache = true;
            }

            if !self.ctx.kv_cache_can_shift() {
                eprintln!(
                    "[Shift] Unsupported by backend request={} seq={} total_tokens={} n_ctx={}",
                    slot.request_id, seq_id, total_tokens, n_ctx
                );
                Self::emit_event(
                    events,
                    req,
                    EventKind::Error {
                        message: "Context shift unsupported by backend for this model".to_string(),
                        request: RequestHandle::new(
                            slot.request_id.clone(),
                            req.cancel_flag.clone(),
                        ),
                    },
                );
                finished.push(seq_id);
                continue;
            }

            unsafe {
                let mem = llama_cpp::llama_get_memory(ctx.as_ptr());
                if mem.is_null() {
                    Self::emit_event(
                        events,
                        req,
                        EventKind::Error {
                            message: "Context shift failed: no memory module".to_string(),
                            request: RequestHandle::new(
                                slot.request_id.clone(),
                                req.cancel_flag.clone(),
                            ),
                        },
                    );
                    finished.push(seq_id);
                    continue;
                }

                let ok = llama_cpp::llama_memory_seq_rm(
                    mem,
                    seq_id,
                    n_keep as i32,
                    (n_keep + n_discard) as i32,
                );
                if !ok {
                    events.push(Event {
                        id: Uuid::new_v4().to_string(),
                        kind: EventKind::Error {
                            message: "Context shift failed: seq_rm".to_string(),
                            request: RequestHandle::new(
                                slot.request_id.clone(),
                                req.cancel_flag.clone(),
                            ),
                        },
                    });
                    finished.push(seq_id);
                    continue;
                }

                llama_cpp::llama_memory_seq_add(
                    mem,
                    seq_id,
                    (n_keep + n_discard) as i32,
                    total_tokens as i32,
                    -(n_discard as i32),
                );
            }

            let start = n_keep;
            let end = n_keep + n_discard;

            if let Some(sid) = &req.params.session_id {
                let mut sessions = self.kv_manager.sessions.write();
                if let Some(s) = sessions.get_mut(sid) {
                    let end_clamped = end.min(s.kv_head).min(s.tokens.len());
                    if start < end_clamped {
                        s.tokens.drain(start..end_clamped);
                        let removed_cnt = end_clamped - start;
                        s.kv_head = s.kv_head.saturating_sub(removed_cnt);
                    }
                }
            }

            if req.pos_offset == 0 {
                let mut combined = Vec::with_capacity(total_tokens.saturating_sub(n_discard));
                combined.extend_from_slice(&req.prompt_tokens);
                combined.extend_from_slice(&req.generated_tokens);
                combined.drain(start..end);
                req.prompt_tokens = combined;
                req.generated_tokens.clear();
            } else {
                let restored_len = req.pos_offset;
                let mut remaining = n_discard;

                if end <= restored_len {
                    req.pos_offset = req.pos_offset.saturating_sub(remaining);
                } else if start < restored_len {
                    let removed_from_restored = restored_len - start;
                    req.pos_offset = start;
                    remaining = remaining.saturating_sub(removed_from_restored);

                    if remaining > 0 {
                        let remove_from_prompt = remaining.min(req.prompt_tokens.len());
                        if remove_from_prompt > 0 {
                            req.prompt_tokens.drain(0..remove_from_prompt);
                            remaining = remaining.saturating_sub(remove_from_prompt);
                        }
                    }
                    if remaining > 0 {
                        let remove_from_gen = remaining.min(req.generated_tokens.len());
                        if remove_from_gen > 0 {
                            req.generated_tokens.drain(0..remove_from_gen);
                        }
                    }
                } else {
                    let start_in_prompt = start.saturating_sub(restored_len);
                    let end_in_prompt = end.saturating_sub(restored_len);

                    if start_in_prompt < req.prompt_tokens.len() {
                        let prompt_end = end_in_prompt.min(req.prompt_tokens.len());
                        let removed_prompt = prompt_end.saturating_sub(start_in_prompt);
                        if removed_prompt > 0 {
                            req.prompt_tokens.drain(start_in_prompt..prompt_end);
                        }
                        remaining = remaining.saturating_sub(removed_prompt);
                        if remaining > 0 {
                            let remove_from_gen = remaining.min(req.generated_tokens.len());
                            if remove_from_gen > 0 {
                                req.generated_tokens.drain(0..remove_from_gen);
                            }
                        }
                    } else {
                        let start_in_gen = start_in_prompt.saturating_sub(req.prompt_tokens.len());
                        let end_in_gen = end_in_prompt.saturating_sub(req.prompt_tokens.len());
                        let gen_end = end_in_gen.min(req.generated_tokens.len());
                        if start_in_gen < gen_end {
                            req.generated_tokens.drain(start_in_gen..gen_end);
                        }
                    }
                }

                if let Some(sid) = &req.params.session_id {
                    let sessions = self.kv_manager.sessions.read();
                    if let Some(s) = sessions.get(sid) {
                        // Rebase prompt start to match the actual KV head after shift.
                        // In processing mode, one generated token is typically pending decode,
                        // so only generated_tokens.len() - 1 are already in KV.
                        let old_pos_offset = req.pos_offset;
                        let prompt_in_kv = slot.n_prompt_processed.min(req.prompt_tokens.len());
                        let generated_in_kv = if req.state == RequestState::Processing {
                            req.generated_tokens.len().saturating_sub(1)
                        } else {
                            0
                        };
                        let live_in_kv = prompt_in_kv.saturating_add(generated_in_kv);
                        req.pos_offset = s.kv_head.saturating_sub(live_in_kv);
                        if self.trace_flow {
                            eprintln!(
                                "[Shift] Rebase request={} seq={} old_pos_offset={} new_pos_offset={} kv_head={} prompt_in_kv={} generated_in_kv={} state={:?}",
                                slot.request_id,
                                seq_id,
                                old_pos_offset,
                                req.pos_offset,
                                s.kv_head,
                                prompt_in_kv,
                                generated_in_kv,
                                req.state
                            );
                        }
                    }
                }
            }
        }

        for seq_id in finished {
            if let Some(slot) = self.slots.remove(&seq_id) {
                if let Some(req) = self.requests.get_mut(&slot.request_id) {
                    req.state = RequestState::Finished;
                }
            }
        }

        Ok(())
    }

    fn eval_one_pending_mm(
        &mut self,
        events: &mut Vec<Event>,
        abort_seqs: &mut Vec<i32>,
    ) -> Result<()> {
        if self.mmproj.is_none() {
            return Ok(());
        }

        let mut mm_seq_id = None;
        let mut mm_tok_idx = None;
        for (&seq_id, slot) in self.slots.iter() {
            let req = match self.requests.get(&slot.request_id) {
                Some(req) => req,
                None => continue,
            };
            if req.state != RequestState::Waiting {
                continue;
            }
            if let Some(tok_idx) = req.pending_mm_start {
                mm_seq_id = Some(seq_id);
                mm_tok_idx = Some(tok_idx);
                break;
            }
        }

        if let (Some(seq_id), Some(tok_idx)) = (mm_seq_id, mm_tok_idx) {
            let slot = match self.slots.get_mut(&seq_id) {
                Some(slot) => slot,
                None => {
                    eprintln!("[MM] Missing slot {}, skipping", seq_id);
                    return Ok(());
                }
            };
            let req_id = slot.request_id.clone();
            let req = match self.requests.get_mut(&req_id) {
                Some(req) => req,
                None => {
                    eprintln!(
                        "[MM] Missing request {} for slot {}, aborting seq",
                        req_id, seq_id
                    );
                    abort_seqs.push(seq_id);
                    return Ok(());
                }
            };
            if let Some(chunk) = req.multimodal_chunks.get(&tok_idx) {
                let n_tokens = chunk.n_tokens();
                let n_pos = chunk.n_pos();
                let pos_next = Self::pos_for_prompt_index(req, tok_idx);
                let (status, _) = self.mmproj.as_ref().unwrap().eval_chunk(
                    chunk,
                    &self.ctx,
                    pos_next,
                    seq_id,
                    self.n_batch as i32,
                    true,
                )?;
                if status != 0 {
                    Self::emit_event(
                        events,
                        req,
                        EventKind::Error {
                            message: format!("Multimodal chunk eval failed: {}", status),
                            request: RequestHandle::new(req_id.clone(), req.cancel_flag.clone()),
                        },
                    );
                    req.state = RequestState::Finished;
                    abort_seqs.push(seq_id);
                } else {
                    req.pending_mm_start = None;
                    slot.n_prompt_processed = slot.n_prompt_processed.saturating_add(n_tokens);
                    if slot.n_prompt_processed >= req.prompt_tokens.len() {
                        req.state = RequestState::Processing;
                    }
                    if let Some(sid) = &req.params.session_id {
                        let mut sessions = self.kv_manager.sessions.write();
                        if let Some(s) = sessions.get_mut(sid) {
                            s.kv_head = s.kv_head.saturating_add(n_pos);
                            s.last_used = Instant::now();
                        }
                    }
                }
            } else {
                req.pending_mm_start = None;
            }
        }

        Ok(())
    }

    fn emit_event(events: &mut Vec<Event>, req: &Request, kind: EventKind) {
        let event = Event {
            id: Uuid::new_v4().to_string(),
            kind,
        };
        if let Some(tx) = &req.response_tx {
            let _ = tx.send(event.clone());
        }
        events.push(event);
    }
}

impl<'a> Drop for LlmEngine<'a> {
    fn drop(&mut self) {
        // Signal the prep thread to stop by dropping the sender
        self.prep_tx.take();

        if let Some(handle) = self._prep_handle.take() {
            // Wait up to 5 seconds for the prep thread to finish
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                if handle.is_finished() {
                    let _ = handle.join();
                    break;
                }
                if std::time::Instant::now() >= deadline {
                    eprintln!("Warning: prep thread did not finish within 5s, abandoning");
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
    }
}
