use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, Condvar};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tokio::sync::mpsc;

use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::mtmd::{mtmd_default_marker, MtmdBitmap, MtmdContext, MtmdInputText};
use llama_cpp_2::token::LlamaToken;
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::model::AddBos;
use encoding_rs::Decoder;
use super::config::{ContextParams, ModelParams, SamplerParams};
use super::session;
use super::chat;
use super::scheduler::{BatchScheduler, Request};

#[derive(Clone)]
pub struct ServiceMessage {
    pub role: String,
    pub content: String,
    pub images: Vec<String>,
}

pub struct SessionState {
    pub seq_id: i32,
    pub name: String,
    pub history: Vec<ServiceMessage>,
    pub tokens: Vec<LlamaToken>,
    pub n_past: i32,
    pub sampler: Option<LlamaSampler>,
    pub sampler_params: Option<SamplerParams>,
    pub decoder: Decoder,
    pub pending_token: Option<LlamaToken>,
    pub accept_prompt_tokens: bool,
}

struct ServiceState {
    sessions: HashMap<String, SessionState>,
    scheduler: BatchScheduler,
    free_seq_ids: Vec<i32>,
    free_seq_ids_present: Vec<bool>,
    step_counter: u64,
}



pub struct LlamaService<'a> {
    model: &'a LlamaModel,
    ctx: Arc<Mutex<LlamaContext<'a>>>,
    mtmd: Option<MtmdContext>,
    n_seq_max: u32,
    state: Mutex<ServiceState>,
    streams: Mutex<HashMap<String, mpsc::UnboundedSender<StreamEvent>>>,
    wake: Arc<ServiceWake>,
    metrics: Arc<ServiceMetrics>,
}

// SAFETY: This service is guarded internally with mutexes and is intended to be used
// on a single runtime. Marking it Send/Sync enables Axum state + spawn_blocking use.
// If llama.cpp or mtmd contexts are not thread-safe, this may still be unsafe.
unsafe impl<'a> Send for LlamaService<'a> {}
unsafe impl<'a> Sync for LlamaService<'a> {}

impl<'a> LlamaService<'a> {
    // Lock order when multiple locks are needed: state -> ctx.
    fn ensure_sampler_for_session(&self, session: &mut SessionState, params: &SamplerParams) {
        let needs_rebuild = session
            .sampler_params
            .as_ref()
            .map_or(true, |existing| existing != params);

        if needs_rebuild {
            let mut sampler = self.build_sampler(params);
            if session.accept_prompt_tokens {
                sampler = sampler.with_tokens(session.tokens.iter());
            }
            session.sampler = Some(sampler);
            session.sampler_params = Some(params.clone());
        }
    }

    fn reset_sampler_for_session(&self, session: &mut SessionState) {
        session.sampler = None;
        session.sampler_params = None;
    }

    fn reset_decoder_for_session(&self, session: &mut SessionState) {
        session.decoder = encoding_rs::UTF_8.new_decoder();
    }

    fn trim_session_prefix(&self, session: &mut SessionState, trim: usize) {
        if trim == 0 {
            return;
        }

        let trim = trim.min(session.tokens.len());
        if trim == 0 {
            return;
        }

        session.tokens.drain(0..trim);
        session.n_past = (session.n_past - trim as i32).max(0);

        if let Some(params) = session.sampler_params.clone() {
            let mut sampler = self.build_sampler(&params);
            if session.accept_prompt_tokens {
                sampler = sampler.with_tokens(session.tokens.iter());
            }
            session.sampler = Some(sampler);
        } else {
            session.sampler = None;
        }
    }

    fn alloc_seq_id(&self) -> Result<i32, Box<dyn std::error::Error>> {
        let mut state = self.state.lock().unwrap();
        while let Some(seq_id) = state.free_seq_ids.pop() {
            if seq_id < 0 {
                continue;
            }
            let idx = seq_id as usize;
            if idx >= state.free_seq_ids_present.len() {
                continue;
            }
            if state.free_seq_ids_present[idx] {
                state.free_seq_ids_present[idx] = false;
                return Ok(seq_id);
            }
        }
        Err("No free seq_id available".into())
    }

