use std::collections::{HashSet, VecDeque};
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::token::LlamaToken;

#[derive(Clone, Debug)]
pub struct Request {
    pub session_id: String,
    pub prompt_tokens: Vec<LlamaToken>,
    pub processed_tokens: usize,
}

#[derive(Debug)]
pub struct SchedulingAction<'a> {
    pub batch: LlamaBatch<'a>,
    pub routing: Vec<(i32, i32, String)>,
    pub touched: Vec<(i32, String, usize)>,
    pub clear_seq_ids: Vec<i32>,
}

struct SlotState {
    is_active: bool,
    session_id: String,
    seq_id: i32,
    n_past: usize,
    last_token: Option<LlamaToken>,
    needs_kv_clear: bool,
}

impl Default for SlotState {
    fn default() -> Self {
        Self {
            is_active: false,
            session_id: String::new(),
            seq_id: 0,
            n_past: 0,
            last_token: None,
            needs_kv_clear: false,
        }
    }
}

pub struct BatchScheduler {
    queue: VecDeque<Request>,
    slots: Vec<SlotState>,
    n_batch: usize,
}

impl BatchScheduler {
    pub fn new(n_seq_max: usize, n_batch: usize) -> Self {
        Self {
            queue: VecDeque::new(),
            slots: (0..n_seq_max).map(|_| SlotState::default()).collect(),
            n_batch,
        }
    }

    pub fn add_request(&mut self, req: Request) {
        self.queue.push_back(req);
    }

    pub fn remove_requests(&mut self, session_id: &str) -> usize {
        let before = self.queue.len();
        self.queue.retain(|req| req.session_id != session_id);
        before.saturating_sub(self.queue.len())
    }

    pub fn n_batch(&self) -> usize {
        self.n_batch
    }

    pub fn has_pending_work(&self) -> bool {
        if !self.queue.is_empty() {
            return true;
        }

        self.slots.iter().any(|slot| {
            slot.is_active || slot.needs_kv_clear || slot.last_token.is_some()
        })
    }

    pub fn free_slot(&mut self, session_id: &str) -> Option<i32> {
        for slot in &mut self.slots {
            if slot.is_active && slot.session_id == session_id {
                slot.is_active = false;
                slot.session_id.clear();
                slot.n_past = 0;
                slot.last_token = None;
                slot.needs_kv_clear = true;
                return Some(slot.seq_id);
            }
        }
        None
    }

    pub fn free_slot_by_seq_id(&mut self, seq_id: i32) -> bool {
        for slot in &mut self.slots {
            if slot.is_active && slot.seq_id == seq_id {
                slot.is_active = false;
                slot.session_id.clear();
                slot.n_past = 0;
                slot.last_token = None;
                slot.needs_kv_clear = true;
                return true;
            }
        }
        false
    }

    pub fn mark_kv_cleared(&mut self, seq_ids: &[i32]) {
        for seq_id in seq_ids {
            for slot in &mut self.slots {
                if slot.needs_kv_clear && slot.seq_id == *seq_id {
                    slot.needs_kv_clear = false;
                }
            }
        }
    }

    pub fn ensure_slot_active(
        &mut self,
        session_id: &str,
        seq_id: i32,
        n_past: usize,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(slot) = self
            .slots
            .iter_mut()
            .find(|slot| slot.is_active && slot.session_id == session_id)
        {
            slot.seq_id = seq_id;
            slot.n_past = n_past;
            return Ok(());
        }

        if let Some(slot) = self
            .slots
            .iter_mut()
            .find(|slot| !slot.is_active && !slot.needs_kv_clear)
        {
            slot.is_active = true;
            slot.session_id = session_id.to_string();
            slot.seq_id = seq_id;
            slot.n_past = n_past;
            slot.last_token = None;
            return Ok(());
        }

        Err("No free scheduler slot available".into())
    }

