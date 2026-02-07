use rquickjs::{AsyncContext, Class, Result};
use crate::core::config::agent::{AgentConfig, NetPermissions};

#[rquickjs::class]
pub struct Net {
    client: reqwest::Client,
    permission: NetPermissions,
}

impl<'js> rquickjs::class::Trace<'js> for Net {
    fn trace<'a>(&self, _tracer: rquickjs::class::Tracer<'a, 'js>) {}
}

#[rquickjs::methods]
impl Net {
    pub async fn fetch(&self, url: String) -> Result<String> {
        
        if !self.permission.allow {
             return Err(rquickjs::Error::new_loading_message("Network Error", "Network access is disabled for this agent."));
        }

        
        let parsed_url = reqwest::Url::parse(&url)
            .map_err(|e| rquickjs::Error::new_loading_message("Invalid URL", e.to_string()))?;
        
        let host = parsed_url.host_str().unwrap_or("");

        
        
        
        
        
        
        
        
        if !self.permission.allowed_domains.is_empty() {
             let allowed = self.permission.allowed_domains.iter().any(|d| host == d || host.ends_with(&format!(".{}", d)));
             if !allowed {
                 return Err(rquickjs::Error::new_loading_message("Network Error", format!("Domain '{}' is not in the allowed list.", host)));
             }
        }

        
        let res = self.client.get(parsed_url)
            .send()
            .await
            .map_err(|e| rquickjs::Error::new_loading_message("Network Request Failed", e.to_string()))?;

        if !res.status().is_success() {
             return Err(rquickjs::Error::new_loading_message("Network Error", format!("HTTP Status: {}", res.status())));
        }

        let text = res.text().await
            .map_err(|e| rquickjs::Error::new_loading_message("Response Read Error", e.to_string()))?;

        Ok(text)
    }
}

pub async fn install(ctx: &AsyncContext, config: &AgentConfig) -> Result<()> {
    let perm = if let Some(p) = &config.permissions {
        p.network.clone().unwrap_or_default()
    } else {
        NetPermissions::default()
    };

    
    let client = reqwest::Client::builder()
        .user_agent("Hugind/0.1 (http://github.com/netdur/hugind)")
        .build()
        .map_err(|e| rquickjs::Error::new_loading_message("Client Build Error", e.to_string()))?;

    let net = Net {
        client,
        permission: perm,
    };

    ctx.async_with(|ctx| Box::pin(async move {
        let cls = Class::instance(ctx.clone(), net)?;
        ctx.globals().set("net", cls)?;
        Ok(())
    })).await
}
