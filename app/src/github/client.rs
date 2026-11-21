use crate::error::{JsError, JsResult};
use crate::github::throttle::RequestThrottle;
use crate::js;
use crate::metrics::Metrics;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use reqwest::Method;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

const URI_COMPONENT_SAFE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'!')
    .remove(b'~')
    .remove(b'*')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')');

const STATUSES_NEVER_RETRIED: [u16; 6] = [400, 401, 403, 404, 422, 451];
const RETRY_ATTEMPTS: u32 = 3;
const INSTALLATION_TOKEN_REFRESH_MARGIN: Duration = Duration::from_secs(5 * 60);

pub fn encode_uri_component(text: &str) -> String {
    utf8_percent_encode(text, URI_COMPONENT_SAFE).to_string()
}

#[derive(Debug, Clone)]
pub struct GithubResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub data: Value,
}

struct CachedInstallationToken {
    token: String,
    refresh_after: SystemTime,
}

pub struct GithubAppSettings {
    pub base_url: String,
    pub app_id: String,
    pub private_key_pem: String,
    pub retry_base_delay: Duration,
    pub throttle: RequestThrottle,
}

pub struct GithubApp {
    http: reqwest::Client,
    base_url: String,
    base_path: String,
    app_id: String,
    signing_key: EncodingKey,
    installation_tokens: Mutex<HashMap<u64, CachedInstallationToken>>,
    retry_base_delay: Duration,
    throttle: RequestThrottle,
    metrics: Option<Arc<Metrics>>,
}