    fn cleanup_seq_ids(&self, cleared_all: &[i32], extra_clear: &[i32]) {
        if cleared_all.is_empty() {
            return;
        }

        let mut state = self.state.lock().unwrap();
        for &seq_id in cleared_all {
            let _ = state.scheduler.free_slot_by_seq_id(seq_id);
        }

        if !extra_clear.is_empty() {
            let mut ctx = self.ctx.lock().unwrap();
            for &seq_id in extra_clear {
                let _ = ctx.clear_kv_cache_seq(Some(seq_id as u32), None, None);
            }
        }

        state.scheduler.mark_kv_cleared(cleared_all);

        for &seq_id in cleared_all {
            if seq_id < 0 {
                continue;
            }
            let idx = seq_id as usize;
            if idx >= state.free_seq_ids_present.len() {
                continue;
            }
            if state.free_seq_ids_present[idx] {
                continue;
            }
            state.free_seq_ids_present[idx] = true;
            state.free_seq_ids.push(seq_id);
        }
    }

    fn take_pending_token(&self, session_name: &str) -> Option<LlamaToken> {
        let mut state = self.state.lock().unwrap();
        let session = state.sessions.get_mut(session_name)?;
        session.pending_token.take()
    }

    pub fn new(
        backend: &'a LlamaBackend,
        model: &'a LlamaModel,
        ctx_params: &ContextParams,
        model_params: &ModelParams,
    ) -> Result<Self, Box<dyn std::error::Error>> {

        let llama_ctx_params: llama_cpp_2::context::params::LlamaContextParams = ctx_params.into();
        let ctx = model.new_context(backend, llama_ctx_params)?;

        let mtmd = if let Some(mmproj) = &model_params.mmproj_path {
             Some(MtmdContext::init_from_file(mmproj, model, &llama_cpp_2::mtmd::MtmdContextParams::default())?)
        } else {
             None
        };

        let ctx = Arc::new(Mutex::new(ctx));
        Ok(Self {
            model,
            ctx,
            mtmd,
            n_seq_max: ctx_params.n_seq_max,
            state: Mutex::new(ServiceState {
                sessions: HashMap::new(),
                scheduler: BatchScheduler::new(
                    ctx_params.n_seq_max as usize,
                    ctx_params.n_batch as usize,
                ),
                free_seq_ids: (0..ctx_params.n_seq_max as i32).rev().collect(),
                free_seq_ids_present: vec![true; ctx_params.n_seq_max as usize],
                step_counter: 0,
            }),
            streams: Mutex::new(HashMap::new()),
            wake: Arc::new(ServiceWake::new()),
            metrics: Arc::new(ServiceMetrics::new()),
        })
    }

    pub fn load_session(&self, session_name: &str) -> Result<bool, Box<dyn std::error::Error>> {
        {
            let state = self.state.lock().unwrap();
            if state.sessions.contains_key(session_name) {
                return Ok(true);
            }
        }

        let seq_id = self.alloc_seq_id()?;

        let (tokens, n_past) = {
            let mut ctx = self.ctx.lock().unwrap();
            match session::load_session(&mut ctx, session_name) {
                Ok((tokens, n_past)) => (tokens, n_past),
                Err(_) => {
                    let mut state = self.state.lock().unwrap();
                    state.sessions.insert(session_name.to_string(), SessionState {
                        seq_id,
                        name: session_name.to_string(),
                        history: Vec::new(),
                        tokens: Vec::new(),
                        n_past: 0,
                        sampler: None,
                        sampler_params: None,
                        decoder: encoding_rs::UTF_8.new_decoder(),
                        pending_token: None,
                        accept_prompt_tokens: true,
                    });
                    return Ok(false);
                }
            }
        };

        let mut state = self.state.lock().unwrap();
        if state.sessions.contains_key(session_name) {
            if seq_id >= 0 {
                let idx = seq_id as usize;
                if idx < state.free_seq_ids_present.len() && !state.free_seq_ids_present[idx] {
                    state.free_seq_ids_present[idx] = true;
                    state.free_seq_ids.push(seq_id);
                }
            }
            return Ok(true);
        }

        state.sessions.insert(session_name.to_string(), SessionState {
            seq_id,
            name: session_name.to_string(),
            history: Vec::new(),
            tokens,
            n_past,
            sampler: None,
            sampler_params: None,
            decoder: encoding_rs::UTF_8.new_decoder(),
            pending_token: None,
            accept_prompt_tokens: true,
        });
        Ok(true)
    }

