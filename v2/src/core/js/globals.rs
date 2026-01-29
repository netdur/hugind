use super::capabilities::{sys, llm, net};
use rquickjs::AsyncContext;

use crate::core::config::agent::AgentConfig;

pub async fn install_globals(ctx: &AsyncContext, config: &AgentConfig) -> rquickjs::Result<()> {
    sys::install(ctx).await?;
    llm::install(ctx, config).await?;
    net::install(ctx).await?;
    Ok(())
}