impl GithubApp {
    pub fn new(settings: GithubAppSettings, metrics: Option<Arc<Metrics>>) -> JsResult<Self> {
        let signing_key = EncodingKey::from_rsa_pem(settings.private_key_pem.as_bytes())
            .map_err(|error| JsError::error(format!("invalid PRIVATE_KEY: {error}")))?;
        let http = reqwest::Client::builder()
            .user_agent(concat!("mergeable-rust/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| JsError::error(error.to_string()))?;
        let base_url = settings.base_url.trim_end_matches('/').to_string();
        let base_path = reqwest::Url::parse(&base_url)
            .map(|url| url.path().trim_end_matches('/').to_string())
            .unwrap_or_default();
        Ok(GithubApp {
            http,
            base_url,
            base_path,
            app_id: settings.app_id,
            signing_key,
            installation_tokens: Mutex::new(HashMap::new()),
            retry_base_delay: settings.retry_base_delay,
            throttle: settings.throttle,
            metrics,
        })
    }

    pub fn installation(self: &Arc<Self>, installation_id: Option<u64>) -> InstallationClient {
        InstallationClient {
            app: Arc::clone(self),
            installation_id,
            unauthenticated_reason: None,
        }
    }

    pub fn unauthenticated(self: &Arc<Self>, reason: String) -> InstallationClient {
        InstallationClient {
            app: Arc::clone(self),
            installation_id: None,
            unauthenticated_reason: Some(reason),
        }
    }

    fn app_jwt(&self) -> JsResult<String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or_default();
        let issued_at = now.saturating_sub(30);
        let issuer = self
            .app_id
            .parse::<u64>()
            .map(Value::from)
            .unwrap_or_else(|_| Value::String(self.app_id.clone()));
        let claims = json!({
            "iat": issued_at,
            "exp": issued_at + 600,
            "iss": issuer,
        });
        jsonwebtoken::encode(&Header::new(Algorithm::RS256), &claims, &self.signing_key)
            .map_err(|error| JsError::error(format!("could not sign app JWT: {error}")))
    }

    async fn installation_token(&self, installation_id: u64) -> JsResult<String> {
        let mut tokens = self.installation_tokens.lock().await;
        if let Some(cached) = tokens.get(&installation_id)
            && SystemTime::now() < cached.refresh_after
        {
            return Ok(cached.token.clone());
        }
        let url = format!(
            "{}/app/installations/{installation_id}/access_tokens",
            self.base_url
        );
        let response = self
            .http
            .post(&url)
            .header("authorization", format!("bearer {}", self.app_jwt()?))
            .header("accept", DEFAULT_ACCEPT)
            .send()
            .await
            .map_err(|error| JsError::http(500, error.to_string(), None))?;
        let status = response.status().as_u16();
        let data: Value = response.json().await.unwrap_or(Value::Null);
        if status >= 400 {
            return Err(JsError::http(status, error_message(&data), Some(data)));
        }
        let token = js::as_str(data.get("token"))
            .ok_or_else(|| JsError::error("installation token missing in response"))?
            .to_string();
        let refresh_after = js::as_str(data.get("expires_at"))
            .and_then(|expires| chrono::DateTime::parse_from_rfc3339(expires).ok())
            .map(|expires| SystemTime::from(expires) - INSTALLATION_TOKEN_REFRESH_MARGIN)
            .unwrap_or_else(|| SystemTime::now() + Duration::from_secs(50 * 60));
        tokens.insert(
            installation_id,
            CachedInstallationToken {
                token: token.clone(),
                refresh_after,
            },
        );
        Ok(token)
    }
}

#[derive(Clone)]
pub struct InstallationClient {
    app: Arc<GithubApp>,
    pub installation_id: Option<u64>,
    unauthenticated_reason: Option<String>,
}

struct PreparedRequest {
    method: Method,
    url: String,
    body: Option<Value>,
    accept: String,
}

const DEFAULT_ACCEPT: &str = "application/vnd.github.v3+json";
const RAW_ACCEPT: &str = "application/vnd.github.v3.raw";

impl InstallationClient {
    fn prepare(&self, route: &str, params: &Value) -> JsResult<PreparedRequest> {
        let (method_name, path_template) = route
            .split_once(' ')
            .ok_or_else(|| JsError::error(format!("invalid route {route}")))?;
        let method = Method::from_bytes(method_name.as_bytes())
            .map_err(|error| JsError::error(error.to_string()))?;
        let mut remaining: Map<String, Value> = params.as_object().cloned().unwrap_or_default();
        let accept = match remaining.shift_remove("mediaType") {
            Some(media_type) => preview_accept_header(&media_type),
            None => None,
        }
        .unwrap_or_else(|| DEFAULT_ACCEPT.to_string());
        let mut path = String::new();
        let mut rest = path_template;
        while let Some(start) = rest.find('{') {
            path.push_str(&rest[..start]);
            let end = rest[start..]
                .find('}')
                .map(|offset| start + offset)
                .ok_or_else(|| JsError::error(format!("invalid route {route}")))?;
            let parameter_name = &rest[start + 1..end];
            let value = remaining.shift_remove(parameter_name);
            if let Some(present) = value.as_ref().filter(|present| !present.is_null()) {
                path.push_str(&encode_uri_component(&js::to_js_string(Some(present))));
            }
            rest = &rest[end + 1..];
        }
        path.push_str(rest);
        let mut url = format!("{}{}", self.app.base_url, path);
        let body = if method == Method::GET || method == Method::HEAD {
            let query: Vec<String> = remaining
                .iter()
                .map(|(name, value)| {
                    let text = js::to_js_string(Some(value));
                    if name == "q" {
                        let encoded: Vec<String> =
                            text.split('+').map(encode_uri_component).collect();
                        format!("q={}", encoded.join("+"))
                    } else {
                        format!("{name}={}", encode_uri_component(&text))
                    }
                })
                .collect();
            if !query.is_empty() {
                url.push(if url.contains('?') { '&' } else { '?' });
                url.push_str(&query.join("&"));
            }
            None
        } else if let Some(data) = remaining.shift_remove("data") {
            Some(data)
        } else if remaining.is_empty() {
            None
        } else {
            Some(Value::Object(remaining))
        };
        Ok(PreparedRequest {
            method,
            url,
            body,
            accept,
        })
    }

    pub fn endpoint_url(&self, route: &str, params: &Value) -> JsResult<String> {
        Ok(self.prepare(route, params)?.url)
    }

    pub async fn request(&self, route: &str, params: Value) -> JsResult<GithubResponse> {
        let prepared = self.prepare(route, &params)?;
        self.send_with_retries(&prepared).await
    }