    pub fn save_session(&self, session_name: &str) -> Result<(), Box<dyn std::error::Error>> {
        let state = self.state.lock().unwrap();
        let session = state.sessions.get(session_name).ok_or("Session not found")?;

        let ctx = self.ctx.lock().unwrap();
        session::save_session(&ctx, session_name, &session.tokens)?;
        Ok(())
    }

    pub fn free_session(&self, session_name: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.cancel_session(session_name)
    }

    pub fn cancel_session(&self, session_name: &str) -> Result<(), Box<dyn std::error::Error>> {
        let mut cleared_all: Vec<i32> = Vec::new();
        let mut extra_clear: Vec<i32> = Vec::new();

        {
            let mut state = self.state.lock().unwrap();
            state.scheduler.remove_requests(session_name);

            if let Some(mut session) = state.sessions.remove(session_name) {
                self.reset_sampler_for_session(&mut session);
                self.reset_decoder_for_session(&mut session);
                let seq_id = session.seq_id;
                if seq_id >= 0 {
                    let _ = state.scheduler.free_slot_by_seq_id(seq_id);
                    cleared_all.push(seq_id);
                    extra_clear.push(seq_id);
                }
            } else if let Some(seq_id) = state.scheduler.free_slot(session_name) {
                if seq_id >= 0 {
                    cleared_all.push(seq_id);
                    extra_clear.push(seq_id);
                }
            }
        }

        if !cleared_all.is_empty() {
            self.cleanup_seq_ids(&cleared_all, &extra_clear);
        }

        Ok(())
    }

    pub fn fork_session(&self, source_session_name: &str, dest_session_name: &str) -> Result<(), Box<dyn std::error::Error>> {
        session::fork_session(source_session_name, dest_session_name)?;
        Ok(())
    }

