use std::collections::HashMap;
use std::path::PathBuf;

use std::io::{Read, Write};
use parking_lot::RwLock;

use crate::llm::context::Context;
use crate::llm::tokenizer::Token;
use crate::llm::error::{Result, Error};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheTier {
    Vram,
    Ram,
    Disk,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Save { path: String },
    Idle,
    Delete,
}

#[derive(Debug, Clone)]
pub struct Session {
    pub id: String,
    pub tier: CacheTier,
    pub last_used: std::time::Instant,
    
    // RAM tier data
    pub ram_state: Option<Vec<u8>>,
    pub ram_kv_head: Option<usize>,
    
    // Disk tier metadata
    pub disk_path: Option<PathBuf>,
    pub pending_action: Option<Action>,
    
    // Recovery Metadata
    pub tokens: Vec<Token>,
    pub n_keep: usize,
    
    // Runtime VRAM mapping
    pub vram_seq_id: Option<i32>,
    
    // Verified KV Checkpoint (Dual Counter)
    // - tokens.len(): Intent (Planned)
    // - kv_head: Actual (Evaluated in KV)
    pub kv_head: usize,
}

#[repr(C)]
struct StateHeader {
    magic: [u8; 4],     // b"HUGN"
    version: u32,       // 1
    n_tokens: u32,      // Total tokens in history
    n_keep: u32,        // System prompt tokens
    reserved: [u8; 16], // Padding
}

pub struct KvCacheManager {
    pub sessions: RwLock<HashMap<String, Session>>,
    pub unified_memory_mode: bool,
}