    fn explain_missing_authentication(
        &self,
        prepared: &PreparedRequest,
        mut error: JsError,
    ) -> JsError {
        let Some(reason) = &self.unauthenticated_reason else {
            return error;
        };
        match error.status {
            Some(404) => {
                error.message =
                    format!("Not found. May be due to lack of authentication. Reason: {reason}");
            }
            Some(401) => {
                error.message = format!(
                    "Unauthorized. \"{} {}\" failed most likely due to lack of authentication. Reason: {reason}",
                    prepared.method, prepared.url
                );
            }
            Some(status) if (400..500).contains(&status) => {
                let trimmed = error
                    .message
                    .strip_suffix('.')
                    .unwrap_or(&error.message)
                    .to_string();
                error.message =
                    format!("{trimmed}. May be caused by lack of authentication ({reason}).");
            }
            _ => {}
        }
        error
    }

    pub async fn request_raw(&self, route: &str, params: Value) -> JsResult<GithubResponse> {
        let mut prepared = self.prepare(route, &params)?;
        prepared.accept = RAW_ACCEPT.to_string();
        self.send_with_retries(&prepared).await
    }

    pub async fn request_url(&self, url: &str) -> JsResult<GithubResponse> {
        let absolute_url = if url.starts_with('/') {
            format!("{}{}", self.app.base_url, url)
        } else {
            url.to_string()
        };
        let prepared = PreparedRequest {
            method: Method::GET,
            url: absolute_url,
            body: None,
            accept: DEFAULT_ACCEPT.to_string(),
        };
        self.send_with_retries(&prepared).await
    }

    pub async fn paginate(&self, route: &str, params: Value) -> JsResult<Vec<Value>> {
        let mut collected = Vec::new();
        let mut response = match self.request(route, params).await {
            Err(error) if error.status == Some(409) => return Ok(collected),
            other => other?,
        };
        loop {
            match response.data {
                Value::Array(items) => collected.extend(items),
                Value::Object(map) => {
                    if let Some(Value::Array(items)) = map
                        .iter()
                        .find(|(key, value)| {
                            !matches!(
                                key.as_str(),
                                "incomplete_results" | "repository_selection" | "total_count"
                            ) && value.is_array()
                        })
                        .map(|(_, value)| value.clone())
                    {
                        collected.extend(items);
                    }
                }
                _ => {}
            }
            let next = response
                .headers
                .get("link")
                .and_then(|link| next_page_url(link));
            match next {
                Some(next_url) => {
                    response = match self.request_url(&next_url).await {
                        Err(error) if error.status == Some(409) => return Ok(collected),
                        other => other?,
                    }
                }
                None => return Ok(collected),
            }
        }
    }

    async fn send_with_retries(&self, prepared: &PreparedRequest) -> JsResult<GithubResponse> {
        self.send_with_retries_unexplained(prepared)
            .await
            .map_err(|error| self.explain_missing_authentication(prepared, error))
    }

    async fn send_with_retries_unexplained(
        &self,
        prepared: &PreparedRequest,
    ) -> JsResult<GithubResponse> {
        let mut retry_count: u32 = 0;
        loop {
            match self.send_once(prepared).await {
                Err(error)
                    if retry_count < RETRY_ATTEMPTS
                        && error.status.is_some_and(|status| {
                            status >= 400 && !STATUSES_NEVER_RETRIED.contains(&status)
                        }) =>
                {
                    retry_count += 1;
                    tokio::time::sleep(self.app.retry_base_delay * retry_count.pow(2)).await;
                }
                outcome => return outcome,
            }
        }
    }

