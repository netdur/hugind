use std::collections::VecDeque;
pub struct RequestQueue {
    pub pending: VecDeque<String>,
    pub max_capacity: Option<usize>,
}

impl RequestQueue {
    pub fn new(capacity: Option<usize>) -> Self {
        Self {
            pending: VecDeque::new(),
            max_capacity: capacity,
        }
    }

    pub fn push(&mut self, request_id: String) -> Result<(), String> {
        if let Some(max) = self.max_capacity {
            if self.pending.len() >= max {
                return Err("Queue full".to_string());
            }
        }
        self.pending.push_back(request_id);
        Ok(())
    }

    pub fn pop(&mut self) -> Option<String> {
        // Simple FIFO for now
        self.pending.pop_front()
    }

    pub fn push_front(&mut self, request_id: String) {
        self.pending.push_front(request_id);
    }

    pub fn remove(&mut self, request_id: &str) -> bool {
        if let Some(pos) = self.pending.iter().position(|id| id == request_id) {
            self.pending.remove(pos);
            return true;
        }
        false
    }
    
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}
