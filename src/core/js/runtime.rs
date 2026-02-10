use crate::core::js::{globals::install_globals, loader::LocalOnlyResolver};
use rquickjs::{AsyncContext, AsyncRuntime, Error, Function, Module};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub struct JsRuntime {
    _runtime: AsyncRuntime,
    context: AsyncContext,
    agent_root: PathBuf,
    logger: Option<crate::shared::logging::RunLogger>,
}

impl JsRuntime {
    pub async fn new(
        agent_root: PathBuf,
        config: &crate::core::config::agent::AgentConfig,
        logger: Option<crate::shared::logging::RunLogger>,
    ) -> rquickjs::Result<Self> {
        let runtime = AsyncRuntime::new()?;
        let context = AsyncContext::full(&runtime).await?;

        runtime.set_loader(
            LocalOnlyResolver {
                root: agent_root.clone(),
            },
            rquickjs::loader::ScriptLoader::default(),
        ).await;

        install_globals(&context, config, &agent_root, logger.clone()).await?;

        Ok(Self {
            _runtime: runtime,
            context,
            agent_root,
            logger,
        })
    }

    pub async fn run_module(&self, entry: &Path, args_val: serde_json::Value) -> rquickjs::Result<serde_json::Value> {
        let entry = entry
            .canonicalize()
            .map_err(|e| Error::new_loading_message(entry.display().to_string(), e.to_string()))?;

        if !entry.starts_with(&self.agent_root) {
            return Err(Error::new_loading_message(
                entry.display().to_string(),
                "entry escapes agent root".to_string(),
            ));
        }

        let args_json = serde_json::to_string(&args_val)
            .map_err(|e| Error::new_loading_message("args", e.to_string()))?;

        let source = fs::read_to_string(&entry).map_err(|e| {
            Error::new_loading_message(entry.display().to_string(), e.to_string())
        })?;

        let name: String = entry.to_string_lossy().into_owned();

        let output = std::sync::Arc::new(std::sync::Mutex::new(None));
        let output_clone = output.clone();
        let explicit_output = std::sync::Arc::new(std::sync::Mutex::new(None));
        let explicit_output_clone = explicit_output.clone();
        let args_json_clone = args_json.clone();

        self.context.with(|ctx| {
            let res: rquickjs::Result<()> = (|| {
                let args_json_inner = args_json_clone.clone();
                let explicit_output_inner = explicit_output_clone.clone();

                let get_args_fn = Function::new(ctx.clone(), move || args_json_inner.clone())?;
                ctx.globals().set("get_args_json", get_args_fn)?;

                let get_args_fn_compat = Function::new(ctx.clone(), move || args_json_clone.clone())?;
                ctx.globals().set("get_args", get_args_fn_compat)?;

                let set_result_fn = Function::new(ctx.clone(), move |ctx, val| {
                    let json = js_to_json(&ctx, val).unwrap_or(serde_json::Value::Null);
                    *explicit_output_inner.lock().unwrap() = Some(json);
                    Ok::<(), Error>(())
                })?;
                ctx.globals().set("set_result", set_result_fn)?;

                let module = Module::declare(ctx.clone(), name, source)?;
                let (module, _promise) = module.eval()?;
                
                if let Ok(default_export) = module.get::<_, Function>("default") {
                    let js_args = json_to_js(&ctx, args_val)?;
                    let result_val: rquickjs::Value = default_export.call((js_args,))?;
                    
                    if let Some(promise) = result_val.as_promise() {
                        let output_inner = output_clone.clone();
                        let on_success = Function::new(ctx.clone(), move |ctx, val| {
                            let json = js_to_json(&ctx, val).unwrap_or(serde_json::Value::Null);
                            *output_inner.lock().unwrap() = Some(Ok(json));
                            Ok::<(), Error>(())
                        })?;

                        let on_fail = Function::new(ctx.clone(), move |_ctx: rquickjs::Ctx, err: rquickjs::Value| {
                            *output_clone.lock().unwrap() = Some(Err(rquickjs::Error::new_loading_message("Async Error", format!("{:?}", err))));
                            Ok::<(), Error>(())
                        })?;

                        let then_fn: Function = promise.as_object().unwrap().get("then")?;
                        let _ = then_fn.call::<_, rquickjs::Value>((rquickjs::function::This(promise.as_object().unwrap().clone()), on_success, on_fail))?;
                    } else {
                        let json = js_to_json(&ctx, result_val).unwrap_or(serde_json::Value::Null);
                        *output_clone.lock().unwrap() = Some(Ok(json));
                    }
                } else {
                    *output_clone.lock().unwrap() = Some(Ok(serde_json::Value::Null));
                }
                Ok(())
            })();

            if let Err(e) = res {
                if let rquickjs::Error::Exception = e {
                    let catch = ctx.catch();
                    if let Some(exception) = catch.as_exception() {
                        eprintln!("JS Exception: {:?}", exception);
                        if let Some(stack) = exception.stack() {
                            eprintln!("Stack: {}", stack);
                        }
                        if let Some(logger) = &self.logger {
                            let mut msg = format!("js.exception {:?}", exception);
                            if let Some(stack) = exception.stack() {
                                msg.push_str(&format!(" stack={}", stack));
                            }
                            logger.log_line(msg);
                        }
                    }
                } else {
                    eprintln!("Execution Error: {}", e);
                    if let Some(logger) = &self.logger {
                        logger.log_line(format!("js.execution_error {}", e));
                    }
                }
                *output.lock().unwrap() = Some(Err(e));
            }
        }).await;

        
        loop {
            self.wait_idle().await;
            if let Some(res) = explicit_output.lock().unwrap().take() {
                return Ok(res);
            }
            if let Some(res) = output.lock().unwrap().take() {
                return res;
            }
            
            
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }
    
    pub async fn wait_idle(&self) {
        self._runtime.idle().await
    }
}

fn json_to_js<'js>(ctx: &rquickjs::Ctx<'js>, value: serde_json::Value) -> rquickjs::Result<rquickjs::Value<'js>> {
    match value {
        serde_json::Value::Null => Ok(rquickjs::Value::new_null(ctx.clone())),
        serde_json::Value::Bool(b) => Ok(rquickjs::Value::new_bool(ctx.clone(), b)),
        serde_json::Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                Ok(rquickjs::Value::new_float(ctx.clone(), f))
            } else if let Some(i) = n.as_i64() {
                Ok(rquickjs::Value::new_int(ctx.clone(), i as i32)) 
            } else {
                Ok(rquickjs::Value::new_null(ctx.clone()))
            }
        }
        serde_json::Value::String(s) => rquickjs::String::from_str(ctx.clone(), &s).map(|s| s.into_value()),
        serde_json::Value::Array(arr) => {
            let js_arr = rquickjs::Array::new(ctx.clone())?;
            for (i, val) in arr.into_iter().enumerate() {
                js_arr.set(i, json_to_js(ctx, val)?)?;
            }
            Ok(js_arr.into_value())
        }
        serde_json::Value::Object(obj) => {
            let js_obj = rquickjs::Object::new(ctx.clone())?;
            for (k, v) in obj {
                js_obj.set(k, json_to_js(ctx, v)?)?;
            }
            Ok(js_obj.into_value())
        }
    }
}

