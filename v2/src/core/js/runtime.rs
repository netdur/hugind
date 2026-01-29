use crate::core::js::{globals::install_globals, loader::LocalOnlyResolver};
use rquickjs::{AsyncContext, AsyncRuntime, Error, Function, Module};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub struct JsRuntime {
    _runtime: AsyncRuntime,
    context: AsyncContext,
}

impl JsRuntime {
    pub async fn new(agent_root: PathBuf, config: &crate::core::config::agent::AgentConfig) -> rquickjs::Result<Self> {
        let runtime = AsyncRuntime::new()?;
        let context = AsyncContext::full(&runtime).await?;

        runtime.set_loader(
            LocalOnlyResolver {
                root: agent_root.clone(),
            },
            rquickjs::loader::ScriptLoader::default(),
        ).await;

        install_globals(&context, config).await?;

        Ok(Self { _runtime: runtime, context })
    }

    pub async fn run_module(&self, entry: &Path) -> rquickjs::Result<()> {
        let entry = entry
            .canonicalize()
            .map_err(|e| Error::new_loading_message(entry.display().to_string(), e.to_string()))?;

        let source = fs::read_to_string(&entry).map_err(|e| {
            Error::new_loading_message(entry.display().to_string(), e.to_string())
        })?;

        let name: String = entry.to_string_lossy().into_owned();

        self.context.with(|ctx| {
            let res = (|| -> rquickjs::Result<()> {
                let module = Module::declare(ctx.clone(), name, source)?;
                // Verify if eval returns promise, we don't await it here.
                let (module, _promise) = module.eval()?;
                
                if let Ok(default_export) = module.get::<_, Function>("default") {
                    let args = rquickjs::Object::new(ctx.clone())?;
                    if let Ok(llm) = ctx.globals().get::<_, rquickjs::Value>("llm") {
                            args.set("llm", llm)?;
                    }

                    // Call default export (main)
                    // If it returns a promise, we just ignore it here (runtime drives it)
                    let _result = default_export.call::<_, rquickjs::Value>((args,))?;
                }
                Ok(())
            })();
            
            if let Err(rquickjs::Error::Exception) = res {
                let catch = ctx.catch();
                if let Some(exception) = catch.as_exception() {
                     eprintln!("JS Exception: {:?}", exception);
                     if let Some(stack) = exception.stack() {
                         eprintln!("Stack: {}", stack);
                     }
                }
                // Don't return error on exception immediately? Or yes?
                // Returning error is fine.
            }
            res
        }).await
    }
    
    pub async fn wait_idle(&self) {
        self._runtime.idle().await
    }
}
