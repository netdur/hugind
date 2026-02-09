use anyhow::{Result};
use inquire::{Select, Text, Confirm};
use std::io::{self, Write};
use std::fs;
use std::time::Duration;
use futures_util::StreamExt;
use base64::{Engine as _, engine::general_purpose};
use serde_json::{json, Value};

use indicatif::{ProgressBar, ProgressStyle};

use crate::core::chat::session::{SessionRepo, Message};
use crate::core::chat::service::ChatService;
use crate::shared::{paths, configs};

pub async fn run_interactive_wizard() -> Result<()> {
    let options = vec![
        "Start New Chat",
        "Resume Chat",
        "List Sessions",
        "Delete Session",
        "Exit",
    ];
    let selection = Select::new("🦅 Hugind AI Workspace", options).prompt()?;

    match selection {
        "Start New Chat" => run_start(String::new()).await,
        "Resume Chat" => run_resume(String::new()).await,
        "List Sessions" => run_list(),
        "Delete Session" => run_delete(None).await,
        _ => Ok(()),
    }
}

pub async fn run_start(config: String) -> Result<()> {
    let mut config_name = config;
    if config_name.is_empty() {
        
        let configs = list_configs();
        if configs.is_empty() {
             config_name = Text::new("Enter Config Name manually:").with_default("my-assistant").prompt()?;
        } else {
             let mut options = configs;
             options.push("Custom...".to_string());
             let selection = Select::new("Select Configuration:", options).prompt()?;
             if selection == "Custom..." {
                 config_name = Text::new("Enter Config Name:").with_default("my-assistant").prompt()?;
             } else {
                 config_name = selection;
             }
        }
    }

    let id = SessionRepo::create(&config_name)?;
    start_chat_loop(id).await
}

pub async fn run_resume(id: String) -> Result<()> {
    let mut session_id = id;
    if session_id.is_empty() {
        let sessions = SessionRepo::list()?;
        if sessions.is_empty() {
            println!("No active sessions found.");
            if Confirm::new("Start a new chat instead?").with_default(true).prompt()? {
                return run_start(String::new()).await;
            }
            return Ok(());
        }
        
        let options: Vec<String> = sessions.iter().map(|s| {
             format!("{} ({}) - {}", s.title, s.model, format_time(s.last_active))
        }).collect();

        let selection = Select::new("Select a session to resume:", options).prompt()?;
        
        let idx = sessions.iter().position(|s| format!("{} ({}) - {}", s.title, s.model, format_time(s.last_active)) == selection).unwrap();
        session_id = sessions[idx].id.clone();
    }
    start_chat_loop(session_id).await
}

pub fn run_list() -> Result<()> {
    let sessions = SessionRepo::list()?;
    println!("{:<20} {:<15} {}", "ID", "LAST ACTIVE", "TITLE");
    for s in sessions {
         println!("{:<20} {:<15} {}", s.id, format_time(s.last_active), s.title);
    }
    Ok(())
}

pub async fn run_delete(id: Option<String>) -> Result<()> {
    let mut session_id = id;
    if session_id.is_none() {
        let sessions = SessionRepo::list()?;
        if sessions.is_empty() { println!("No sessions."); return Ok(()); }
        
        let options: Vec<String> = sessions.iter().map(|s| {
             format!("{} ({}) - {}", s.title, s.model, format_time(s.last_active))
        }).collect();
        
        let selection = Select::new("Select session to DELETE:", options).prompt()?;
        let idx = sessions.iter().position(|s| format!("{} ({}) - {}", s.title, s.model, format_time(s.last_active)) == selection).unwrap();
        session_id = Some(sessions[idx].id.clone());
    }
    
    if let Some(sid) = session_id {
        if Confirm::new(&format!("Are you sure you want to delete {}?", sid)).with_default(false).prompt()? {
            SessionRepo::delete(&sid)?;
            println!("✅ Session deleted.");
        }
    }
    Ok(())
}

fn list_configs() -> Vec<String> {
    let dir = paths::config_home().join("configs");
    let mut configs = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if ext == "yml" || ext == "yaml" {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        if configs::is_reserved_config_name(stem) {
                            continue;
                        }
                        configs.push(stem.to_string());
                    }
                }
            }
        }
    }
    configs
}

fn format_time(dt: chrono::DateTime<chrono::Utc>) -> String {
    let diff = chrono::Utc::now() - dt;
    if diff.num_minutes() < 60 {
        format!("{}m ago", diff.num_minutes())
    } else if diff.num_hours() < 24 {
        format!("{}h ago", diff.num_hours())
    } else {
        format!("{}d ago", diff.num_days())
    }
}



