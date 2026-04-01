use super::capabilities::{fs, llm, net, shell, sys, team, tools};
use rquickjs::AsyncContext;

use crate::core::config::agent::AgentConfig;
use crate::core::orchestrator::context::TeamContext;
use crate::shared::logging::RunLogger;

pub async fn install_globals(
    ctx: &AsyncContext,
    config: &AgentConfig,
    fs_root: &std::path::Path,
    logger: Option<RunLogger>,
    team_ctx: Option<&TeamContext>,
) -> rquickjs::Result<()> {
    sys::install(ctx, logger.clone()).await?;
    llm::install(ctx, config, logger.clone()).await?;
    net::install(ctx, config, logger.clone()).await?;
    shell::install(ctx, config, logger.clone()).await?;
    fs::install(ctx, config, fs_root, logger.clone()).await?;
    tools::install(ctx, config, logger.clone()).await?;

    if let Some(tc) = team_ctx {
        team::install(ctx, tc, logger).await?;
    }

    Ok(())
}