    pub fn add_message(
        &self,
        session_name: &str,
        role: &str,
        content: &str,
        images: Vec<String>,
        params: &SamplerParams,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut state = self.state.lock().unwrap();
        let session = state
            .sessions
            .get_mut(session_name)
            .ok_or("Session not found. Call load_session first.")?;

        session.history.push(ServiceMessage {
            role: role.to_string(),
            content: content.to_string(),
            images: images.clone(),
        });

        let formatted_content = if role == "user" {
             match chat::apply_chat_template(self.model, content) {
                 Ok(s) => s,
                 Err(_) => content.to_string(),
             }
        } else {
             content.to_string()
        };

        let has_images = !images.is_empty();

        drop(state);

        if has_images {
            let mtmd = self
                .mtmd
                .as_ref()
                .ok_or("MTMD context not initialized (missing mmproj)")?;
            if !mtmd.support_vision() {
                return Err("MTMD context does not support vision".into());
            }

            let marker = mtmd_default_marker();
            let mut mtmd_text = content.to_string();
            if !mtmd_text.contains(marker) {
                for _ in 0..images.len() {
                    if !mtmd_text.ends_with(' ') {
                        mtmd_text.push(' ');
                    }
                    mtmd_text.push_str(marker);
                }
            }

            let mut bitmaps = Vec::with_capacity(images.len());
            for path in &images {
                bitmaps.push(MtmdBitmap::from_file(mtmd, path)?);
            }
            let bitmap_refs: Vec<&MtmdBitmap> = bitmaps.iter().collect();

            let chunks = mtmd.tokenize(
                MtmdInputText {
                    text: mtmd_text,
                    add_special: true,
                    parse_special: true,
                },
                &bitmap_refs,
            )?;

            let mut state = self.state.lock().unwrap();
            let mut ctx = self.ctx.lock().unwrap();
            let n_batch = state.scheduler.n_batch() as i32;
            let session = state
                .sessions
                .get_mut(session_name)
                .ok_or("Session not found. Call load_session first.")?;
            session.accept_prompt_tokens = params.grammar.is_none();

            let max_ctx = ctx.n_ctx() as i32;
            if max_ctx <= 0 {
                return Err("Invalid context size".into());
            }

            let n_pos = chunks.total_positions();
            if n_pos > max_ctx {
                return Err(format!(
                    "multimodal prompt positions {} exceed context window {}",
                    n_pos, max_ctx
                )
                .into());
            }

            let overflow = session.n_past + n_pos - max_ctx;
            if overflow > 0 {
                let trim = overflow as u32;
                let _ = ctx.clear_kv_cache_seq(Some(session.seq_id as u32), Some(0), Some(trim));
                let _ = ctx.kv_cache_seq_add(session.seq_id, Some(trim), None, -overflow);
                self.trim_session_prefix(session, overflow as usize);
            }

            let new_n_past = chunks.eval_chunks(
                mtmd,
                &ctx,
                session.n_past,
                session.seq_id,
                n_batch,
                true,
            )?;
            session.n_past = new_n_past;

            self.ensure_sampler_for_session(session, params);
            for index in 0..chunks.len() {
                if let Some(chunk) = chunks.get(index) {
                    if let Some(tokens) = chunk.text_tokens() {
                        session.tokens.extend_from_slice(tokens);
                        if session.accept_prompt_tokens {
                            if let Some(sampler) = &mut session.sampler {
                                sampler.accept_many(tokens.iter());
                            }
                        }
                    }
                }
            }

            let (seq_id, n_past) = {
                let sampler = session.sampler.as_mut().ok_or("Sampler not initialized")?;
                let mut data = ctx.token_data_array();
                data.apply_sampler(sampler);
                let token = data
                    .selected_token()
                    .ok_or("Sampler did not select a token")?;
                sampler.accept(token);
                session.tokens.push(token);
                session.pending_token = Some(token);
                (session.seq_id, session.n_past)
            };

            state
                .scheduler
                .ensure_slot_active(session_name, seq_id, n_past as usize)?;

            return Ok(());
        }

        self.add_request(session_name, &formatted_content, &params)
    }

    fn build_sampler(&self, params: &SamplerParams) -> LlamaSampler {
        let mut samplers = Vec::new();

        if params.penalty_last_n != 0 || params.penalty_repeat != 1.0 {
            samplers.push(LlamaSampler::penalties(
                params.penalty_last_n,
                params.penalty_repeat,
                params.penalty_freq,
                params.penalty_present
            ));
        }

        if let Some(grammar) = &params.grammar {
            match LlamaSampler::grammar(self.model, &grammar.grammar, &grammar.root) {
                Ok(sampler) => samplers.push(sampler),
                Err(e) => eprintln!("Failed to initialize grammar sampler: {}", e),
            }
        }

        samplers.push(LlamaSampler::top_k(params.top_k));
        samplers.push(LlamaSampler::top_p(params.top_p, 1));
        samplers.push(LlamaSampler::min_p(params.min_p, 1));
        samplers.push(LlamaSampler::temp(params.temp));
        samplers.push(LlamaSampler::dist(params.seed));

        LlamaSampler::chain_simple(samplers)
    }

