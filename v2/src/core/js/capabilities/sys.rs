use rquickjs::{AsyncContext, Function};

pub async fn install(ctx: &AsyncContext) -> rquickjs::Result<()> {
    ctx.with(|ctx| {
        let print = Function::new(ctx.clone(), |msg: String| {
            println!("{msg}");
        })?;

        ctx.globals().set("print", print)?;
        Ok(())
    }).await
}