fn js_to_json<'js>(ctx: &rquickjs::Ctx<'js>, value: rquickjs::Value<'js>) -> rquickjs::Result<serde_json::Value> {
    if value.is_null() || value.is_undefined() {
        Ok(serde_json::Value::Null)
    } else if value.is_bool() {
        Ok(serde_json::Value::Bool(value.as_bool().unwrap()))
    } else if value.is_number() {
        Ok(serde_json::Value::Number(serde_json::Number::from_f64(value.as_number().unwrap()).unwrap()))
    } else if value.is_string() {
        let s: rquickjs::String = value.into_string().unwrap();
        Ok(serde_json::Value::String(s.to_string()?))
    } else if value.is_array() {
        let arr = value.into_array().unwrap();
        let mut out = Vec::new();
        for i in 0..arr.len() {
            out.push(js_to_json(ctx, arr.get(i)?)?);
        }
        Ok(serde_json::Value::Array(out))
    } else if value.is_object() {
        let obj = value.into_object().unwrap();
        let mut out = serde_json::Map::new();
        
        
        for key in obj.keys::<rquickjs::String>() {
            let key = key?;
            let k_str = key.to_string()?;
            let v = obj.get(&k_str)?;
            out.insert(k_str, js_to_json(ctx, v)?);
        }
        Ok(serde_json::Value::Object(out))
    } else {
        Ok(serde_json::Value::Null)
    }
}
