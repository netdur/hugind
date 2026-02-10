use rquickjs::{function::Async, AsyncContext, Function, Result};

use crate::shared::logging::RunLogger;

fn print(msg: String) {
    println!("{msg}");
}

fn print_raw(msg: String) {
    use std::io::{self, Write};
    let mut out = io::stdout();
    let _ = out.write_all(msg.as_bytes());
    let _ = out.flush();
}

fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

async fn input(prompt: String) -> String {
    use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
    
    let mut stdout = io::stdout();
    let _ = stdout.write_all(prompt.as_bytes()).await;
    let _ = stdout.flush().await;
    
    let mut reader = BufReader::new(io::stdin());
    let mut buffer = String::new();
    let _ = reader.read_line(&mut buffer).await;
    buffer.trim().to_string()
}

pub async fn install(ctx: &AsyncContext, logger: Option<RunLogger>) -> Result<()> {
    ctx.async_with(|ctx| Box::pin(async move {
        let print_func = Function::new(ctx.clone(), move |msg: String| {
            print(msg);
        })?;
        ctx.globals().set("print", print_func)?;

        let print_raw_func = Function::new(ctx.clone(), move |msg: String| {
            print_raw(msg);
        })?;
        ctx.globals().set("print_raw", print_raw_func)?;

        let logger_input = logger.clone();
        let input_func = Function::new(ctx.clone(), Async(move |prompt: String| {
            let logger_input = logger_input.clone();
            async move {
                if let Some(l) = &logger_input {
                    l.log_line(format!("host.sys.input prompt_len={}", prompt.len()));
                }
                Ok::<String, rquickjs::Error>(input(prompt).await)
            }
        }))?;
        ctx.globals().set("input", input_func)?;

        let version_func = Function::new(ctx.clone(), move || {
            version()
        })?;
        ctx.globals().set("hugind_version", version_func)?;

        Ok(())
    })).await
}
