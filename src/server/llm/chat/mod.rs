use crate::llm::error::{Error, Result};
use crate::llm::model::Model;
use std::ffi::CString;

#[repr(C)]
struct llama_chat_message {
    role: *const std::os::raw::c_char,
    content: *const std::os::raw::c_char,
}

pub struct Message {
    pub role: String,
    pub content: String,
}

impl Message {
    pub fn new(role: &str, content: &str) -> Self {
        Self {
            role: role.to_string(),
            content: content.to_string(),
        }
    }
}

pub fn template(model: &Model, prompt: &str) -> Result<String> {
    
    let messages = vec![
        Message::new("user", prompt),
    ];
    apply_template(model, &messages)
}

pub fn apply_template(model: &Model, messages: &[Message]) -> Result<String> {
    let tmpl_str = model.chat_template()?;
    let c_tmpl = if tmpl_str.is_empty() {
        None
    } else {
        Some(CString::new(tmpl_str)?)
        
        
    };
    
    
    let c_roles: Vec<CString> = messages.iter()
        .map(|m| CString::new(m.role.clone()).unwrap())
        .collect();
    let c_contents: Vec<CString> = messages.iter()
        .map(|m| CString::new(m.content.clone()).unwrap())
        .collect();
        
    let c_messages: Vec<llama_chat_message> = c_roles.iter().zip(c_contents.iter())
        .map(|(role, content)| llama_chat_message {
            role: role.as_ptr(),
            content: content.as_ptr(),
        })
        .collect();
        
    let tmpl_ptr = match &c_tmpl {
        Some(c) => c.as_ptr(),
        None => std::ptr::null(),
    };
    
    
    
    
    
    
    
    
    
    
    
    
    let mut buf = vec![0u8; 4096];
    
    let res = unsafe {
        llama_cpp::llama_chat_apply_template(
            tmpl_ptr,
            c_messages.as_ptr() as *const _,
            c_messages.len(),
            true, 
            buf.as_mut_ptr() as *mut i8,
            buf.len() as i32,
        )
    };
    
    if res < 0 {
        return Err(Error::BackendError(format!("Failed to apply chat template: {}", res)));
    }
    
    let len = res as usize;
    if len > buf.len() {
        
        buf.resize(len + 1, 0);
        let res2 = unsafe {
            llama_cpp::llama_chat_apply_template(
                tmpl_ptr,
                c_messages.as_ptr() as *const _,
                c_messages.len(),
                true,
                buf.as_mut_ptr() as *mut i8,
                buf.len() as i32,
            )
        };
        if res2 < 0 {
             return Err(Error::BackendError(format!("Failed to apply chat template (retry): {}", res2)));
        }
    }
    
    
    
    
    let c_str = unsafe { std::ffi::CStr::from_ptr(buf.as_ptr() as *const i8) };
    Ok(c_str.to_string_lossy().into_owned())
}
