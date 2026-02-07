use std::path::PathBuf;
use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::token::LlamaToken;
use llama_cpp_sys_2;


pub struct SeqState {
    pub seq_id: i32,
    pub data: Vec<u8>,
}



unsafe fn raw_ctx_ptr(ctx: &LlamaContext) -> *mut llama_cpp_sys_2::llama_context {
    let ptr = ctx as *const LlamaContext as *const std::ptr::NonNull<llama_cpp_sys_2::llama_context>;
    unsafe { (*ptr).as_ptr() }
}


fn make_session_path(session_name: &str) -> PathBuf {
    PathBuf::from(format!("sessions/{}.bin", session_name))
}

pub fn delete_session(session_name: &str) -> Result<(), std::io::Error> {
    let path = make_session_path(session_name);
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

pub fn save_session(ctx: &LlamaContext, session_name: &str, tokens: &[LlamaToken]) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let path = make_session_path(session_name);
    ctx.save_session_file(&path, tokens)?;
    Ok(path)
}



pub fn load_session(
    ctx: &mut LlamaContext, 
    session_name: &str
) -> Result<(Vec<LlamaToken>, i32), Box<dyn std::error::Error>> {
    let path = make_session_path(session_name);
    if !path.exists() {
        return Err("Session file does not exist".into());
    }
    
    
    let max_tokens = ctx.n_ctx() as usize;
    let tokens = ctx.load_session_file(&path, max_tokens)?;
    
    let n_past = tokens.len() as i32;
    Ok((tokens, n_past))
}

pub fn fork_session(source_name: &str, dest_name: &str) -> Result<PathBuf, std::io::Error> {
    let source = make_session_path(source_name);
    let dest = make_session_path(dest_name);

    if !source.exists() {
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "Source session not found"));
    }

    std::fs::copy(&source, &dest)?;
    Ok(dest)
}


pub fn save_seq_state_to_mem(
    ctx: &LlamaContext,
    seq_id: i32,
) -> Result<SeqState, Box<dyn std::error::Error>> {
    let size = unsafe { llama_cpp_sys_2::llama_state_seq_get_size(raw_ctx_ptr(ctx), seq_id) };
    if size == 0 {
        return Err("Sequence state size is zero".into());
    }
    let mut data = vec![0u8; size];
    let copied = unsafe {
        llama_cpp_sys_2::llama_state_seq_get_data(
            raw_ctx_ptr(ctx),
            data.as_mut_ptr(),
            data.len(),
            seq_id,
        )
    };
    if copied != size {
        return Err(format!(
            "Sequence state copy size mismatch (expected {}, got {})",
            size, copied
        )
        .into());
    }
    Ok(SeqState { seq_id, data })
}



pub fn load_seq_state_from_mem(
    ctx: &mut LlamaContext,
    seq_id: i32,
    data: &[u8],
) -> Result<usize, Box<dyn std::error::Error>> {
    if data.is_empty() {
        return Err("Sequence state data is empty".into());
    }
    let read = unsafe {
        llama_cpp_sys_2::llama_state_seq_set_data(
            raw_ctx_ptr(ctx),
            data.as_ptr(),
            data.len(),
            seq_id,
        )
    };
    if read != data.len() {
        return Err(format!(
            "Sequence state restore size mismatch (expected {}, got {})",
            data.len(),
            read
        )
        .into());
    }
    Ok(read)
}


pub fn clear_seq_state(ctx: &mut LlamaContext, seq_id: i32) -> Result<(), Box<dyn std::error::Error>> {
    let _ = ctx.clear_kv_cache_seq(Some(seq_id as u32), None, None);
    Ok(())
}
