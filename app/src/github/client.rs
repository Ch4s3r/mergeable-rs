use crate::error::{JsError, JsResult};
use crate::github::app_auth::{next_page_url, octokit_error_message};
use crate::github::throttle::RequestThrottle;
use crate::js;
use crate::metrics::Metrics;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use reqwest::Method;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde_json::{Map, Value, json};
use std::borrow::Cow;
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
const INSTALLATION_TOKEN_REFRESH_MARGIN: Duration = Duration::from_mins(5);

pub fn encode_uri_component(text: &str) -> String {
    utf8_percent_encode(text, URI_COMPONENT_SAFE).to_string()
}

fn push_uri_component(target: &mut String, text: &str) {
    target.extend(utf8_percent_encode(text, URI_COMPONENT_SAFE));
}

fn js_text(value: &Value) -> Cow<'_, str> {
    match value {
        Value::String(text) => Cow::Borrowed(text),
        other => Cow::Owned(js::to_js_string(Some(other))),
    }
}

pub fn header_text<'headers>(headers: &'headers HeaderMap, name: &str) -> Option<&'headers str> {
    headers
        .get_all(name)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .next_back()
}

#[derive(Debug, Clone)]
pub struct GithubResponse {
    pub status: u16,
    pub headers: HeaderMap,
    pub data: Value,
}

impl GithubResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        header_text(&self.headers, name)
    }
}

