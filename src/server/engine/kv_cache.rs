use std::collections::HashMap;
use std::path::PathBuf;

use std::io::{Read, Write};
use parking_lot::RwLock;

use crate::shared::paths;
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
    
    
    pub ram_state: Option<Vec<u8>>,
    pub ram_kv_head: Option<usize>,
    
    
    pub disk_path: Option<PathBuf>,
    pub pending_action: Option<Action>,
    
    
    pub tokens: Vec<Token>,
    pub n_keep: usize,
    
    
    pub vram_seq_id: Option<i32>,
    
    
    
    
    pub kv_head: usize,
}

#[repr(C)]
struct StateHeader {
    magic: [u8; 4],     
    version: u32,       
    n_tokens: u32,      
    n_keep: u32,        
    reserved: [u8; 16], 
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
            let disk_path = paths::sessions_dir().join(format!("{}.bin", id));
            sessions.insert(id.clone(), Session {
                id,
                tier: CacheTier::Vram,
                last_used: std::time::Instant::now(),
                ram_state: None,
                ram_kv_head: None,
                disk_path: Some(disk_path),
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

    
    pub fn evict(&self, ctx: &mut Context, seq_id: i32, session_id: &str) -> Result<()> {
        let (disk_path, kv_head, n_keep, unified) = {
            let sessions = self.sessions.read();
            let session = sessions.get(session_id).ok_or(Error::BackendError("Session not found".to_string()))?;
            (session.disk_path.clone(), session.kv_head, session.n_keep, self.unified_memory_mode)
        };

        let state_data = ctx.state_seq_get_data(seq_id)?;

        
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

        
        if let Some(data) = &session.ram_state {
             let restored = session.ram_kv_head.unwrap_or(session.tokens.len());
             ctx.state_seq_set_data(seq_id, data);
             session.tier = CacheTier::Vram;
             if !self.unified_memory_mode {
                session.ram_state = None; 
                session.ram_kv_head = None;
             }
             session.vram_seq_id = Some(seq_id);
             session.kv_head = restored;
             return Ok(restored);
        }

        
        if let Some(path) = &session.disk_path {
            let (data, n_tokens, n_keep) = Self::read_state_file(path)?;
            
            
            
            
            
            
            if (n_tokens as usize) > session.tokens.len() {
                return Err(Error::BackendError("Disk state has more tokens than intent history".to_string()));
            }
            
            
            ctx.state_seq_set_data(seq_id, &data);
            session.tier = CacheTier::Vram;
            session.n_keep = n_keep as usize;
            session.vram_seq_id = Some(seq_id);
            
            
            
            let restored = n_tokens as usize;
            session.kv_head = restored;
            return Ok(restored);
        }

        Err(Error::BackendError("No state data found in RAM or Disk".to_string()))
    }
    
    
    pub fn set_vram_seq(&self, session_id: &str, seq_id: i32) -> Result<()> {
        let mut sessions = self.sessions.write();
        let session = sessions.get_mut(session_id).ok_or(Error::BackendError("Session not found".to_string()))?;
        session.vram_seq_id = Some(seq_id);
        session.tier = CacheTier::Vram;
        Ok(())
    }

    
    
    pub fn release_sequence(&self, seq_id: i32, except_session_id: Option<&str>) {
        let mut sessions = self.sessions.write();
        for (id, session) in sessions.iter_mut() {
            if let Some(except) = except_session_id {
                if id == except { continue; }
            }
            if session.vram_seq_id == Some(seq_id) {
                session.vram_seq_id = None;
                
                
                
                
                
                
                
                if session.tier == CacheTier::Vram {
                     
                     
                     
                }
            }
        }
    }

    
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
    
    
    pub fn save_to_disk(&self, ctx: &Context, seq_id: i32, session_id: &str, path: &str) -> Result<()> {
        let mut sessions = self.sessions.write();
        let session = sessions.get_mut(session_id).ok_or(Error::BackendError("Session not found".to_string()))?;
        
        let state_data = ctx.state_seq_get_data(seq_id)?;
        session.kv_head = session.kv_head.min(session.tokens.len());
        session.tokens.truncate(session.kv_head);
        Self::write_state_file(&PathBuf::from(path), &state_data, session.kv_head as u32, session.n_keep as u32)?;
        
        session.disk_path = Some(PathBuf::from(path));
        
        Ok(())
    }

    
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
    
    
    fn write_state_file(path: &PathBuf, data: &[u8], n_tokens: u32, n_keep: u32) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::BackendError(e.to_string()))?;
        }
        let header = StateHeader {
            magic: *b"HUGN", 
            version: 1,
            n_tokens,
            n_keep,
            reserved: [0; 16],
        };

        let mut file = std::fs::File::create(path).map_err(|e| Error::BackendError(e.to_string()))?;
        
        
        let header_slice = unsafe {
            std::slice::from_raw_parts(
                &header as *const _ as *const u8,
                std::mem::size_of::<StateHeader>()
            )
        };
        file.write_all(header_slice).map_err(|e| Error::BackendError(e.to_string()))?;
        
        
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
