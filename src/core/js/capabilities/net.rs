use futures::StreamExt;
use reqwest::Url;
use rquickjs::{AsyncContext, Class, Result};

use crate::core::config::agent::{AgentConfig, NetPermissions};
use crate::core::runtime::util::{is_private_ip, parse_duration_string, parse_memory_string};
use crate::shared::logging::RunLogger;

#[rquickjs::class]
pub struct Net {
    client: reqwest::Client,
    permission: NetPermissions,
    logger: Option<RunLogger>,
}

impl<'js> rquickjs::class::Trace<'js> for Net {
    fn trace<'a>(&self, _tracer: rquickjs::class::Tracer<'a, 'js>) {}
}

#[rquickjs::methods]
impl Net {
    pub async fn fetch(&self, url: String) -> Result<String> {
        if let Some(logger) = &self.logger {
            logger.log_line(format!("host.net.fetch url={}", url));
        }
        if !self.permission.allow {
            return Err(rquickjs::Error::new_loading_message(
                "Network Error",
                "Network access is disabled for this agent.",
            ));
        }

        let mut current = Url::parse(&url)
            .map_err(|e| rquickjs::Error::new_loading_message("Invalid URL", e.to_string()))?;

        let max_redirects = 5;

        for _ in 0..=max_redirects {
            match current.scheme() {
                "http" | "https" => {}
                _ => {
                    return Err(rquickjs::Error::new_loading_message(
                        "Network Error",
                        format!("URL scheme '{}' is not allowed.", current.scheme()),
                    ))
                }
            }

            let host = current.host_str().unwrap_or("");
            let port = current.port_or_known_default().unwrap_or(80);

            if !self.permission.allowed_domains.is_empty() || !self.permission.allowed_ips.is_empty() {
                let allowed = self
                    .permission
                    .allowed_domains
                    .iter()
                    .any(|d| host == d || host.ends_with(&format!(".{}", d)));
                if !allowed {
                    let is_ip_allowed = if let Ok(_ip) = host.parse::<std::net::IpAddr>() {
                        self.permission.allowed_ips.iter().any(|allowed_ip| allowed_ip == host)
                    } else {
                        false
                    };
                    if !is_ip_allowed {
                        return Err(rquickjs::Error::new_loading_message(
                            "Network Error",
                            format!("Domain/IP '{}' is not in the allowed list.", host),
                        ));
                    }
                }
            }

            if self.permission.block_private_networks {
                let ips: Vec<std::net::IpAddr> = if let Ok(ip) = host.parse::<std::net::IpAddr>() {
                    vec![ip]
                } else {
                    let addr_str = format!("{}:{}", host, port);
                    tokio::net::lookup_host(&addr_str)
                        .await
                        .map_err(|e| {
                            rquickjs::Error::new_loading_message(
                                "Network Error",
                                format!("DNS resolution failed for {}: {}", host, e),
                            )
                        })?
                        .map(|sa| sa.ip())
                        .collect()
                };

                for ip in ips {
                    if is_private_ip(&ip) {
                        return Err(rquickjs::Error::new_loading_message(
                            "Network Error",
                            format!("Access to private network blocked (IP: {})", ip),
                        ));
                    }
                }
            }

            let timeout_duration = self
                .permission
                .timeout
                .as_deref()
                .and_then(parse_duration_string)
                .unwrap_or(std::time::Duration::from_secs(30));

            let max_bytes = self
                .permission
                .max_response_bytes
                .as_deref()
                .and_then(parse_memory_string)
                .unwrap_or(10 * 1024 * 1024);

            let res = self
                .client
                .get(current.clone())
                .timeout(timeout_duration)
                .send()
                .await
                .map_err(|e| {
                    rquickjs::Error::new_loading_message("Network Request Failed", e.to_string())
                })?;

            if res.status().is_redirection() {
                let location = res
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .ok_or_else(|| {
                        rquickjs::Error::new_loading_message(
                            "Network Error",
                            "Redirect missing Location header",
                        )
                    })?
                    .to_str()
                    .map_err(|e| rquickjs::Error::new_loading_message("Network Error", e.to_string()))?;
                current = current
                    .join(location)
                    .map_err(|e| rquickjs::Error::new_loading_message("Network Error", e.to_string()))?;
                continue;
            }

            if !res.status().is_success() {
                return Err(rquickjs::Error::new_loading_message(
                    "Network Error",
                    format!("HTTP Status: {}", res.status()),
                ));
            }

            let mut content = Vec::new();
            let mut stream = res.bytes_stream();

            while let Some(item) = stream.next().await {
                let chunk = item
                    .map_err(|e| rquickjs::Error::new_loading_message("Network Error", e.to_string()))?;
                if content.len() + chunk.len() > max_bytes {
                    let remaining = max_bytes - content.len();
                    content.extend_from_slice(&chunk[..remaining]);
                    break;
                }
                content.extend_from_slice(&chunk);
            }

            let text = String::from_utf8_lossy(&content).to_string();
            return Ok(text);
        }

        Err(rquickjs::Error::new_loading_message(
            "Network Error",
            "Too many redirects",
        ))
    }
}

pub async fn install(ctx: &AsyncContext, config: &AgentConfig, logger: Option<RunLogger>) -> Result<()> {
    let perm = if let Some(p) = &config.permissions {
        p.network.clone().unwrap_or_default()
    } else {
        NetPermissions::default()
    };

    let client = reqwest::Client::builder()
        .user_agent("Hugind/0.1 (http://github.com/netdur/hugind)")
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| rquickjs::Error::new_loading_message("Client Build Error", e.to_string()))?;

    let net = Net {
        client,
        permission: perm,
        logger,
    };

    ctx.async_with(|ctx| Box::pin(async move {
        let cls = Class::instance(ctx.clone(), net)?;
        ctx.globals().set("net", cls)?;
        Ok(())
    }))
    .await
}
