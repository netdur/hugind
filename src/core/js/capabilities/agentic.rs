use rquickjs::{AsyncContext, Function, Object, Result};

use crate::core::orchestrator::agentic::{AgentTool, ToolRegistry};
use crate::shared::logging::RunLogger;

/// Install `register_tool()` and `set_system_prompt()` globals.
pub async fn install(
    ctx: &AsyncContext,
    registry: &ToolRegistry,
    logger: Option<RunLogger>,
) -> Result<()> {
    let registry = registry.clone();
    let logger = logger.clone();

    ctx.async_with(|ctx| {
        let registry = registry.clone();
        let logger = logger.clone();

        Box::pin(async move {
            // Create __tool_executors map to store JS execute functions
            let executors = Object::new(ctx.clone())?;
            ctx.globals().set("__tool_executors", executors)?;

            // register_tool({ name, description, parameters, execute })
            // Uses a JS wrapper that extracts fields and stores the execute function
            {
                let registry = registry.clone();
                let logger = logger.clone();

                // We use a simpler approach: register_tool takes (name, description, params_json, execute_fn)
                // and a JS shim converts the object call to this.
                let register_inner = Function::new(
                    ctx.clone(),
                    move |name: String, description: String, params_json: String| {
                        let params: serde_json::Value =
                            serde_json::from_str(&params_json).unwrap_or(
                                serde_json::json!({"type": "object", "properties": {}}),
                            );

                        if let Some(l) = &logger {
                            l.log_line(format!("agentic.register_tool name={}", name));
                        }

                        registry.register(AgentTool {
                            name,
                            description,
                            parameters: params,
                        });
                    },
                )?;
                ctx.globals().set("__register_tool_inner", register_inner)?;

                // JS shim that unpacks the object and stores execute in __tool_executors
                ctx.eval::<(), _>(r#"
                    function register_tool(def) {
                        var name = def.name || "";
                        var description = def.description || "";
                        var params = def.parameters ? JSON.stringify(def.parameters) : '{"type":"object","properties":{}}';
                        __register_tool_inner(name, description, params);
                        if (def.execute) {
                            __tool_executors[name] = def.execute;
                        }
                    }
                    var registerTool = register_tool;
                "#)?;
            }

            // set_system_prompt(prompt)
            {
                let registry1 = registry.clone();
                let set_prompt_fn =
                    Function::new(ctx.clone(), move |prompt: String| {
                        registry1.set_system_prompt(prompt);
                    })?;
                ctx.globals().set("set_system_prompt", set_prompt_fn)?;

                let registry2 = registry.clone();
                let set_prompt_fn2 =
                    Function::new(ctx.clone(), move |prompt: String| {
                        registry2.set_system_prompt(prompt);
                    })?;
                ctx.globals().set("setSystemPrompt", set_prompt_fn2)?;
            }

            // set_max_turns(n)
            {
                let registry3 = registry.clone();
                let set_turns_fn =
                    Function::new(ctx.clone(), move |turns: u32| {
                        registry3.set_max_turns(turns);
                    })?;
                ctx.globals().set("set_max_turns", set_turns_fn)?;

                let registry4 = registry.clone();
                let set_turns_fn2 =
                    Function::new(ctx.clone(), move |turns: u32| {
                        registry4.set_max_turns(turns);
                    })?;
                ctx.globals().set("setMaxTurns", set_turns_fn2)?;
            }

            Ok(())
        })
    })
    .await
}

