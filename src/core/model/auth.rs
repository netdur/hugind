use crate::core::config::settings::GlobalSettings;
use reqwest::header::AUTHORIZATION;

/// Build a reqwest client and attach the HF Bearer token if configured.
pub fn authenticated_request(client: &reqwest::Client, url: &str) -> reqwest::RequestBuilder {
    let mut request = client.get(url);

    if let Ok(settings) = GlobalSettings::load() {
        if let Some(token) = settings.get("hf_token") {
            if !token.is_empty() {
                request = request.header(AUTHORIZATION, format!("Bearer {}", token));
            }
        }
    }

    request
}