pub async fn start_chat_loop(session_id: String) -> Result<()> {
    let mut session = SessionRepo::load(&session_id)?;
    let service = ChatService::new();
    let base_url = service.resolve_base_url(&session.model);

    println!("\x1B[1;34m\n🦅  HUGIND WORKSPACE\x1B[0m");
    println!("   Session: {}", session.id);
    println!("   Model:   {}", session.model);
    println!("   Type /help for commands.\n");

    
    if !session.messages.is_empty() {
        println!("\x1B[90m--- Recent Context ---\x1B[0m");
        for msg in session.messages.iter().rev().take(6).rev() {
             let role_color = if msg.role == "user" { "\x1B[32m" } else { "\x1B[36m" };
             let preview = match &msg.content {
                 Value::String(s) => s.lines().next().unwrap_or(""),
                 _ => "(multimodal)"
             };
             println!("{}{}: \x1B[0m{}...", role_color, msg.role, preview);
        }
        println!("\x1B[90m----------------------\x1B[0m");
    }

    let mut pending_image: Option<String> = None;
    let mut pending_text: Option<String> = None;

    loop {
        let prompt = if pending_image.is_some() {
            "\n\x1B[1m🖼️  (Image) \x1B[32m>>> \x1B[0m"
        } else if pending_text.is_some() {
            "\n\x1B[1m📄  (Text) \x1B[32m>>> \x1B[0m"
        } else {
            "\n\x1B[32m>>> \x1B[0m"
        };
        
        print!("{}", prompt);
        io::stdout().flush()?;

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() { break; } 
        let input = input.trim();

        if input.is_empty() && pending_image.is_none() { continue; }

        if input.starts_with('/') {
            let parts: Vec<&str> = input.splitn(2, ' ').collect();
            let cmd = parts[0];
            let arg = if parts.len() > 1 { parts[1] } else { "" };

            match cmd {
                "/exit" | "/quit" => break,
                "/help" => {
                    println!("\x1B[1mAvailable Commands:\x1B[0m");
                    println!("  /image <path>   Attach an image");
                    println!("  /text <path>    Attach a text file");
                    println!("  /fork <name>    Save session fork");
                    println!("  /clear          Clear screen");
                    println!("  /exit, /quit    Exit");
                    continue;
                },
                "/clear" => {
                     print!("\x1B[2J\x1B[0;0H");
                     println!("\x1B[1;34m\n🦅  HUGIND WORKSPACE\x1B[0m");
                     continue;
                },
                "/image" => {
                    if arg.is_empty() { println!("Usage: /image <path>"); continue; }
                    match fs::read(arg) {
                        Ok(bytes) => {
                             let b64 = general_purpose::STANDARD.encode(&bytes);
                             
                             let mime = if arg.to_lowercase().ends_with(".png") { "image/png" } else { "image/jpeg" };
                             pending_image = Some(format!("data:{};base64,{}", mime, b64));
                             println!("✅ Image attached!");
                        }
                        Err(e) => println!("❌ Error reading file: {}", e),
                    }
                    continue;
                },
                 "/text" => {
                    if arg.is_empty() { println!("Usage: /text <path>"); continue; }
                    match fs::read_to_string(arg) {
                        Ok(content) => {
                             pending_text = Some(content);
                             println!("✅ Text attached!");
                        }
                        Err(e) => println!("❌ Error reading file: {}", e),
                    }
                    continue;
                },
                "/fork" => {
                     println!("(Fork not implemented in this demo yet)"); 
                     continue;
                }
                _ => { println!("Unknown command."); continue; }
            }
        }

        
        let new_msg = if let Some(img_data) = pending_image.take() {
            Message {
                role: "user".to_string(),
                content: json!([
                    { "type": "text", "text": if input.is_empty() { "Describe this image" } else { input } },
                    { "type": "image_url", "image_url": { "url": img_data } }
                ])
            }
        } else if let Some(txt_data) = pending_text.take() {
             let merged = if input.is_empty() { txt_data } else { format!("{}\n\n{}", input, txt_data) };
             Message { role: "user".to_string(), content: Value::String(merged) }
        } else {
             Message { role: "user".to_string(), content: Value::String(input.to_string()) }
        };

        
        let pb = ProgressBar::new_spinner();
        pb.set_style(ProgressStyle::default_spinner().template("{spinner:.cyan} Thinking...")?);
        pb.enable_steady_tick(Duration::from_millis(80));

        let is_new = session.messages.is_empty();
        let response_result: Result<reqwest::Response> = service.send_message(
            &session_id, 
            &session.model, 
            &session.messages, 
            &new_msg, 
            is_new, 
            &base_url
        ).await;

        pb.finish_and_clear();

        match response_result {
            Ok(resp) => {
                if !resp.status().is_success() {
                    println!("Error: {}", resp.status());
                    continue;
                }

                print!("\x1B[36m"); 
                let mut buffer = String::new();
                let mut stream = resp.bytes_stream();
                
                while let Some(item) = stream.next().await {
                    let item: Result<bytes::Bytes, reqwest::Error> = item;
                    match item {
                        Ok(chunk) => {
                             let s = String::from_utf8_lossy(&chunk);
                             for line in s.lines() {
                                 if line.starts_with("data: ") {
                                     let data = &line[6..];
                                     if data == "[DONE]" { break; }
                                     if let Ok(json) = serde_json::from_str::<Value>(data) {
                                         if let Some(delta) = json.get("choices").and_then(|c| c.get(0)).and_then(|c| c.get("delta")).and_then(|d| d.get("content")).and_then(|c| c.as_str()) {
                                             print!("{}", delta);
                                             io::stdout().flush()?;
                                             buffer.push_str(delta);
                                         }
                                     }
                                 }
                             }
                        }
                        Err(e) => {
                             println!("\nStream Error: {}", e);
                             break;
                        }
                    }
                }
                println!("\x1B[0m\n");

                
                session.messages.push(new_msg);
                session.messages.push(Message { role: "assistant".to_string(), content: Value::String(buffer) });

                
                if session.messages.len() == 2 {
                    print!("\x1B[90mGenerating title...\x1B[0m");
                    io::stdout().flush()?;
                    let title: String = service.generate_title(&session.model, &session.messages, &base_url).await;
                    if !title.is_empty() {
                         session.title = Some(title.clone());
                         print!("\r\x1B[90mTitle updated: {}\x1B[0m\n", title);
                    } else {
                        print!("\r                        \r");
                    }
                }
                SessionRepo::save(&session_id, session.clone())?;
            }
            Err(e) => {
                println!("\nConnection Failed: {}", e);
            }
        }
    }

    println!("\n❄️  Hibernating...");
    service.hibernate(&session_id, &base_url).await;
    println!("👋 Exiting...");
    
    Ok(())
}
