pub mod request;
pub mod types;
pub mod kv_cache;
pub mod queue;

use crate::llm::batch::Batch;
use crate::llm::context::{Context, ContextParams};
use crate::llm::error::{Result};
use crate::llm::model::Model;
use crate::llm::sampling::Sampler;
use crate::llm::tokenizer::{Tokenizer, Token};
use crate::llm::multimodal::{MultimodalContext, Image, Chunk};
use request::{Request, RequestState, ChunkMeta};
use types::{Event, EventKind, StopReason, RequestHandle};

use std::collections::{HashMap};
use std::sync::{Arc, atomic::Ordering, mpsc};
use std::thread;
use std::time::Instant;
use uuid::Uuid;
use parking_lot::RwLock;

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
    
    // New components
    pub input_rx: tokio::sync::mpsc::Receiver<Request>, // Server -> Engine (Request Input)
    pub request_queue: crate::engine::queue::RequestQueue,
    pub kv_manager: Arc<crate::engine::kv_cache::KvCacheManager>,
    pub engine_stats: Arc<RwLock<EngineStats>>,
    
    prep_tx: Option<mpsc::Sender<PrepJob>>,
    prep_rx: mpsc::Receiver<PrepResult>,
    _prep_handle: Option<thread::JoinHandle<()>>,
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

        let mmproj = if let Some(path) = mmproj_path {
            Some(Arc::new(MultimodalContext::from_file(path, model)?))
        } else {
            None
        };

        let (prep_tx, prep_job_rx) = mpsc::channel::<PrepJob>();
        let (prep_result_tx, prep_rx) = mpsc::channel::<PrepResult>();

        let worker_mmproj = mmproj.clone();
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
                                Err(e) => Err(format!("Multimodal tokenization failed: {}", e)),
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
            n_ubatch: ctx_params.n_batch as usize,
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
        let cancel_flag = req.cancel_flag.clone();
        let response_tx = req.response_tx.clone();
        let mut submission_error = None;

        // Tokenization Logic
        if self.mmproj.is_some() && !req.params.images.is_empty() {
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
                        submission_error = Some("Failed to enqueue multimodal preparation".to_string());
                        self.requests.remove(&id);
                    }
                }
                None => {
                    submission_error = Some("Prep worker unavailable".to_string());
                    self.requests.remove(&id);
                }
            }
        } else {
            match self.tokenizer.tokenize(&req.params.prompt, true, true) {
                Ok(tokens) => {
                    req.prompt_tokens = tokens;
                    req.state = RequestState::Waiting;
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

        if let Some(err_msg) = submission_error {
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

        // 0. Drain new requests from server
        while let Ok(req) = self.input_rx.try_recv() {
            self.push(req);
        }

        events.append(&mut self.pending_events);
        
        let mut abort_seqs = Vec::new();

        // Drain multimodal preparation results (non-blocking)
        for prep in self.prep_rx.try_iter() {
            if let Some(req) = self.requests.get_mut(&prep.request_id) {
                match prep.result {
                    Ok(prepared) => {
                        req.prompt_tokens = prepared.prompt_tokens;
                        req.multimodal_chunks = prepared.multimodal_chunks;
                        req.multimodal_meta = prepared.multimodal_meta;
                        req.state = RequestState::Waiting;
                        if let Err(msg) = self.request_queue.push(prep.request_id.clone()) {
                            req.state = RequestState::Finished;
                            Self::emit_event(&mut events, req, EventKind::Error {
                                message: msg,
                                request: RequestHandle::new(prep.request_id.clone(), req.cancel_flag.clone()),
                            });
                            self.requests.remove(&prep.request_id);
                        }
                    }
                    Err(msg) => {
                        req.state = RequestState::Finished;
                        Self::emit_event(&mut events, req, EventKind::Error {
                            message: msg,
                            request: RequestHandle::new(prep.request_id.clone(), req.cancel_flag.clone()),
                        });
                        self.requests.remove(&prep.request_id);
                    }
                }
            }
        }

        // 0.5 Process State Actions
        self.process_state_actions();

        // 1. Assign Slots
        self.schedule_requests();

        // 1.5 Apply context shift
        self.apply_context_shifts(&mut events)?;

        // 2. Prepare Batch
        self.batch.clear();
        
        // Track where each slot's "logit token" is in the batch
        let mut slot_batch_idx = HashMap::new(); 
        let mut eval_tokens: HashMap<i32, Vec<Token>> = HashMap::new();
        {
        let slots = &mut self.slots;
        let requests = &mut self.requests;
        let batch = &mut self.batch;
        let n_batch = self.n_batch;
        let n_ubatch = self.n_ubatch;

        // 2.a Add decode tokens first (continuous batching parity)
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
            
            // Check cancellation
            if req.cancel_flag.load(Ordering::Acquire) {
                 req.state = RequestState::Finished;
                 abort_seqs.push(seq_id);
                 Self::emit_event(&mut events, req, EventKind::Finish {
                     request: RequestHandle::new(slot.request_id.clone(), req.cancel_flag.clone()),
                     reason: StopReason::Cancelled,
                 });
                 continue;
            }

            if req.state != RequestState::Processing {
                continue;
            }

            if batch.handle.n_tokens as usize >= n_batch {
                break;
            }

            if req.generated_tokens.is_empty() && req.prompt_tokens.is_empty() {
                Self::emit_event(&mut events, req, EventKind::Error {
                    message: "Processing request has no prompt tokens".to_string(),
                    request: RequestHandle::new(slot.request_id.clone(), req.cancel_flag.clone()),
                });
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
            let pos_last = Self::pos_for_last_token(req);
            let idx = batch.handle.n_tokens;
            batch.add_seq(last_tok.0, pos_last, seq_id, true)?;
            slot_batch_idx.insert(seq_id, idx);
        }

        // 2.b Fill remaining capacity with prompt prefill
        // We limit by n_ubatch (physical batch limit) or n_batch (logical limit), whichever is smaller/relevant.
        // Usually we want to fill up to n_batch, but if n_ubatch affects memory buffering, we might limit.
        // Let's use the stricter of n_batch or n_ubatch for the *current* step's fill.
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
            
            // Check cancellation
            if req.cancel_flag.load(Ordering::Acquire) {
                 req.state = RequestState::Finished;
                 abort_seqs.push(seq_id);
                 Self::emit_event(&mut events, req, EventKind::Finish {
                     request: RequestHandle::new(slot.request_id.clone(), req.cancel_flag.clone()),
                     reason: StopReason::Cancelled,
                 });
                 continue;
            }

            if req.state != RequestState::Waiting {
                continue;
            }

            if req.prompt_tokens.is_empty() {
                Self::emit_event(&mut events, req, EventKind::Error {
                    message: "Waiting request has empty prompt tokens".to_string(),
                    request: RequestHandle::new(slot.request_id.clone(), req.cancel_flag.clone()),
                });
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

            // Removed "Prompt too large" check. We rely on chunking loop below.

            while processed < total_len && prefill_budget > 0 {
                let tok_idx = processed;

                // Multimodal chunk at this position
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
                let logits_flag = is_last;
                let pos = Self::pos_for_prompt_index(req, tok_idx);
                let idx = batch.handle.n_tokens;
                batch.add_seq(tok.0, pos, seq_id, logits_flag)?;
                if is_last {
                    slot_batch_idx.insert(seq_id, idx);
                }
                eval_tokens.entry(seq_id).or_default().push(tok);
                processed += 1;
                prefill_budget = prefill_budget.saturating_sub(1);
            }

            slot.n_prompt_processed = processed;
            if slot.n_prompt_processed >= total_len {
                req.state = RequestState::Processing;
            }
        }
        }

        if self.batch.handle.n_tokens == 0 {
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

        // 3. Decode
        if self._model.has_encoder() && !self._model.has_decoder() {
            self.ctx.encode(&mut self.batch)?;
        } else {
            if let Err(e) = self.ctx.decode(&mut self.batch) {
                eprintln!("[Step] Decode FAILED: {}", e);
                // Dump Batch state
                unsafe {
                    eprintln!("[Step] Batch n_tokens: {}", self.batch.handle.n_tokens);
                    if self.batch.handle.n_tokens > 0 {
                        let pos = *self.batch.handle.pos.add(0);
                        let seq = *(*self.batch.handle.seq_id.add(0));
                        eprintln!("[Step] First Token: Pos={}, Seq={}", pos, seq);
                    }
                }
                return Err(e);
            }
        }
        
        // Commit evaluated tokens to kv_head and session history after a successful decode.
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

        // 3.5 Extract Embeddings
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
                             let n_embd = self._model.n_embd() as usize;
                             let slice = unsafe { std::slice::from_raw_parts(emb_ptr, n_embd) };
                             let mut embedding = slice.to_vec();

                             // Normalization (L2) - common for embedding models
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

                             Self::emit_event(&mut events, req, EventKind::Embedding {
                                 embedding,
                                 request: RequestHandle::new(req.params.id.clone(), req.cancel_flag.clone()),
                             });
                             
                             // Send Finish for clean close
                             Self::emit_event(&mut events, req, EventKind::Finish {
                                 request: RequestHandle::new(req.params.id.clone(), req.cancel_flag.clone()),
                                 reason: StopReason::Eos,
                             });

                             embedding_seqs.push(seq_id);
                         } else {
                             Self::emit_event(&mut events, req, EventKind::Error {
                                 message: "Failed to retrieve embeddings (null ptr)".to_string(),
                                 request: RequestHandle::new(req.params.id.clone(), req.cancel_flag.clone()),
                             });
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
        
        // 4. Sample
        let sampled_tokens = self.sample_batch(&slot_batch_idx, &mut events, &mut finished_seqs);
        
        // 4.5 Post-sample multimodal eval (cap = 1 per tick)
        self.eval_one_pending_mm(&mut events, &mut abort_seqs)?;
        
        // 5. Cleanup finished
        for seq_id in finished_seqs {
            if let Some(slot) = self.slots.remove(&seq_id) {
                // Request remains in map as Finished, or remove?
                // Typically user wants to poll until finish, then we can drop.
                // But map grows indefinitely if we don't drop.
                // Let's remove from requests too? 
                // Or let user 'ack'?
                // For simplicity, we keep it in requests map but state is Finished.
                // Cleanup separate method?
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
        
        // Update shared stats for /v1/monitor
        self.update_stats(sampled_tokens as f64, Instant::now());

        Ok(events)
    }

    fn update_stats(&mut self, generated_tokens: f64, now: Instant) {
        let mut stats = self.engine_stats.write();
        stats.requests_processing = self.requests.values().filter(|r| r.state == RequestState::Processing).count();
        stats.requests_waiting = self.requests.values().filter(|r| matches!(r.state, RequestState::Waiting | RequestState::Preparing)).count();
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

    fn sample_batch(&mut self, 
        slot_batch_idx: &HashMap<i32, i32>, 
        events: &mut Vec<Event>,
        finished_seqs: &mut Vec<i32>) -> usize 
    {
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
            
            // Sample
            let next_token = slot.sampler.sample(&self.ctx, batch_idx);
            slot.sampler.accept(next_token);
            
            // Check Stop
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

            if stop_reason.is_none() {
                req.generated_tokens.push(next_token);
                slot.n_decoded += 1;
                sampled_tokens += 1;
                if let Some(sid) = &req.params.session_id {
                    let mut sessions = self.kv_manager.sessions.write();
                    if let Some(session) = sessions.get_mut(sid) {
                        session.last_used = std::time::Instant::now();
                    }
                }

                // Decode piece
                let piece = self.tokenizer.decode(&[next_token]).unwrap_or_default();

                Self::emit_event(events, req, EventKind::Text {
                    text: piece,
                    request: RequestHandle::new(req_id.clone(), req.cancel_flag.clone()),
                });
            }
            
            if let Some(reason) = stop_reason {
                req.state = RequestState::Finished;
                finished_seqs.push(seq_id);
                Self::emit_event(events, req, EventKind::Finish {
                    request: RequestHandle::new(req_id, req.cancel_flag.clone()),
                    reason,
                });
            }
        }
        sampled_tokens
    }

    fn schedule_requests(&mut self) {
        // Collect free slots (not currently processing)
        // We will alloc from this set if we can't reuse.
        // But we won't eagerly pop.
        
        // Very basic simple scheduler: FIFO 
        if self.request_queue.is_empty() { return; }

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
                continue;
            }
            // Extract necessary data to avoid borrowing self.requests while mutating
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

            // 1. Determine Target Slot
            let mut target_seq_id = None;
            
            // Priority A: Reuse existing VRAM sequence for this session
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
            
            // Priority B: Any free slot
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
                break;
            }
            
            let seq_id = target_seq_id.unwrap();
            
            // CRITICAL: Evict ownership of this sequence from ANYONE ELSE (preserve state)
            match self.kv_manager.evict_seq_owner(&mut self.ctx, seq_id, session_id.as_deref()) {
                Ok(Some(owner_id)) => {
                    eprintln!("[State] Evicted session {} from seq {} to preserve state", owner_id, seq_id);
                }
                Ok(None) => {}
                Err(e) => {
                    eprintln!("[State] Failed to evict owner of seq {}: {}", seq_id, e);
                    self.request_queue.push_front(req_id);
                    break;
                }
            }
            
            
            // Check if we need to restore state or fork
            let mut restored = false;
            let mut restored_token_count = 0;
            let mut delta_mode = false;

            // 1. Forking?
            if let Some(pid) = &parent_id {
                // Check if parent is in VRAM
                if let Some(parent_slot) = self.slots.values().find(|s| s.request_id == *pid) {
                     let parent_seq_id = self.slots.iter()
                        .find(|(_, s)| s.request_id == *pid)
                        .map(|(id, _)| *id);
                        
                     if let Some(p_seq) = parent_seq_id {
                         self.ctx.kv_cache_seq_cp(p_seq, seq_id, -1, -1);
                         restored = true;
                         restored_token_count = parent_slot.n_prompt_processed + parent_slot.n_decoded;
                         
                         if let Some(sid) = &session_id {
                             let _ = self.kv_manager.set_vram_seq(sid, seq_id);
                         }
                     }
                }
            }
            
            // 2. Resuming session? (Self)
            if !restored {
               if let Some(sid) = &session_id {
                   // Register session (ensure it exists in manager)
                    self.kv_manager.register_session(sid.clone(), prompt_tokens.clone(), n_keep);
                   
                   // Check VRAM HIT
                   let mut reused_vram = false;
                   {
                       let sessions = self.kv_manager.sessions.read();
                       if let Some(s) = sessions.get(sid) {
                           if s.vram_seq_id == Some(seq_id) {
                               reused_vram = true;
                               // CRITICAL: Use kv_head (verified state), NOT tokens.len() (intent)
                               // If we reuse VRAM, what is there is exactly what we evaluated.
                               restored_token_count = s.kv_head;
                           }
                       }
                   }

                   if reused_vram {
                       restored = true;
                       self.kv_manager.touch(sid);
                   } else {
                       // Try restore (Unified/RAM/Disk)
                       match self.kv_manager.restore(&mut self.ctx, seq_id, sid) {
                           Ok(n_restored) => {
                               restored = true;
                               restored_token_count = n_restored;
                           },
                           Err(_) => {
                                let filename = format!("cache/{}.bin", sid);
                                if std::path::Path::new(&filename).exists() {
                                     // Legacy: ignore for now.
                                }
                           }
                       }
                   }
                   
                   // STRICT STATEFUL LOGIC:
                   // If we successfully restored a session (RAM/Disk/VRAM), the new request IS a Delta (Append).
                   // There is no "Full History" check. User supplies ID -> We append to that ID's state.
                   if restored {
                        let session_len = {
                            let sessions = self.kv_manager.sessions.read();
                            if let Some(s) = sessions.get(sid) {
                                s.kv_head
                            } else {
                                0
                            }
                        };
                        
                        // Delta Mode: Append
                        // We must strip BOS if present in new tokens, to avoid [BOS, Hello, ... BOS, New]
                        // This corresponds to the user input "Where is Paris?" when context already has "Hello".
                        if !prompt_tokens.is_empty() && prompt_tokens[0].0 == self._model.token_bos() {
                             prompt_tokens.remove(0);
                        }
                        
                        if let Some(req_mut) = self.requests.get_mut(&req_id) {
                            req_mut.prompt_tokens = prompt_tokens.clone();
                            req_mut.pos_offset = session_len;
                        }
                        
                        // Prevent truncation of the existing (restored) state.
                        // We are logically processing ONLY the new tokens, so the "restored" match count 
                        // for the purpose of the *current request's prompt* is technically 0 relative to the new tokens.
                        // (The context is handled via pos_offset).
                        delta_mode = true;
                        restored_token_count = 0; 
                   }
               }
            }

            if !restored {
                 // Clear just in case we stole a dirty slot
                 self.ctx.kv_cache_seq_rm(seq_id, -1, -1);
                 restored_token_count = 0;
                 
                 // If we have a session, assume ownership of this empty slot
                 if let Some(sid) = &session_id {
                     let _ = self.kv_manager.set_vram_seq(sid, seq_id);
                 }
            }
            
            if let Ok(sampler) = Sampler::new(&sampling, Some(self._model.vocab())) {
                 // Calculate n_prompt_processed based on prefix match
                 let req_tokens_len = prompt_tokens.len();
                 let n_past = std::cmp::min(restored_token_count, req_tokens_len);
                 
                 // Only truncate in Full prompt mode. Delta mode must not touch restored KV.
                 if !delta_mode && restored_token_count > n_past {
                     self.ctx.kv_cache_seq_rm(seq_id, n_past as i32, -1);
                 }

                 self.slots.insert(seq_id, Slot {
                     request_id: req_id.clone(),
                     sampler,
                     n_decoded: 0,
                     n_prompt_processed: n_past, 
                 });
            }
        }
    }
    
    fn process_state_actions(&mut self) {
        use crate::engine::kv_cache::Action;
        
        let actions: Vec<(String, Action)> = {
             let sessions = self.kv_manager.sessions.read();
             sessions.iter()
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
                         if let Err(e) = self.kv_manager.save_to_disk(&self.ctx, seq_id, &session_id, &path) {
                             eprintln!("Failed to save session {}: {}", session_id, e);
                         }
                    } else {
                        let mut saved = false;
                        let vram_seq_id = {
                            let sessions = self.kv_manager.sessions.read();
                            sessions.get(&session_id).and_then(|s| s.vram_seq_id)
                        };
                        if let Some(seq_id) = vram_seq_id {
                            if let Err(e) = self.kv_manager.save_to_disk(&self.ctx, seq_id, &session_id, &path) {
                                eprintln!("Failed to save session {} from VRAM seq {}: {}", session_id, seq_id, e);
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
                },
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
                            if let Err(e) = self.kv_manager.evict(&mut self.ctx, seq_id, &session_id) {
                                eprintln!("Failed to idle session {} from VRAM seq {}: {}", session_id, seq_id, e);
                            }
                        }
                    }
                },
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
            
            // Clear pending action
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
        let n_ctx = self.n_ctx;
        let ctx = &self.ctx;

        let mut finished = Vec::new();

        for (&seq_id, slot) in self.slots.iter_mut() {
            let req = match self.requests.get_mut(&slot.request_id) {
                Some(req) => req,
                None => continue,
            };

            if req.state != RequestState::Processing {
                continue;
            }

            let total_tokens = req.pos_offset + req.prompt_tokens.len() + req.generated_tokens.len();
            if total_tokens + 1 < n_ctx {
                continue;
            }

            if !req.multimodal_meta.is_empty() {
                Self::emit_event(events, req, EventKind::Error {
                    message: "Context shift not supported for multimodal requests".to_string(),
                    request: RequestHandle::new(slot.request_id.clone(), req.cancel_flag.clone()),
                });
                finished.push(seq_id);
                continue;
            }

            let mut n_keep = req.params.n_keep;
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
                Self::emit_event(events, req, EventKind::Error {
                    message: "Context full and n_discard is 0. Cannot shift.".to_string(),
                    request: RequestHandle::new(slot.request_id.clone(), req.cancel_flag.clone()),
                });
                finished.push(seq_id);
                continue;
            }

            unsafe {
                let mem = llama_cpp::llama_get_memory(ctx.as_ptr());
                if mem.is_null() {
                    Self::emit_event(events, req, EventKind::Error {
                        message: "Context shift failed: no memory module".to_string(),
                        request: RequestHandle::new(slot.request_id.clone(), req.cancel_flag.clone()),
                    });
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
                            request: RequestHandle::new(slot.request_id.clone(), req.cancel_flag.clone()),
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

            // Update session token history if available (full history).
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
                // Treat context as: [restored (pos_offset)] + [prompt] + [generated]
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
                    // start is within prompt or generated
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

                // Recompute pos_offset from session history if available.
                if let Some(sid) = &req.params.session_id {
                    let sessions = self.kv_manager.sessions.read();
                    if let Some(s) = sessions.get(sid) {
                        let live_len = req.prompt_tokens.len() + req.generated_tokens.len();
                        req.pos_offset = s.kv_head.saturating_sub(live_len);
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

    fn eval_one_pending_mm(&mut self, events: &mut Vec<Event>, abort_seqs: &mut Vec<i32>) -> Result<()> {
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
                let (status, _) = self
                    .mmproj
                    .as_ref()
                    .unwrap()
                    .eval_chunk(chunk, &self.ctx, pos_next, seq_id, self.n_batch as i32, true)?;
                if status != 0 {
                    Self::emit_event(events, req, EventKind::Error {
                        message: format!("Multimodal chunk eval failed: {}", status),
                        request: RequestHandle::new(req_id.clone(), req.cancel_flag.clone()),
                    });
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
        // Close the prep worker channel so recv() exits.
        self.prep_tx.take();

        if let Some(handle) = self._prep_handle.take() {
            let _ = handle.join();
        }
    }
}