    pub fn stream(&self, session_name: &str, params: &SamplerParams) -> impl Iterator<Item = String> + '_ {
        {
            let mut state = self.state.lock().unwrap();
            if let Some(session) = state.sessions.get_mut(session_name) {
                session.sampler_params = Some(params.clone());
                self.ensure_sampler_for_session(session, params);
            }
        }
        ServiceStream {
            service: self,
            session_name: session_name.to_string(),
            completed: Vec::new(),
            buffer: VecDeque::new(),
            finished: false,
        }
    }
}

struct ServiceStream<'a, 'b> {
    service: &'b LlamaService<'a>,
    session_name: String,
    completed: Vec<(String, LlamaToken)>,
    buffer: VecDeque<String>,
    finished: bool,
}

impl<'a, 'b> Iterator for ServiceStream<'a, 'b> {
    type Item = String;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }

        loop {
            if let Some(piece) = self.buffer.pop_front() {
                return Some(piece);
            }

            if self.completed.is_empty() {
                if let Some(token) = self.service.take_pending_token(&self.session_name) {
                    if let Ok(piece) = self.service.decode_token(&self.session_name, token) {
                        self.buffer.push_back(piece);
                    }
                    self.completed.push((self.session_name.clone(), token));
                    continue;
                }
            }

            let step = match self.service.service_step(&self.completed) {
                Ok(step) => step,
                Err(e) => {
                    eprintln!("service_step failed: {}", e);
                    self.finished = true;
                    return None;
                }
            };

            self.completed = step.completed.clone();

            if step.emitted.is_empty() {
                if self.completed.iter().any(|(sid, tok)| sid == &self.session_name && self.service.model.is_eog_token(*tok)) {
                    self.finished = true;
                    return None;
                }
                continue;
            }

            for (sid, tok) in step.emitted {
                if sid == self.session_name {
                    if let Ok(piece) = self.service.decode_token(&sid, tok) {
                        self.buffer.push_back(piece);
                    }
                }
            }

            if self.completed.iter().any(|(sid, tok)| sid == &self.session_name && self.service.model.is_eog_token(*tok)) {
                self.finished = true;
            }
        }
    }
}

pub struct StepResult {
    /// Tokens to feed back into the scheduler (includes EOS).
    pub completed: Vec<(String, LlamaToken)>,
    /// Tokens safe to emit to the user (excludes EOS).
    pub emitted: Vec<(String, LlamaToken)>,
}

impl<'a> LlamaService<'a> {
    pub fn generate(&self, session_name: &str, params: &SamplerParams) -> Result<String, Box<dyn std::error::Error>> {
        let mut output = String::new();
        for part in self.stream(session_name, params) {
            output.push_str(&part);
        }
        Ok(output)
    }


