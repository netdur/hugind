//! Helpers for FFI safety and lifetime management.

use crate::llm::error::{Error, Result};
use std::ptr::NonNull;

/// helper to ensure a pointer is not null
pub fn ensure_non_null<T>(ptr: *mut T, err_msg: &str) -> Result<NonNull<T>> {
    NonNull::new(ptr).ok_or_else(|| Error::BackendError(err_msg.to_string()))
}

/// helper to ensure a const pointer is not null
pub fn ensure_non_null_const<T>(ptr: *const T, err_msg: &str) -> Result<NonNull<T>> {
    NonNull::new(ptr as *mut T).ok_or_else(|| Error::BackendError(err_msg.to_string()))
}
