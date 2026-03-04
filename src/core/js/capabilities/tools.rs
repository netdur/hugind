use anyhow::Result as AnyhowResult;
use rquickjs::{AsyncContext, Function, Object, Result, Value, function::Async};
use serde_json::Value as JsonValue;
use std::sync::Arc;

use crate::core::config::agent::AgentConfig;
use crate::core::mcp::McpManager;
use crate::shared::logging::RunLogger;

pub async fn install(
    ctx: &AsyncContext,
    config: &AgentConfig,
    logger: Option<RunLogger>,
) -> Result<()> {
    let manager = match McpManager::new(config).await {
        Ok(m) => m.map(Arc::new),
        Err(e) => {
            return Err(rquickjs::Error::new_loading_message(
                "MCP Error",
                e.to_string(),
            ));
        }
    };

    ctx.async_with(|ctx| {
        Box::pin(async move {
            let tools_obj = Object::new(ctx.clone())?;

            let list_manager = manager.clone();
            let list_logger = logger.clone();
            let list_fn = Function::new(
                ctx.clone(),
                Async(move || {
                    let list_manager = list_manager.clone();
                    let list_logger = list_logger.clone();
                    async move {
                        if let Some(l) = &list_logger {
                            l.log_line("host.tools.list");
                        }
                        let tools = match &list_manager {
                            Some(m) => m.list_tools().await.map_err(map_mcp_err)?,
                            None => Vec::new(),
                        };
                        let json = serde_json::to_string(&tools).map_err(map_mcp_err)?;
                        Ok::<String, rquickjs::Error>(json)
                    }
                }),
            )?;
            tools_obj.set("list", list_fn)?;

            let call_manager = manager.clone();
            let call_logger = logger.clone();
            let call_fn = Function::new(
                ctx.clone(),
                Async(move |name: String, args: Value| {
                    let call_manager = call_manager.clone();
                    let call_logger = call_logger.clone();
                    let args_json = js_value_to_json(args).map_err(|e| {
                        rquickjs::Error::new_loading_message("MCP Error", e.to_string())
                    });
                    async move {
                        if let Some(l) = &call_logger {
                            let args_len = args_json
                                .as_ref()
                                .ok()
                                .and_then(|v| serde_json::to_string(v).ok())
                                .map(|s| s.len())
                                .unwrap_or(0);
                            l.log_line(format!(
                                "host.tools.call name={} args_len={}",
                                name, args_len
                            ));
                        }
                        let manager = call_manager.as_ref().ok_or_else(|| {
                            rquickjs::Error::new_loading_message(
                                "MCP Error",
                                "No MCP tools configured",
                            )
                        })?;
                        let mut args_json = args_json?;
                        if args_json.is_null() {
                            args_json = JsonValue::Object(serde_json::Map::new());
                        }
                        let result = manager
                            .call_tool(&name, args_json)
                            .await
                            .map_err(map_mcp_err)?;
                        let json = serde_json::to_string(&result).map_err(map_mcp_err)?;
                        Ok::<String, rquickjs::Error>(json)
                    }
                }),
            )?;
            tools_obj.set("call", call_fn)?;

            ctx.globals().set("tools", tools_obj)?;
            Ok(())
        })
    })
    .await
}

fn map_mcp_err<E: std::fmt::Display>(err: E) -> rquickjs::Error {
    rquickjs::Error::new_loading_message("MCP Error", err.to_string())
}

fn js_value_to_json(value: Value<'_>) -> AnyhowResult<JsonValue> {
    if value.is_null() || value.is_undefined() {
        Ok(JsonValue::Null)
    } else if value.is_bool() {
        Ok(JsonValue::Bool(value.as_bool().unwrap()))
    } else if value.is_number() {
        let n = value.as_number().unwrap();
        if n.is_finite() && (n.fract() == 0.0) {
            if n >= (i64::MIN as f64) && n <= (i64::MAX as f64) {
                return Ok(JsonValue::Number(serde_json::Number::from(n as i64)));
            }
        }
        Ok(JsonValue::Number(
            serde_json::Number::from_f64(n).unwrap_or_else(|| serde_json::Number::from(0)),
        ))
    } else if value.is_string() {
        let s: rquickjs::String = value.into_string().unwrap();
        Ok(JsonValue::String(s.to_string()?))
    } else if value.is_array() {
        let arr = value.into_array().unwrap();
        let mut out = Vec::new();
        for i in 0..arr.len() {
            out.push(js_value_to_json(arr.get(i)?)?);
        }
        Ok(JsonValue::Array(out))
    } else if value.is_object() {
        let obj = value.into_object().unwrap();
        let mut out = serde_json::Map::new();
        for key in obj.keys::<rquickjs::String>() {
            let key = key?;
            let k_str = key.to_string()?;
            let v = obj.get(&k_str)?;
            out.insert(k_str, js_value_to_json(v)?);
        }
        Ok(JsonValue::Object(out))
    } else {
        Ok(JsonValue::Null)
    }
}