    pub fn add_request(&self, session_name: &str, prompt: &str, params: &SamplerParams) -> Result<(), Box<dyn std::error::Error>> {
        let tokens = self.model.str_to_token(prompt, AddBos::Always)?;
        if tokens.is_empty() {
            return Err("Tokenization produced zero tokens".into());
        }
        let mut state = self.state.lock().unwrap();
        let seq_id = if let Some(session) = state.sessions.get_mut(session_name) {
            session.accept_prompt_tokens = params.grammar.is_none();
            self.ensure_sampler_for_session(session, params);
            if session.accept_prompt_tokens {
                if let Some(sampler) = &mut session.sampler {
                    sampler.accept_many(tokens.iter());
                }
            }
            session.tokens.extend_from_slice(&tokens);
            session.seq_id
        } else {
            let mut id: Option<i32> = None;
            while let Some(candidate) = state.free_seq_ids.pop() {
                if candidate < 0 {
                    continue;
                }
                let idx = candidate as usize;
                if idx < state.free_seq_ids_present.len() && state.free_seq_ids_present[idx] {
                    state.free_seq_ids_present[idx] = false;
                    id = Some(candidate);
                    break;
                }
            }
            let id = id.ok_or_else(|| Box::<dyn std::error::Error>::from("No free seq_id available"))?;
            state.sessions.insert(session_name.to_string(), SessionState {
                seq_id: id,
                name: session_name.to_string(),
                history: Vec::new(),
                tokens: Vec::new(),
                n_past: 0,
                sampler: None,
                sampler_params: None,
                decoder: encoding_rs::UTF_8.new_decoder(),
                pending_token: None,
                accept_prompt_tokens: params.grammar.is_none(),
            });
            let session = state.sessions.get_mut(session_name).expect("session just inserted");
            self.ensure_sampler_for_session(session, params);
            if session.accept_prompt_tokens {
                if let Some(sampler) = &mut session.sampler {
                    sampler.accept_many(tokens.iter());
                }
            }
            session.tokens.extend_from_slice(&tokens);
            id
        };
        if seq_id < 0 || seq_id as u32 >= self.n_seq_max {
            return Err(format!(
                "internal seq_id {} out of range (n_seq_max={})",
                seq_id, self.n_seq_max
            ).into());
        }

        let req = Request {
            session_id: session_name.to_string(),
            prompt_tokens: tokens,
            processed_tokens: 0,
        };

        state.scheduler.add_request(req);
        self.wake.notify();
        Ok(())
    }

