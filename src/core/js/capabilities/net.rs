use futures::StreamExt;
use reqwest::Url;
use rquickjs::{AsyncContext, Class, Result};

use crate::core::config::agent::{AgentConfig, NetPermissions};
use crate::core::runtime::util::{parse_duration_string, parse_memory_string};
use crate::shared::logging::RunLogger;

#[derive(rquickjs::JsLifetime)]
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
            ensure_http_scheme(&current)
                .map_err(|e| rquickjs::Error::new_loading_message("Network Error", e))?;

            let host = current.host_str().unwrap_or("");
            let port = current.port_or_known_default().unwrap_or(80);

            ensure_host_allowed(host, &self.permission)
                .map_err(|e| rquickjs::Error::new_loading_message("Network Error", e))?;

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

                ensure_public_network_access(&self.permission, &ips)
                    .map_err(|e| rquickjs::Error::new_loading_message("Network Error", e))?;
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
                    .map_err(|e| {
                        rquickjs::Error::new_loading_message("Network Error", e.to_string())
                    })?;
                current = current.join(location).map_err(|e| {
                    rquickjs::Error::new_loading_message("Network Error", e.to_string())
                })?;
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
                let chunk = item.map_err(|e| {
                    rquickjs::Error::new_loading_message("Network Error", e.to_string())
                })?;
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

fn ensure_http_scheme(url: &Url) -> std::result::Result<(), String> {
    crate::core::runtime::util::validate_http_scheme(url.scheme())
}

fn ensure_host_allowed(host: &str, permission: &NetPermissions) -> std::result::Result<(), String> {
    crate::core::runtime::util::validate_host_allowed(host, permission)
}

fn ensure_public_network_access(
    permission: &NetPermissions,
    ips: &[std::net::IpAddr],
) -> std::result::Result<(), String> {
    crate::core::runtime::util::validate_public_network(permission, ips)
}

pub async fn install(
    ctx: &AsyncContext,
    config: &AgentConfig,
    logger: Option<RunLogger>,
) -> Result<()> {
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

    ctx.async_with(|ctx| {
        Box::pin(async move {
            let cls = Class::instance(ctx.clone(), net)?;
            ctx.globals().set("net", cls)?;
            Ok(())
        })
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::{ensure_host_allowed, ensure_http_scheme, ensure_public_network_access};
    use crate::core::config::agent::NetPermissions;
    use reqwest::Url;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn allows_http_and_https_schemes() {
        let http = Url::parse("http://example.com").expect("url");
        let https = Url::parse("https://example.com").expect("url");
        assert!(ensure_http_scheme(&http).is_ok());
        assert!(ensure_http_scheme(&https).is_ok());
    }

    #[test]
    fn rejects_non_http_scheme() {
        let ftp = Url::parse("ftp://example.com").expect("url");
        let err = ensure_http_scheme(&ftp).expect_err("must reject");
        assert!(err.contains("URL scheme 'ftp' is not allowed."));
    }

    #[test]
    fn allows_any_host_when_lists_are_empty() {
        let perm = NetPermissions::default();
        assert!(ensure_host_allowed("example.com", &perm).is_ok());
    }

    #[test]
    fn allows_exact_and_subdomain_matches() {
        let mut perm = NetPermissions::default();
        perm.allowed_domains = vec!["example.com".to_string()];
        assert!(ensure_host_allowed("example.com", &perm).is_ok());
        assert!(ensure_host_allowed("api.example.com", &perm).is_ok());
        assert!(ensure_host_allowed("badexample.com", &perm).is_err());
    }

    #[test]
    fn allows_ip_when_ip_is_whitelisted() {
        let mut perm = NetPermissions::default();
        perm.allowed_ips = vec!["127.0.0.1".to_string()];
        assert!(ensure_host_allowed("127.0.0.1", &perm).is_ok());
        assert!(ensure_host_allowed("127.0.0.2", &perm).is_err());
    }

    #[test]
    fn rejects_host_not_in_any_allowlist() {
        let mut perm = NetPermissions::default();
        perm.allowed_domains = vec!["allowed.com".to_string()];
        perm.allowed_ips = vec!["10.0.0.2".to_string()];
        let err = ensure_host_allowed("blocked.com", &perm).expect_err("must reject");
        assert!(err.contains("Domain/IP 'blocked.com' is not in the allowed list."));
    }

    #[test]
    fn blocks_private_ips_when_private_network_blocking_is_enabled() {
        let mut perm = NetPermissions::default();
        perm.block_private_networks = true;
        let ips = vec![IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))];
        let err = ensure_public_network_access(&perm, &ips).expect_err("must reject");
        assert!(err.contains("Access to private network blocked (IP: 127.0.0.1)"));
    }

    #[test]
    fn allows_private_ips_when_private_network_blocking_is_disabled() {
        let perm = NetPermissions::default();
        let ips = vec![IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))];
        assert!(ensure_public_network_access(&perm, &ips).is_ok());
    }
}
