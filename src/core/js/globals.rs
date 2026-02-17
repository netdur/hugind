use super::capabilities::{fs, llm, net, shell, sys, tools};
use rquickjs::AsyncContext;

use crate::core::config::agent::AgentConfig;
use crate::shared::logging::RunLogger;

pub async fn install_globals(
    ctx: &AsyncContext,
    config: &AgentConfig,
    fs_root: &std::path::Path,
    logger: Option<RunLogger>,
) -> rquickjs::Result<()> {
    sys::install(ctx, logger.clone()).await?;
    llm::install(ctx, config, logger.clone()).await?;
    net::install(ctx, config, logger.clone()).await?;
    shell::install(ctx, config, logger.clone()).await?;
    fs::install(ctx, config, fs_root, logger.clone()).await?;
    tools::install(ctx, config, logger).await?;
    Ok(())
}
