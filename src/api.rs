use crate::config::AppConfig;
use reqwest::blocking::{Client, RequestBuilder};
use serde_json::{json, Value};
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("invalid API base URL")]
    InvalidBaseUrl,
    #[error("network request failed: {0}")]
    Network(#[from] reqwest::Error),
    #[error("Anixart API returned HTTP {status}: {body}")]
    Http { status: u16, body: String },
    #[error("Anixart API returned code {code}{message}")]
    ApiCode { code: i64, message: String },
}

#[derive(Clone)]
pub struct ApiClient {
    client: Client,
    base_url: String,
    token: Option<String>,
    beta: bool,
    version_code: i64,
}

impl ApiClient {
    pub fn from_config(config: &AppConfig) -> Result<Self, ApiError> {
        let base_url = config.base_url.trim().trim_end_matches('/').to_owned();
        if !(base_url.starts_with("https://") || base_url.starts_with("http://")) {
            return Err(ApiError::InvalidBaseUrl);
        }

        let client = Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(20))
            .user_agent(format!(
                "Anixart Arch/{} (Linux; desktop client)",
                env!("CARGO_PKG_VERSION")
            ))
            .build()?;

        Ok(Self {
            client,
            base_url,
            token: config.token.clone().filter(|value| !value.trim().is_empty()),
            beta: config.beta,
            version_code: config.version_code,
        })
    }

    pub fn config_toggles(&self) -> Result<Value, ApiError> {
        let params = [
            ("version_code", self.version_code.to_string()),
            ("is_beta", self.beta.to_string()),
        ];
        self.get("config/toggles", &params)
    }

    pub fn discover_interesting(&self) -> Result<Value, ApiError> {
        self.get("discover/interesting", &[])
    }

    pub fn search_releases(&self, query: &str, page: u32) -> Result<Value, ApiError> {
        self.post_json(
            &format!("search/releases/{page}"),
            &json!({ "query": query }),
        )
    }

    pub fn release(&self, id: i64) -> Result<Value, ApiError> {
        self.get(&format!("release/{id}"), &[])
    }

    /// First level of the episode API. Returns dubbing/voiceover `types`.
    pub fn episode_voiceovers(&self, release_id: i64) -> Result<Value, ApiError> {
        self.get(&format!("episode/{release_id}"), &[])
    }

    /// Second level. Returns player/source choices for a dubbing type.
    pub fn episode_sources(
        &self,
        release_id: i64,
        voiceover_id: i64,
    ) -> Result<Value, ApiError> {
        self.get(&format!("episode/{release_id}/{voiceover_id}"), &[])
    }

    /// Third level. Returns concrete episode entries including provider URLs.
    pub fn episode_list(
        &self,
        release_id: i64,
        voiceover_id: i64,
        source_id: i64,
    ) -> Result<Value, ApiError> {
        self.get(
            &format!("episode/{release_id}/{voiceover_id}/{source_id}"),
            &[],
        )
    }

    fn get(&self, path: &str, params: &[(&str, String)]) -> Result<Value, ApiError> {
        let mut request = self.client.get(self.endpoint(path));
        for (key, value) in params {
            request = request.query(&[(*key, value)]);
        }
        self.send(self.with_token(request))
    }

    fn post_json(&self, path: &str, body: &Value) -> Result<Value, ApiError> {
        let request = self.client.post(self.endpoint(path)).json(body);
        self.send(self.with_token(request))
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{}/{}", self.base_url, path.trim_start_matches('/'))
    }

    fn with_token(&self, request: RequestBuilder) -> RequestBuilder {
        match self.token.as_deref() {
            Some(token) => request.query(&[("token", token)]),
            None => request,
        }
    }

    fn send(&self, request: RequestBuilder) -> Result<Value, ApiError> {
        let response = request.send()?;
        let status = response.status();
        let text = response.text()?;

        if !status.is_success() {
            return Err(ApiError::Http {
                status: status.as_u16(),
                body: truncate(&text, 500),
            });
        }

        let value: Value = serde_json::from_str(&text).map_err(|err| ApiError::Http {
            status: status.as_u16(),
            body: format!("non-JSON response: {err}; {}", truncate(&text, 300)),
        })?;

        if let Some(code) = value.get("code").and_then(Value::as_i64) {
            if code != 0 {
                let detail = value
                    .get("message")
                    .or_else(|| value.get("error"))
                    .and_then(Value::as_str)
                    .map(|message| format!(": {message}"))
                    .unwrap_or_default();
                return Err(ApiError::ApiCode {
                    code,
                    message: detail,
                });
            }
        }

        Ok(value)
    }
}

fn truncate(input: &str, max: usize) -> String {
    if input.chars().count() <= max {
        return input.to_owned();
    }
    input.chars().take(max).collect::<String>() + "…"
}
