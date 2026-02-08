use super::capabilities::{fs, llm, net, shell, sys};
use rquickjs::AsyncContext;

use crate::core::config::agent::AgentConfig;

pub async fn install_globals(
    ctx: &AsyncContext,
    config: &AgentConfig,
    agent_root: &std::path::Path,
) -> rquickjs::Result<()> {
    sys::install(ctx).await?;
    llm::install(ctx, config).await?;
    net::install(ctx, config).await?;
    shell::install(ctx, config).await?;
    fs::install(ctx, config, agent_root).await?;
    Ok(())
}