    pub fn step<'a>(
        &mut self,
        completed_tokens: &[(String, LlamaToken)],
        finished_sessions: &[String],
        mut seq_id_of: impl FnMut(&str) -> Option<i32>,
        mut n_past_of: impl FnMut(&str) -> Option<usize>,
    ) -> Result<SchedulingAction<'a>, Box<dyn std::error::Error>> {
        let finished_set: HashSet<&str> = finished_sessions.iter().map(String::as_str).collect();
        let mut clear_seq_ids: Vec<i32> = Vec::new();
        for (session_id, token) in completed_tokens {
             for slot in &mut self.slots {
                 if slot.is_active && &slot.session_id == session_id {
                     if finished_set.contains(session_id.as_str()) {
                         eprintln!("Session {} finished (EOG).", session_id);
                        slot.is_active = false;
                        slot.session_id.clear();
                        slot.n_past = 0;
                        slot.last_token = None;
                        slot.needs_kv_clear = true;
                        clear_seq_ids.push(slot.seq_id);
                    } else {
                        slot.last_token = Some(*token);
                    }
                    break;
                }
            }
        }

        let mut batch = LlamaBatch::new(self.n_batch, 1);
        let mut routing: Vec<(i32, i32, String)> = Vec::new();
        let mut touched_map: std::collections::HashMap<i32, (String, usize)> =
            std::collections::HashMap::new();

        for slot in self.slots.iter_mut() {
            if slot.is_active {
                if let Some(token) = slot.last_token {
                    if (batch.n_tokens() as usize) < self.n_batch {
                         let batch_index = batch.n_tokens();
                         batch.add(token, slot.n_past as i32, &[slot.seq_id], true)?;
                         slot.n_past += 1;
                         let entry = touched_map
                             .entry(slot.seq_id)
                             .or_insert_with(|| (slot.session_id.clone(), 0));
                         debug_assert_eq!(
                             entry.0,
                             slot.session_id,
                             "seq_id {} reused within a batch",
                             slot.seq_id
                         );
                         entry.1 += 1;
                         routing.push((batch_index, slot.seq_id, slot.session_id.clone()));
                    } else {
                        break; 
                    }
                }
            }
        }

        while (batch.n_tokens() as usize) < self.n_batch {
             if let Some(req) = self.queue.front_mut() {
                 let slot_idx = self
                     .slots
                     .iter()
                     .position(|s| s.is_active && s.session_id == req.session_id)
                     .or_else(|| self.slots.iter().position(|s| !s.is_active && !s.needs_kv_clear));

                if let Some(slot_idx) = slot_idx {
                     let slot = &mut self.slots[slot_idx];
                     
                     if !slot.is_active {
                        debug_assert!(
                            !slot.needs_kv_clear,
                            "reusing slot before kv clear for session {}",
                            req.session_id
                        );
                         slot.is_active = true;
                         slot.session_id = req.session_id.clone();
                         slot.seq_id = seq_id_of(&req.session_id)
                             .ok_or_else(|| format!("Unknown session {}", req.session_id))?;
                         slot.n_past = n_past_of(&req.session_id).unwrap_or(0);
                         slot.last_token = None;
                     }

                     let remaining_prompt = match req.prompt_tokens.len().checked_sub(req.processed_tokens) {
                         Some(remaining) => remaining,
                         None => {
                             eprintln!(
                                 "Invalid request state for session {}: processed_tokens={} > prompt_len={}",
                                 req.session_id,
                                 req.processed_tokens,
                                 req.prompt_tokens.len()
                             );
                             let seq_id = slot.seq_id;
                             self.queue.pop_front();
                             slot.is_active = false;
                             slot.session_id.clear();
                             slot.n_past = 0;
                             slot.last_token = None;
                             slot.needs_kv_clear = true;
                             clear_seq_ids.push(seq_id);
                             continue;
                         }
                     };

                     let available = self.n_batch - (batch.n_tokens() as usize);
                     let to_process = std::cmp::min(available, remaining_prompt);
                     
                     let entry = touched_map
                         .entry(slot.seq_id)
                         .or_insert_with(|| (slot.session_id.clone(), 0));
                     debug_assert_eq!(
                         entry.0,
                         slot.session_id,
                         "seq_id {} reused within a batch",
                         slot.seq_id
                     );
                     entry.1 += 1;

                     for i in 0..to_process {
                         let token = req.prompt_tokens[req.processed_tokens + i];
                         let is_last = (req.processed_tokens + i) == (req.prompt_tokens.len() - 1);
                         
                         let logits = is_last; 
                         
                         let batch_index = batch.n_tokens();
                         batch.add(token, slot.n_past as i32, &[slot.seq_id], logits)?;
                         
                         slot.n_past += 1;
                         
                         if logits {
                             routing.push((batch_index, slot.seq_id, slot.session_id.clone()));
                         }
                     }
                     
                     req.processed_tokens += to_process;
                     
                     if req.processed_tokens >= req.prompt_tokens.len() {
                         self.queue.pop_front();
                     } else {
                     }
                 } else {
                     break; 
                 }
             } else {
                 break;
             }
        }
        
        let mut touched: Vec<(i32, String, usize)> = Vec::with_capacity(touched_map.len());
        for (seq_id, (session_id, count)) in touched_map {
            touched.push((seq_id, session_id, count));
        }

        Ok(SchedulingAction {
            batch,
            routing,
            touched,
            clear_seq_ids,
        })
    }
}
