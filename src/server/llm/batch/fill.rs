pub(super) unsafe fn batch_set(
    batch: &mut llama_cpp::llama_batch,
    i: usize,
    tok: llama_cpp::llama_token,
    pos: llama_cpp::llama_pos,
    seq_ids: &[llama_cpp::llama_seq_id],
    logits: bool,
) {
    // SAFETY: caller guarantees that `i` is within batch capacity and `seq_ids.len()`
    // does not exceed `n_seq_max` for this batch slot.
    unsafe {
        *batch.token.add(i) = tok;
        *batch.pos.add(i) = pos;
        *batch.n_seq_id.add(i) = seq_ids.len() as i32;

        let seq_ptr_ptr = batch.seq_id.add(i);
        let seq_ptr = *seq_ptr_ptr;
        for (j, &seq) in seq_ids.iter().enumerate() {
            *seq_ptr.add(j) = seq;
        }

        *batch.logits.add(i) = if logits { 1 } else { 0 };
    }
}
