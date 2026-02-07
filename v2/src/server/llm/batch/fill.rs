
/// Helper to set batch fields.
/// 
/// # Safety
/// * `batch` pointers must be valid and allocated for at least `i + 1` tokens.
/// * `batch.seq_id[i]` must be allocated for at least `seq_ids.len()` sequence IDs.
pub unsafe fn batch_set(
    batch: &mut llama_cpp::llama_batch,
    i: usize,
    tok: llama_cpp::llama_token,
    pos: llama_cpp::llama_pos,
    seq_ids: &[llama_cpp::llama_seq_id],
    logits: bool,
) {
    if i >= batch.n_tokens as usize {
        // Technically we are writing *into* the batch before setting n_tokens sometimes, 
        // but typically we init the batch with max capacity.
        // For raw pointer safety, we trust the caller has allocated enough space.
    }

    // batch.token[i] = tok
    *batch.token.add(i) = tok;

    // batch.pos[i] = pos
    *batch.pos.add(i) = pos;

    // batch.n_seq_id[i] = len
    *batch.n_seq_id.add(i) = seq_ids.len() as i32;

    // batch.seq_id[i][j] = seq_ids[j]
    // batch.seq_id is *mut *mut llama_seq_id
    let seq_ptr_ptr = batch.seq_id.add(i);
    let seq_ptr = *seq_ptr_ptr; // The array for this token
    
    for (j, &seq) in seq_ids.iter().enumerate() {
        *seq_ptr.add(j) = seq;
    }

    // batch.logits[i] = 1 or 0
    *batch.logits.add(i) = if logits { 1 } else { 0 };
}