    async fn send_once(&self, prepared: &PreparedRequest) -> JsResult<GithubResponse> {
        let full_path = reqwest::Url::parse(&prepared.url)
            .map(|url| url.path().to_string())
            .unwrap_or_default();
        let pathname = full_path
            .strip_prefix(self.app.base_path.as_str())
            .filter(|rest| rest.starts_with('/'))
            .unwrap_or(&full_path)
            .to_string();
        let _permit = self
            .app
            .throttle
            .acquire(prepared.method.as_str(), &pathname)
            .await;
        let mut builder = self
            .app
            .http
            .request(prepared.method.clone(), &prepared.url)
            .header("accept", prepared.accept.as_str());
        if let Some(installation_id) = self.installation_id {
            let token = self.app.installation_token(installation_id).await?;
            builder = builder.header("authorization", format!("token {token}"));
        }
        if let Some(body) = &prepared.body {
            builder = builder
                .header("content-type", "application/json; charset=utf-8")
                .body(js::stringify(body));
        }
        let response = builder
            .send()
            .await
            .map_err(|error| JsError::http(500, error.to_string(), None))?;
        let status = response.status().as_u16();
        let headers: HashMap<String, String> = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|text| (name.as_str().to_ascii_lowercase(), text.to_string()))
            })
            .collect();
        if let (Some(metrics), Some(installation_id)) = (&self.app.metrics, self.installation_id) {
            metrics.record_rate_limit(installation_id, &headers);
        }
        let is_json = headers
            .get("content-type")
            .is_some_and(|content_type| content_type.contains("application/json"));
        let text = response.text().await.unwrap_or_default();
        let data = if is_json {
            serde_json::from_str(&text).unwrap_or(Value::String(text))
        } else if text.is_empty() {
            Value::String(String::new())
        } else {
            Value::String(text)
        };
        if status == 304 {
            return Err(JsError::http(status, "Not modified", Some(data)));
        }
        if status >= 400 {
            return Err(JsError::http(status, error_message(&data), Some(data)));
        }
        Ok(GithubResponse {
            status,
            headers,
            data,
        })
    }

    pub fn base_path(&self) -> &str {
        &self.app.base_path
    }
}

fn preview_accept_header(media_type: &Value) -> Option<String> {
    let Some(Value::Array(previews)) = media_type.get("previews") else {
        return None;
    };
    if previews.is_empty() {
        return None;
    }
    let format = match media_type.get("format").and_then(Value::as_str) {
        Some(format) => format!(".{format}"),
        None => "+json".to_string(),
    };
    Some(
        previews
            .iter()
            .map(|preview| {
                format!(
                    "application/vnd.github.{}-preview{format}",
                    js::to_js_string(Some(preview))
                )
            })
            .collect::<Vec<_>>()
            .join(","),
    )
}

fn error_message(data: &Value) -> String {
    match data {
        Value::String(text) => text.clone(),
        Value::Object(map) => {
            let suffix = map
                .get("documentation_url")
                .map(|url| format!(" - {}", js::to_js_string(Some(url))))
                .unwrap_or_default();
            match map.get("message") {
                Some(message) => match map.get("errors") {
                    Some(Value::Array(errors)) => format!(
                        "{}: {}{suffix}",
                        js::to_js_string(Some(message)),
                        errors
                            .iter()
                            .map(js::stringify)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    _ => format!("{}{suffix}", js::to_js_string(Some(message))),
                },
                None => format!("Unknown error: {}", js::stringify(data)),
            }
        }
        other => format!("Unknown error: {}", js::stringify(other)),
    }
}

fn next_page_url(link_header: &str) -> Option<String> {
    link_header.split(',').find_map(|part| {
        let (target, relation) = part.split_once(';')?;
        if relation.contains("rel=\"next\"") {
            Some(
                target
                    .trim()
                    .trim_start_matches('<')
                    .trim_end_matches('>')
                    .to_string(),
            )
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn octokit_error_message_appends_documentation_url_and_errors() {
        let data = json!({"message": "Validation Failed", "errors": [{"code": "invalid"}], "documentation_url": "https://docs"});
        assert_eq!(
            error_message(&data),
            "Validation Failed: {\"code\":\"invalid\"} - https://docs"
        );
    }

    #[test]
    fn link_header_next_page_is_found() {
        let header = "<https://api/x?page=2>; rel=\"next\", <https://api/x?page=5>; rel=\"last\"";
        assert_eq!(
            next_page_url(header).as_deref(),
            Some("https://api/x?page=2")
        );
    }

    #[test]
    fn uri_component_encoding_matches_javascript() {
        assert_eq!(
            encode_uri_component(".github/mergeable.yml"),
            ".github%2Fmergeable.yml"
        );
        assert_eq!(encode_uri_component("a b+c"), "a%20b%2Bc");
    }
}