    /// The new "tick" function. Takes map of (session_id -> last_generated_token) from previous step.
    /// Returns both scheduler feedback tokens and user-emittable tokens.
    pub fn service_step(&self, completed_tokens: &[(String, LlamaToken)]) -> Result<StepResult, Box<dyn std::error::Error>> {
        let mut pending_emitted: Vec<(String, LlamaToken)> = Vec::new();
        let mut completed_tokens: Vec<(String, LlamaToken)> = completed_tokens.to_vec();
        {
            let mut state = self.state.lock().unwrap();
            for (session_id, session) in state.sessions.iter_mut() {
                if let Some(token) = session.pending_token.take() {
                    completed_tokens.push((session_id.clone(), token));
                    if !self.model.is_eog_token(token) {
                        pending_emitted.push((session_id.clone(), token));
                    }
                }
            }
        }

        let finished_sessions: Vec<String> = completed_tokens
            .iter()
            .filter(|(_, token)| self.model.is_eog_token(*token))
            .map(|(session_id, _)| session_id.clone())
            .collect();
        let session_snapshot: HashMap<String, (i32, usize)> = {
            let state = self.state.lock().unwrap();
            state
                .sessions
                .iter()
                .map(|(name, session)| (name.clone(), (session.seq_id, session.n_past as usize)))
                .collect()
        };
        let action = {
            let mut state = self.state.lock().unwrap();
            state.scheduler.step(
                &completed_tokens,
                &finished_sessions,
                |session_id| session_snapshot.get(session_id).map(|(seq_id, _)| *seq_id),
                |session_id| session_snapshot.get(session_id).map(|(_, n_past)| *n_past),
            )?
        };
        let mut batch = action.batch;
        let batch_tokens = batch.n_tokens();
        let mut completed = Vec::new();
        let mut emitted = Vec::new();
        let no_batch = batch_tokens == 0;

        let mut extra_clear: Vec<i32> = Vec::new();
        {
            let mut state = self.state.lock().unwrap();
            let mut ctx = self.ctx.lock().unwrap();

            for &seq_id in &action.clear_seq_ids {
                let _ = ctx.clear_kv_cache_seq(Some(seq_id as u32), None, None);
            }

            let max_ctx = ctx.n_ctx() as i32;
            if max_ctx > 0 {
                for (seq_id, session_id, count) in &action.touched {
                    if let Some(session) = state.sessions.get_mut(session_id.as_str()) {
                        let count = *count as i32;
                        if count > max_ctx {
                            return Err(format!(
                                "batch size {} exceeds context window {}",
                                count, max_ctx
                            )
                            .into());
                        }
                        let overflow = session.n_past + count - max_ctx;
                        if overflow > 0 {
                            let trim = overflow as u32;
                            let _ = ctx.clear_kv_cache_seq(Some(*seq_id as u32), Some(0), Some(trim));
                            let _ = ctx.kv_cache_seq_add(*seq_id, Some(trim), None, -overflow);
                            self.trim_session_prefix(session, overflow as usize);
                        }
                    }
                }
            }

            if !no_batch {
                if let Err(e) = ctx.decode(&mut batch) {
                    let mut cleared_all = action.clear_seq_ids.clone();
                    cleared_all.extend(action.touched.iter().map(|(seq_id, _, _)| *seq_id));
                    cleared_all.sort_unstable();
                    cleared_all.dedup();
                    drop(ctx);
                    drop(state);
                    self.cleanup_seq_ids(&cleared_all, &cleared_all);
                    return Err(e.into());
                }
            }

            if !no_batch {
                for (seq_id, session_id, count) in &action.touched {
                    if let Some(session) = state.sessions.get_mut(session_id.as_str()) {
                        debug_assert_eq!(
                            session.seq_id,
                            *seq_id,
                            "session.seq_id mismatch for {}",
                            session_id
                        );
                        session.n_past += *count as i32;
                    }
                }

                state.step_counter += 1;
                if state.step_counter % 50 == 0 {
                    for (seq_id, session_id, _) in &action.touched {
                        if let Some(session) = state.sessions.get_mut(session_id.as_str()) {
                            let pos = ctx.kv_cache_seq_pos_max(*seq_id);
                            session.n_past = if pos >= 0 { pos + 1 } else { 0 };
                        }
                    }
                }

                for (batch_index, expected_seq_id, session_id) in &action.routing {
                    let session = match state.sessions.get_mut(session_id.as_str()) {
                        Some(session) => session,
                        None => {
                            return Err(format!(
                                "Session {} missing during sampling for batch index {}",
                                session_id, batch_index
                            ).into());
                        }
                    };
                    debug_assert_eq!(
                        session.seq_id,
                        *expected_seq_id,
                        "session.seq_id mismatch for {}",
                        session_id
                    );

                    if session.sampler.is_none() {
                        let params = session.sampler_params.clone().unwrap_or_else(SamplerParams::default);
                        self.ensure_sampler_for_session(session, &params);
                    }

                let sampler = session.sampler.as_mut().expect("sampler must be initialized");
                let grammar_enabled = session
                    .sampler_params
                    .as_ref()
                    .and_then(|p| p.grammar.as_ref())
                    .is_some();
                let token = if grammar_enabled {
                    let mut data = ctx.token_data_array_ith(*batch_index);
                    data.apply_sampler(sampler);
                    data.selected_token()
                        .ok_or("Sampler did not select a token")?
                } else {
                    sampler.sample(&ctx, *batch_index)
                };
                sampler.accept(token);
                session.tokens.push(token);
                    completed.push((session_id.clone(), token));
                if !self.model.is_eog_token(token) {
                    emitted.push((session_id.clone(), token));
                }
                }
            }

            if !finished_sessions.is_empty() {
                use std::collections::HashSet;
                let cleared: HashSet<i32> = action.clear_seq_ids.iter().copied().collect();
                for session_id in &finished_sessions {
                    if let Some(session) = state.sessions.remove(session_id.as_str()) {
                        if !cleared.contains(&session.seq_id) {
                            extra_clear.push(session.seq_id);
                        }
                    }
                }
            }
        }

        let mut cleared_all = action.clear_seq_ids.clone();
        cleared_all.extend(extra_clear.iter().copied());
        if !cleared_all.is_empty() {
            cleared_all.sort_unstable();
            cleared_all.dedup();
            self.cleanup_seq_ids(&cleared_all, &extra_clear);
        }

        if no_batch {
            return Ok(StepResult { completed: Vec::new(), emitted: pending_emitted });
        }

        if !pending_emitted.is_empty() {
            emitted.extend(pending_emitted);
        }

        if !emitted.is_empty() {
            self.metrics
                .emitted_tokens
                .fetch_add(emitted.len() as u64, Ordering::Relaxed);
        }

        Ok(StepResult { completed, emitted })
    }

