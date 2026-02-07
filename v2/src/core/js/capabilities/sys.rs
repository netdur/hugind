use rquickjs::{function::Async, AsyncContext, Function, Result};

fn print(msg: String) {
    println!("{msg}");
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

pub async fn install(ctx: &AsyncContext) -> Result<()> {
    ctx.async_with(|ctx| Box::pin(async move {
        
        let print_func = Function::new(ctx.clone(), print)?;
        ctx.globals().set("print", print_func)?;

        
        
        let input_func = Function::new(ctx.clone(), Async(input))?;
        ctx.globals().set("input", input_func)?;

        Ok(())
    })).await
}