struct CachedInstallationToken {
    authorization: HeaderValue,
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
    jwt_issuer: Value,
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
        let jwt_issuer = settings
            .app_id
            .parse::<u64>()
            .map_or_else(|_| Value::String(settings.app_id), Value::from);
        Ok(GithubApp {
            http,
            base_url,
            base_path,
            jwt_issuer,
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
        let claims = json!({
            "iat": issued_at,
            "exp": issued_at + 600,
            "iss": self.jwt_issuer,
        });
        jsonwebtoken::encode(&Header::new(Algorithm::RS256), &claims, &self.signing_key)
            .map_err(|error| JsError::error(format!("could not sign app JWT: {error}")))
    }

    async fn installation_authorization(&self, installation_id: u64) -> JsResult<HeaderValue> {
        let mut tokens = self.installation_tokens.lock().await;
        if let Some(cached) = tokens.get(&installation_id)
            && SystemTime::now() < cached.refresh_after
        {
            return Ok(cached.authorization.clone());
        }
        let url = format!(
            "{}/app/installations/{installation_id}/access_tokens",
            self.base_url
        );
        let response = self
            .http
            .post(&url)
            .header(AUTHORIZATION, format!("bearer {}", self.app_jwt()?))
            .header(ACCEPT, DEFAULT_ACCEPT)
            .send()
            .await
            .map_err(|error| JsError::http(500, error.to_string(), None))?;
        let status = response.status().as_u16();
        let data: Value = response.json().await.unwrap_or(Value::Null);
        if status >= 400 {
            return Err(JsError::http(
                status,
                octokit_error_message(&data),
                Some(data),
            ));
        }
        let token = js::as_str(data.get("token"))
            .ok_or_else(|| JsError::error("installation token missing in response"))?;
        let authorization = HeaderValue::try_from(format!("token {token}"))
            .map_err(|error| JsError::http(500, error.to_string(), None))?;
        let refresh_after = js::as_str(data.get("expires_at"))
            .and_then(|expires| chrono::DateTime::parse_from_rfc3339(expires).ok())
            .map_or_else(
                || SystemTime::now() + Duration::from_mins(50),
                |expires| SystemTime::from(expires) - INSTALLATION_TOKEN_REFRESH_MARGIN,
            );
        tokens.insert(
            installation_id,
            CachedInstallationToken {
                authorization: authorization.clone(),
                refresh_after,
            },
        );
        Ok(authorization)
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
    accept: Cow<'static, str>,
}

const DEFAULT_ACCEPT: &str = "application/vnd.github.v3+json";
const RAW_ACCEPT: &str = "application/vnd.github.v3.raw";

impl InstallationClient {
    fn prepare(&self, route: &str, params: Value) -> JsResult<PreparedRequest> {
        let (method_name, path_template) = route
            .split_once(' ')
            .ok_or_else(|| JsError::error(format!("invalid route {route}")))?;
        let method = Method::from_bytes(method_name.as_bytes())
            .map_err(|error| JsError::error(error.to_string()))?;
        let mut remaining = match params {
            Value::Object(map) => map,
            _ => Map::new(),
        };
        let accept = remaining
            .shift_remove("mediaType")
            .and_then(|media_type| preview_accept_header(&media_type))
            .map_or(Cow::Borrowed(DEFAULT_ACCEPT), Cow::Owned);
        let mut url = String::with_capacity(self.app.base_url.len() + path_template.len() + 32);
        url.push_str(&self.app.base_url);
        let mut rest = path_template;
        while let Some(start) = rest.find('{') {
            url.push_str(&rest[..start]);
            let end = rest[start..]
                .find('}')
                .map(|offset| start + offset)
                .ok_or_else(|| JsError::error(format!("invalid route {route}")))?;
            let parameter_name = &rest[start + 1..end];
            if let Some(present) = remaining
                .shift_remove(parameter_name)
                .filter(|present| !present.is_null())
            {
                push_uri_component(&mut url, &js_text(&present));
            }
            rest = &rest[end + 1..];
        }
        url.push_str(rest);
        let body = if method == Method::GET || method == Method::HEAD {
            let mut separator = if url.contains('?') { '&' } else { '?' };
            for (name, value) in &remaining {
                url.push(separator);
                separator = '&';
                let text = js_text(value);
                if name == "q" {
                    url.push_str("q=");
                    for (index, term) in text.split('+').enumerate() {
                        if index > 0 {
                            url.push('+');
                        }
                        push_uri_component(&mut url, term);
                    }
                } else {
                    url.push_str(name);
                    url.push('=');
                    push_uri_component(&mut url, &text);
                }
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
        Ok(self.prepare(route, params.clone())?.url)
    }

    pub async fn request(&self, route: &str, params: Value) -> JsResult<GithubResponse> {
        let prepared = self.prepare(route, params)?;
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
                let trimmed = error.message.strip_suffix('.').unwrap_or(&error.message);
                error.message =
                    format!("{trimmed}. May be caused by lack of authentication ({reason}).");
            }
            _ => {}
        }
        error
    }

    pub async fn request_raw(&self, route: &str, params: Value) -> JsResult<GithubResponse> {
        let mut prepared = self.prepare(route, params)?;
        prepared.accept = Cow::Borrowed(RAW_ACCEPT);
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
            accept: Cow::Borrowed(DEFAULT_ACCEPT),
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
            let next = response.header("link").and_then(next_page_url);
            match response.data {
                Value::Array(items) => collected.extend(items),
                Value::Object(map) => {
                    if let Some((_, Value::Array(items))) = map.into_iter().find(|(key, value)| {
                        !matches!(
                            key.as_str(),
                            "incomplete_results" | "repository_selection" | "total_count"
                        ) && value.is_array()
                    }) {
                        collected.extend(items);
                    }
                }
                _ => {}
            }
            let Some(next_url) = next else {
                return Ok(collected);
            };
            response = match self.request_url(&next_url).await {
                Err(error) if error.status == Some(409) => return Ok(collected),
                other => other?,
            };
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
        let parsed_url = reqwest::Url::parse(&prepared.url);
        let full_path = parsed_url.as_ref().map_or("", reqwest::Url::path);
        let pathname = full_path
            .strip_prefix(self.app.base_path.as_str())
            .filter(|rest| rest.starts_with('/'))
            .unwrap_or(full_path);
        let _permit = self
            .app
            .throttle
            .acquire(prepared.method.as_str(), pathname)
            .await;
        let builder = match parsed_url {
            Ok(url) => self.app.http.request(prepared.method.clone(), url),
            Err(_) => self
                .app
                .http
                .request(prepared.method.clone(), prepared.url.as_str()),
        };
        let mut builder = builder.header(ACCEPT, prepared.accept.as_ref());
        if let Some(installation_id) = self.installation_id {
            builder = builder.header(
                AUTHORIZATION,
                self.app.installation_authorization(installation_id).await?,
            );
        }
        if let Some(body) = &prepared.body {
            builder = builder
                .header(CONTENT_TYPE, "application/json; charset=utf-8")
                .body(js::stringify(body));
        }
        let response = builder
            .send()
            .await
            .map_err(|error| JsError::http(500, error.to_string(), None))?;
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        if let (Some(metrics), Some(installation_id)) = (&self.app.metrics, self.installation_id) {
            metrics.record_rate_limit(installation_id, &headers);
        }
        let is_json = header_text(&headers, "content-type")
            .is_some_and(|content_type| content_type.contains("application/json"));
        let text = response.text().await.unwrap_or_default();
        let data = if is_json {
            serde_json::from_str(&text).unwrap_or(Value::String(text))
        } else {
            Value::String(text)
        };
        if status == 304 {
            return Err(JsError::http(status, "Not modified", Some(data)));
        }
        if status >= 400 {
            return Err(JsError::http(
                status,
                octokit_error_message(&data),
                Some(data),
            ));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn octokit_error_message_appends_documentation_url_and_errors() {
        let data = json!({"message": "Validation Failed", "errors": [{"code": "invalid"}], "documentation_url": "https://docs"});
        assert_eq!(
            octokit_error_message(&data),
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
