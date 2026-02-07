pub async fn run(path: String, args_vec: Vec<String>) -> anyhow::Result<()> {
    crate::core::orchestrator::execute(path, args_vec).await
}

pub fn install() -> anyhow::Result<()> {
    println!("Agent install not implemented yet");
    Ok(())
}

pub fn remove() -> anyhow::Result<()> {
    println!("Agent remove not implemented yet");
    Ok(())
}
