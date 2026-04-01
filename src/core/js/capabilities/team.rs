use rquickjs::{AsyncContext, Function, Object, Result};

use crate::core::orchestrator::context::TeamContext;
use crate::core::orchestrator::task::Task;
use crate::shared::logging::RunLogger;

pub async fn install(
    ctx: &AsyncContext,
    team_ctx: &TeamContext,
    logger: Option<RunLogger>,
) -> Result<()> {
    let memory = team_ctx.memory.clone();
    let messages = team_ctx.messages.clone();
    let task_queue = team_ctx.task_queue.clone();
    let agent_name = team_ctx.agent_name.clone();

    ctx.async_with(|ctx| {
        let memory = memory.clone();
        let messages = messages.clone();
        let task_queue = task_queue.clone();
        let agent_name = agent_name.clone();
        let logger = logger.clone();

        Box::pin(async move {
            // --- memory global ---
            let mem_obj = Object::new(ctx.clone())?;
            {
                let memory = memory.clone();
                let agent = agent_name.clone();
                let logger = logger.clone();
                let set_fn = Function::new(ctx.clone(), move |key: String, value: String| {
                    if let Some(l) = &logger {
                        l.log_line(format!("host.memory.set agent={} key={}", agent, key));
                    }
                    let json_val = serde_json::from_str(&value)
                        .unwrap_or(serde_json::Value::String(value.clone()));
                    memory.set(&agent, &key, json_val);
                })?;
                mem_obj.set("set", set_fn)?;
            }
            {
                let memory = memory.clone();
                let logger = logger.clone();
                let get_fn = Function::new(ctx.clone(), move |key: String| -> String {
                    if let Some(l) = &logger {
                        l.log_line(format!("host.memory.get key={}", key));
                    }
                    match memory.get(&key) {
                        Some(v) => serde_json::to_string(&v).unwrap_or_default(),
                        None => "null".to_string(),
                    }
                })?;
                mem_obj.set("get", get_fn)?;
            }
            {
                let memory = memory.clone();
                let list_fn = Function::new(ctx.clone(), move || -> String {
                    let all = memory.list_all();
                    let map: serde_json::Map<String, serde_json::Value> = all
                        .into_iter()
                        .collect();
                    serde_json::to_string(&map).unwrap_or("{}".to_string())
                })?;
                mem_obj.set("list", list_fn)?;
            }
            {
                let memory = memory.clone();
                let summary_fn = Function::new(ctx.clone(), move || -> String {
                    memory.summary()
                })?;
                mem_obj.set("summary", summary_fn)?;
            }
            ctx.globals().set("memory", mem_obj)?;

            // --- messaging global ---
            let msg_obj = Object::new(ctx.clone())?;
            {
                let messages = messages.clone();
                let agent = agent_name.clone();
                let logger = logger.clone();
                let send_fn = Function::new(ctx.clone(), move |to: String, content: String| {
                    if let Some(l) = &logger {
                        l.log_line(format!("host.messaging.send from={} to={}", agent, to));
                    }
                    messages.send(&agent, &to, &content);
                })?;
                msg_obj.set("send", send_fn)?;
            }
            {
                let messages = messages.clone();
                let agent = agent_name.clone();
                let logger = logger.clone();
                let broadcast_fn = Function::new(ctx.clone(), move |content: String| {
                    if let Some(l) = &logger {
                        l.log_line(format!("host.messaging.broadcast from={}", agent));
                    }
                    messages.broadcast(&agent, &content);
                })?;
                msg_obj.set("broadcast", broadcast_fn)?;
            }
            {
                let messages = messages.clone();
                let agent = agent_name.clone();
                let receive_fn = Function::new(ctx.clone(), move || -> String {
                    let msgs = messages.receive(&agent);
                    let arr: Vec<serde_json::Value> = msgs
                        .iter()
                        .map(|m| {
                            serde_json::json!({
                                "from": m.from,
                                "to": m.to,
                                "content": m.content,
                            })
                        })
                        .collect();
                    serde_json::to_string(&arr).unwrap_or("[]".to_string())
                })?;
                msg_obj.set("receive", receive_fn)?;
            }
            ctx.globals().set("messaging", msg_obj)?;

            // --- tasks global (only if task queue is available) ---
            if let Some(queue) = task_queue {
                let tasks_obj = Object::new(ctx.clone())?;
                {
                    let queue = queue.clone();
                    let logger = logger.clone();
                    let spawn_fn = Function::new(ctx.clone(), move |json_str: String| -> String {
                        if let Some(l) = &logger {
                            l.log_line(format!("host.tasks.spawn input_len={}", json_str.len()));
                        }

                        let parsed: std::result::Result<SpawnTaskInput, _> =
                            serde_json::from_str(&json_str);
                        match parsed {
                            Ok(input) => {
                                let id = format!("dyn-{}", uuid::Uuid::new_v4());
                                let mut task = Task::new(&id, &input.title, &input.description);
                                task.assignee = input.assignee;
                                task.depends_on = input.depends_on;

                                let mut q = queue.lock();
                                match q.add(task) {
                                    Ok(()) => serde_json::json!({"ok": true, "id": id}).to_string(),
                                    Err(e) => serde_json::json!({"ok": false, "error": e.to_string()}).to_string(),
                                }
                            }
                            Err(e) => {
                                serde_json::json!({"ok": false, "error": format!("Invalid task JSON: {}", e)}).to_string()
                            }
                        }
                    })?;
                    tasks_obj.set("spawn", spawn_fn)?;
                }
                ctx.globals().set("tasks", tasks_obj)?;
            }

            Ok(())
        })
    })
    .await
}

#[derive(serde::Deserialize)]
struct SpawnTaskInput {
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    assignee: Option<String>,
    #[serde(default)]
    depends_on: Vec<String>,
}