    pub fn metrics_snapshot(&self) -> ServiceMetricsSnapshot {
        let elapsed = self.metrics.started_at.elapsed().as_secs_f64();
        let tokens = self.metrics.emitted_tokens.load(Ordering::Relaxed);
        let tps = if elapsed > 0.0 { tokens as f64 / elapsed } else { 0.0 };
        ServiceMetricsSnapshot {
            emitted_tokens: tokens,
            tps,
            uptime_secs: elapsed,
        }
    }

    pub fn decode_token(&self, session_name: &str, token: LlamaToken) -> Result<String, Box<dyn std::error::Error>> {
        let mut state = self.state.lock().unwrap();
        let session = state.sessions.get_mut(session_name).ok_or("Session not found")?;

        let piece = self.model.token_to_piece(token, &mut session.decoder, false, None)
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
        Ok(piece)
    }

    pub fn is_eog_token(&self, token: LlamaToken) -> bool {
        self.model.is_eog_token(token)
    }

    pub fn register_stream(&self, session_name: &str) -> mpsc::UnboundedReceiver<StreamEvent> {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut streams = self.streams.lock().unwrap();
        streams.insert(session_name.to_string(), tx);
        rx
    }

    pub fn unregister_stream(&self, session_name: &str) {
        let mut streams = self.streams.lock().unwrap();
        streams.remove(session_name);
    }

    pub fn send_event(&self, session_name: &str, event: StreamEvent) -> SendStatus {
        let mut streams = self.streams.lock().unwrap();
        let Some(tx) = streams.get(session_name) else {
            return SendStatus::NoListener;
        };

        if tx.send(event).is_ok() {
            SendStatus::Sent
        } else {
            streams.remove(session_name);
            SendStatus::Closed
        }
    }

    pub fn has_pending_work(&self) -> bool {
        let state = self.state.lock().unwrap();
        state.scheduler.has_pending_work()
    }

    pub fn wait_for_work(&self) {
        self.wake.wait();
    }

    pub fn is_idle(&self) -> bool {
        let state = self.state.lock().unwrap();
        if state.sessions.values().any(|session| session.pending_token.is_some()) {
            return false;
        }
        !state.scheduler.has_pending_work()
    }
}

struct ServiceMetrics {
    started_at: Instant,
    emitted_tokens: AtomicU64,
}

impl ServiceMetrics {
    fn new() -> Self {
        Self {
            started_at: Instant::now(),
            emitted_tokens: AtomicU64::new(0),
        }
    }
}

pub struct ServiceMetricsSnapshot {
    pub emitted_tokens: u64,
    pub tps: f64,
    pub uptime_secs: f64,
}

struct ServiceWake {
    mutex: Mutex<()>,
    condvar: Condvar,
}

impl ServiceWake {
    fn new() -> Self {
        Self {
            mutex: Mutex::new(()),
            condvar: Condvar::new(),
        }
    }

    fn notify(&self) {
        self.condvar.notify_one();
    }

    fn wait(&self) {
        let guard = self.mutex.lock().unwrap();
        let _ = self.condvar.wait_timeout(guard, std::time::Duration::from_millis(10)).unwrap();
    }
}

#[derive(Debug, Clone)]
pub enum StreamEvent {
    Token(String),
    Done,
}

#[derive(Debug, Clone, Copy)]
pub enum SendStatus {
    Sent,
    Closed,
    NoListener,
}