impl KvCacheManager {
    pub fn new(unified_memory_mode: bool) -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            unified_memory_mode,
        }
    }
    
    pub fn register_session(&self, id: String, _tokens: Vec<Token>, n_keep: usize) {
        let mut sessions = self.sessions.write();
        if !sessions.contains_key(&id) {
            sessions.insert(id.clone(), Session {
                id,
                tier: CacheTier::Vram,
                last_used: std::time::Instant::now(),
                ram_state: None,
                ram_kv_head: None,
                disk_path: None,
                pending_action: None,
                tokens: Vec::new(),
                n_keep,
                vram_seq_id: None,
                kv_head: 0,
            });
        }
    }

    pub fn update_tokens(&self, id: &str, tokens: Vec<Token>, n_keep: usize) {
         let mut sessions = self.sessions.write();
         if let Some(session) = sessions.get_mut(id) {
             session.tokens = tokens;
             session.n_keep = n_keep;
             session.kv_head = session.kv_head.min(session.tokens.len());
             session.last_used = std::time::Instant::now();
         }
    }
    
    pub fn touch(&self, id: &str) {
        let mut sessions = self.sessions.write();
        if let Some(session) = sessions.get_mut(id) {
            session.last_used = std::time::Instant::now();
        }
    }

    // Move from VRAM to RAM (or Disk if unified)
    pub fn evict(&self, ctx: &mut Context, seq_id: i32, session_id: &str) -> Result<()> {
        let (disk_path, kv_head, n_keep, unified) = {
            let sessions = self.sessions.read();
            let session = sessions.get(session_id).ok_or(Error::BackendError("Session not found".to_string()))?;
            (session.disk_path.clone(), session.kv_head, session.n_keep, self.unified_memory_mode)
        };

        let state_data = ctx.state_seq_get_data(seq_id)?;

        // Clear from VRAM
        ctx.kv_cache_seq_rm(seq_id, -1, -1);

        let mut sessions = self.sessions.write();
        let session = sessions.get_mut(session_id).ok_or(Error::BackendError("Session not found".to_string()))?;
        session.vram_seq_id = None;

        if unified {
            if let Some(path) = &disk_path {
                Self::write_state_file(path, &state_data, kv_head as u32, n_keep as u32)?;
                session.disk_path = Some(path.clone());
                session.tier = CacheTier::Disk;
                session.ram_state = None;
                session.ram_kv_head = None;
            } else {
                session.ram_state = Some(state_data);
                session.ram_kv_head = Some(kv_head);
                session.tier = CacheTier::Ram;
            }
        } else {
            session.ram_state = Some(state_data);
            session.ram_kv_head = Some(kv_head);
            session.tier = CacheTier::Ram;
        }

        Ok(())
    }

    pub fn restore(&self, ctx: &mut Context, seq_id: i32, session_id: &str) -> Result<usize> {
        let mut sessions = self.sessions.write();
        let session = sessions.get_mut(session_id).ok_or(Error::BackendError("Session not found".to_string()))?;

        // 1. Try RAM
        if let Some(data) = &session.ram_state {
             let restored = session.ram_kv_head.unwrap_or(session.tokens.len());
             ctx.state_seq_set_data(seq_id, data);
             session.tier = CacheTier::Vram;
             if !self.unified_memory_mode {
                session.ram_state = None; // Free RAM if not unified (since we just copied to VRAM)
                session.ram_kv_head = None;
             }
             session.vram_seq_id = Some(seq_id);
             session.kv_head = restored;
             return Ok(restored);
        }

        // 2. Try Disk
        if let Some(path) = &session.disk_path {
            let (data, n_tokens, n_keep) = Self::read_state_file(path)?;
            
            // Validate: Loaded state must be a prefix of the current target tokens (or match exactly)
            // If loaded state is larger than target, or 0, it's invalid/useless? 
            // Actually, if loaded is larger, we might be reverting? 
            // For now: Accept if n_tokens <= session.tokens.len()
            
            if (n_tokens as usize) > session.tokens.len() {
                return Err(Error::BackendError("Disk state has more tokens than intent history".to_string()));
            }
            
            // We just assume session_id collision implies same context.
            ctx.state_seq_set_data(seq_id, &data);
            session.tier = CacheTier::Vram;
            session.n_keep = n_keep as usize;
            session.vram_seq_id = Some(seq_id);
            
            // Note: We do NOT update session.tokens from disk, because session.tokens is the Target (Request).
            // We return n_tokens to tell the engine how much we restored.
            let restored = n_tokens as usize;
            session.kv_head = restored;
            return Ok(restored);
        }

        Err(Error::BackendError("No state data found in RAM or Disk".to_string()))
    }
    
    // Explicitly set VRAM sequence mapping (for when we reuse an existing sequence without restore)
    pub fn set_vram_seq(&self, session_id: &str, seq_id: i32) -> Result<()> {
        let mut sessions = self.sessions.write();
        let session = sessions.get_mut(session_id).ok_or(Error::BackendError("Session not found".to_string()))?;
        session.vram_seq_id = Some(seq_id);
        session.tier = CacheTier::Vram;
        Ok(())
    }

    // Release a sequence from any session (except potentially one)
    // Used when overwriting a slot to ensure no other session thinks it owns this seq_id.
    pub fn release_sequence(&self, seq_id: i32, except_session_id: Option<&str>) {
        let mut sessions = self.sessions.write();
        for (id, session) in sessions.iter_mut() {
            if let Some(except) = except_session_id {
                if id == except { continue; }
            }
            if session.vram_seq_id == Some(seq_id) {
                session.vram_seq_id = None;
                // If it was in VRAM tier and we stole its sequence, it effectively drops to RAM?
                // But we don't have the RAM data!
                // So it drops to... Nothing? (Lost state).
                // Or we should evict it properly first? 
                // For robustness: usage of release_sequence implies the caller is about to overwrite.
                // If we want to save, the caller should have evicted.
                // Here we just update metadata to avoid phantom ownership.
                if session.tier == CacheTier::Vram {
                     // It's now technically invalid/lost.
                     // Ideally we should mark it as such.
                     // But for now, just clearing vram_seq_id is enough to prevent reuse.
                }
            }
        }
    }

    // Evict the current owner of seq_id (if any) to preserve state before reuse.
    pub fn evict_seq_owner(
        &self,
        ctx: &mut Context,
        seq_id: i32,
        except_session_id: Option<&str>,
    ) -> Result<Option<String>> {
        let owner_id = {
            let sessions = self.sessions.read();
            sessions
                .iter()
                .find(|(id, s)| {
                    if let Some(except) = except_session_id {
                        if id.as_str() == except {
                            return false;
                        }
                    }
                    s.vram_seq_id == Some(seq_id)
                })
                .map(|(id, _)| id.clone())
        };

        if let Some(id) = owner_id {
            self.evict(ctx, seq_id, &id)?;
            return Ok(Some(id));
        }

        Ok(None)
    }
    
    // VRAM -> Disk (Save)
    pub fn save_to_disk(&self, ctx: &Context, seq_id: i32, session_id: &str, path: &str) -> Result<()> {
        let mut sessions = self.sessions.write();
        let session = sessions.get_mut(session_id).ok_or(Error::BackendError("Session not found".to_string()))?;
        
        let state_data = ctx.state_seq_get_data(seq_id)?;
        session.kv_head = session.kv_head.min(session.tokens.len());
        session.tokens.truncate(session.kv_head);
        Self::write_state_file(&PathBuf::from(path), &state_data, session.kv_head as u32, session.n_keep as u32)?;
        
        session.disk_path = Some(PathBuf::from(path));
        // Note: We don't change tier here unless we also evict.
        Ok(())
    }

    // RAM -> Disk (Save)
    pub fn save_ram_to_disk(&self, session_id: &str, path: &str) -> Result<()> {
        let mut sessions = self.sessions.write();
        let session = sessions.get_mut(session_id).ok_or(Error::BackendError("Session not found".to_string()))?;
        let state_data = session
            .ram_state
            .as_ref()
            .ok_or(Error::BackendError("Session has no RAM state".to_string()))?;

        session.kv_head = session.kv_head.min(session.tokens.len());
        session.tokens.truncate(session.kv_head);
        Self::write_state_file(&PathBuf::from(path), state_data, session.kv_head as u32, session.n_keep as u32)?;
        session.disk_path = Some(PathBuf::from(path));
        session.tier = CacheTier::Disk;
        if !self.unified_memory_mode {
            session.ram_state = None;
        }
        Ok(())
    }
    
    // Internal IO Helpers
    fn write_state_file(path: &PathBuf, data: &[u8], n_tokens: u32, n_keep: u32) -> Result<()> {
        let header = StateHeader {
            magic: *b"HUGN", // Hugind
            version: 1,
            n_tokens,
            n_keep,
            reserved: [0; 16],
        };

        let mut file = std::fs::File::create(path).map_err(|e| Error::BackendError(e.to_string()))?;
        
        // Write Header
        let header_slice = unsafe {
            std::slice::from_raw_parts(
                &header as *const _ as *const u8,
                std::mem::size_of::<StateHeader>()
            )
        };
        file.write_all(header_slice).map_err(|e| Error::BackendError(e.to_string()))?;
        
        // Write Blob
        file.write_all(data).map_err(|e| Error::BackendError(e.to_string()))?;
        
        Ok(())
    }

    fn read_state_file(path: &PathBuf) -> Result<(Vec<u8>, u32, u32)> {
        let mut file = std::fs::File::open(path).map_err(|e| Error::BackendError(e.to_string()))?;
        let len = file.metadata().map_err(|e| Error::BackendError(e.to_string()))?.len();
        
        let header_size = std::mem::size_of::<StateHeader>() as u64;
        if len < header_size {
            return Err(Error::BackendError("State file too small".to_string()));
        }

        let mut header_buf = [0u8; std::mem::size_of::<StateHeader>()];
        file.read_exact(&mut header_buf).map_err(|e| Error::BackendError(e.to_string()))?;
        
        let header: StateHeader = unsafe { std::mem::transmute(header_buf) };
        
        if &header.magic != b"HUGN" {
             return Err(Error::BackendError("Invalid state file magic".to_string()));   
        }
        
        let mut body = Vec::new();
        file.read_to_end(&mut body).map_err(|e| Error::BackendError(e.to_string()))?;
        
        Ok((body, header.n_tokens, header.n_keep))
    }
}
