use crate::error::{Error, Result};
use crate::github::app_auth::{AppSigner, build_http_client};
use crate::github::throttle::RequestThrottle;
use crate::json_value;
use crate::metrics::Metrics;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use reqwest::Method;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde_json::{Map, Value};
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Arc, PoisonError};
use std::time::{Duration, SystemTime};

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
const INSTALLATION_TOKEN_FALLBACK_LIFETIME: Duration = Duration::from_mins(50);
const NOT_MODIFIED: u16 = 304;
const CONFLICT: u16 = 409;
pub(crate) const DEFAULT_ACCEPT: &str = "application/vnd.github.v3+json";
const RAW_ACCEPT: &str = "application/vnd.github.v3.raw";

#[must_use]
pub fn encode_uri_component(text: &str) -> String {
    utf8_percent_encode(text, URI_COMPONENT_SAFE).to_string()
}

fn push_uri_component(target: &mut String, text: &str) {
    target.extend(utf8_percent_encode(text, URI_COMPONENT_SAFE));
}

fn display_text(value: &Value) -> Cow<'_, str> {
    match value {
        Value::String(text) => Cow::Borrowed(text),
        other => Cow::Owned(json_value::to_display_string(Some(other))),
    }
}

#[must_use]
pub fn header_text<'headers>(headers: &'headers HeaderMap, name: &str) -> Option<&'headers str> {
    headers
        .get_all(name)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .next_back()
}

#[must_use]
pub fn api_error_message(data: &Value) -> String {
    match data {
        Value::String(text) => text.clone(),
        Value::Object(map) => {
            let documentation_suffix = map
                .get("documentation_url")
                .map(|url| format!(" - {}", json_value::to_display_string(Some(url))))
                .unwrap_or_default();
            match map.get("message") {
                Some(message) => match map.get("errors") {
                    Some(Value::Array(errors)) => format!(
                        "{}: {}{documentation_suffix}",
                        json_value::to_display_string(Some(message)),
                        errors
                            .iter()
                            .map(json_value::stringify)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    _ => format!(
                        "{}{documentation_suffix}",
                        json_value::to_display_string(Some(message))
                    ),
                },
                None => format!("Unknown error: {}", json_value::stringify(data)),
            }
        }
        other => format!("Unknown error: {}", json_value::stringify(other)),
    }
}

#[must_use]
pub fn next_page_url(link_header: &str) -> Option<String> {
    link_header.split(',').find_map(|part| {
        let (target, relation) = part.split_once(';')?;
        relation.contains("rel=\"next\"").then(|| {
            target
                .trim()
                .trim_start_matches('<')
                .trim_end_matches('>')
                .to_string()
        })
    })
}

fn transport_error(error: &reqwest::Error) -> Error {
    Error::http(500, error.to_string())
}

#[derive(Debug, Clone)]
pub struct GithubResponse {
    pub status: u16,
    pub headers: HeaderMap,
    pub data: Value,
}

impl GithubResponse {
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        header_text(&self.headers, name)
    }
}

struct CachedInstallationToken {
    authorization: HeaderValue,
    refresh_after: SystemTime,
}

type InstallationTokenSlot = Arc<tokio::sync::Mutex<Option<CachedInstallationToken>>>;

pub struct GithubAppSettings {
    pub base_url: String,
    pub app_id: String,
    pub private_key_pem: String,
    pub retry_base_delay: Duration,
    pub throttle: Option<RequestThrottle>,
}

pub struct GithubApp {
    http: reqwest::Client,
    base_url: String,
    base_path: String,
    signer: AppSigner,
    installation_tokens: std::sync::Mutex<HashMap<u64, InstallationTokenSlot>>,
    retry_base_delay: Duration,
    throttle: Option<RequestThrottle>,
    metrics: Option<Arc<Metrics>>,
}

impl GithubApp {
    pub fn new(settings: GithubAppSettings, metrics: Option<Arc<Metrics>>) -> Result<Self> {
        let issuer = settings
            .app_id
            .parse::<u64>()
            .map_or_else(|_| Value::String(settings.app_id), Value::from);
        let signer = AppSigner::new(issuer, &settings.private_key_pem)?;
        let base_url = settings.base_url.trim_end_matches('/').to_string();
        let base_path = reqwest::Url::parse(&base_url)
            .map(|url| url.path().trim_end_matches('/').to_string())
            .unwrap_or_default();
        Ok(GithubApp {
            http: build_http_client()?,
            base_url,
            base_path,
            signer,
            installation_tokens: std::sync::Mutex::new(HashMap::new()),
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

    fn installation_token_slot(&self, installation_id: u64) -> InstallationTokenSlot {
        let mut slots = self
            .installation_tokens
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        Arc::clone(slots.entry(installation_id).or_default())
    }

    async fn installation_authorization(&self, installation_id: u64) -> Result<HeaderValue> {
        let slot = self.installation_token_slot(installation_id);
        let mut cached = slot.lock().await;
        if let Some(token) = cached.as_ref()
            && SystemTime::now() < token.refresh_after
        {
            return Ok(token.authorization.clone());
        }
        let fresh = self.create_installation_token(installation_id).await?;
        let authorization = fresh.authorization.clone();
        *cached = Some(fresh);
        Ok(authorization)
    }

    async fn create_installation_token(
        &self,
        installation_id: u64,
    ) -> Result<CachedInstallationToken> {
        let url = format!(
            "{}/app/installations/{installation_id}/access_tokens",
            self.base_url
        );
        let response = self
            .http
            .post(&url)
            .header(AUTHORIZATION, self.signer.bearer_authorization()?)
            .header(ACCEPT, DEFAULT_ACCEPT)
            .send()
            .await
            .map_err(|error| transport_error(&error))?;
        let status = response.status().as_u16();
        let data: Value = response.json().await.unwrap_or(Value::Null);
        if status >= 400 {
            return Err(Error::http(status, api_error_message(&data)));
        }
        let token = json_value::as_str(data.get("token"))
            .ok_or_else(|| Error::general("installation token missing in response"))?;
        let authorization = HeaderValue::try_from(format!("token {token}"))
            .map_err(|error| Error::http(500, error.to_string()))?;
        let refresh_after = json_value::as_str(data.get("expires_at"))
            .and_then(|expires| chrono::DateTime::parse_from_rfc3339(expires).ok())
            .map_or_else(
                || SystemTime::now() + INSTALLATION_TOKEN_FALLBACK_LIFETIME,
                |expires| SystemTime::from(expires) - INSTALLATION_TOKEN_REFRESH_MARGIN,
            );
        Ok(CachedInstallationToken {
            authorization,
            refresh_after,
        })
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

impl InstallationClient {
    fn prepare(&self, route: &str, params: Value) -> Result<PreparedRequest> {
        let (method_name, path_template) = route
            .split_once(' ')
            .ok_or_else(|| Error::general(format!("invalid route {route}")))?;
        let method = Method::from_bytes(method_name.as_bytes())
            .map_err(|error| Error::general(error.to_string()))?;
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
                .ok_or_else(|| Error::general(format!("invalid route {route}")))?;
            let parameter_name = &rest[start + 1..end];
            if let Some(present) = remaining
                .shift_remove(parameter_name)
                .filter(|present| !present.is_null())
            {
                push_uri_component(&mut url, &display_text(&present));
            }
            rest = &rest[end + 1..];
        }
        url.push_str(rest);
        let body = if method == Method::GET || method == Method::HEAD {
            let mut separator = if url.contains('?') { '&' } else { '?' };
            for (name, value) in &remaining {
                url.push(separator);
                separator = '&';
                let text = display_text(value);
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

    pub fn endpoint_url(&self, route: &str, params: &Value) -> Result<String> {
        Ok(self.prepare(route, params.clone())?.url)
    }

    pub async fn request(&self, route: &str, params: Value) -> Result<GithubResponse> {
        let prepared = self.prepare(route, params)?;
        self.send_with_retries(&prepared).await
    }

    fn explain_missing_authentication(&self, prepared: &PreparedRequest, error: Error) -> Error {
        let Some(reason) = &self.unauthenticated_reason else {
            return error;
        };
        let explanation = match error.status() {
            Some(404) => {
                format!("Not found. May be due to lack of authentication. Reason: {reason}")
            }
            Some(401) => format!(
                "Unauthorized. \"{} {}\" failed most likely due to lack of authentication. Reason: {reason}",
                prepared.method, prepared.url
            ),
            Some(status) if (400..500).contains(&status) => {
                let trimmed = error.message().strip_suffix('.').unwrap_or(error.message());
                format!("{trimmed}. May be caused by lack of authentication ({reason}).")
            }
            _ => return error,
        };
        error.with_message(explanation)
    }

    pub async fn request_raw(&self, route: &str, params: Value) -> Result<GithubResponse> {
        let mut prepared = self.prepare(route, params)?;
        prepared.accept = Cow::Borrowed(RAW_ACCEPT);
        self.send_with_retries(&prepared).await
    }

    async fn request_url(&self, url: &str) -> Result<GithubResponse> {
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

    pub async fn paginate(&self, route: &str, params: Value) -> Result<Vec<Value>> {
        let mut collected = Vec::new();
        let mut response = match self.request(route, params).await {
            Err(error) if error.has_status(CONFLICT) => return Ok(collected),
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
                Err(error) if error.has_status(CONFLICT) => return Ok(collected),
                other => other?,
            };
        }
    }

    async fn send_with_retries(&self, prepared: &PreparedRequest) -> Result<GithubResponse> {
        self.send_with_retries_unexplained(prepared)
            .await
            .map_err(|error| self.explain_missing_authentication(prepared, error))
    }

    async fn send_with_retries_unexplained(
        &self,
        prepared: &PreparedRequest,
    ) -> Result<GithubResponse> {
        let mut retry_count: u32 = 0;
        loop {
            match self.send_once(prepared).await {
                Err(error)
                    if retry_count < RETRY_ATTEMPTS
                        && error.status().is_some_and(|status| {
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

    async fn send_once(&self, prepared: &PreparedRequest) -> Result<GithubResponse> {
        let parsed_url = reqwest::Url::parse(&prepared.url);
        let full_path = parsed_url.as_ref().map_or("", reqwest::Url::path);
        let pathname = full_path
            .strip_prefix(self.app.base_path.as_str())
            .filter(|rest| rest.starts_with('/'))
            .unwrap_or(full_path);
        let _concurrency_permit = match &self.app.throttle {
            Some(throttle) => throttle.acquire(&prepared.method, pathname).await,
            None => None,
        };
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
                .body(json_value::stringify(body));
        }
        let response = builder
            .send()
            .await
            .map_err(|error| transport_error(&error))?;
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
        if status == NOT_MODIFIED {
            return Err(Error::http(status, "Not modified"));
        }
        if status >= 400 {
            return Err(Error::http(status, api_error_message(&data)));
        }
        Ok(GithubResponse {
            status,
            headers,
            data,
        })
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
                    json_value::to_display_string(Some(preview))
                )
            })
            .collect::<Vec<_>>()
            .join(","),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn api_error_message_appends_documentation_url_and_errors() {
        let data = json!({"message": "Validation Failed", "errors": [{"code": "invalid"}], "documentation_url": "https://docs"});
        assert_eq!(
            api_error_message(&data),
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
    fn api_error_message_without_errors_appends_only_the_documentation_url() {
        let data =
            json!({"message": "Not Found", "documentation_url": "https://docs.github.com/rest"});
        assert_eq!(
            api_error_message(&data),
            "Not Found - https://docs.github.com/rest"
        );
    }

    #[test]
    fn link_header_without_a_next_relation_has_no_next_page() {
        assert_eq!(next_page_url("<https://x/?page=1>; rel=\"prev\""), None);
    }

    #[test]
    fn uri_component_encoding_escapes_reserved_characters() {
        assert_eq!(
            encode_uri_component(".github/mergeable.yml"),
            ".github%2Fmergeable.yml"
        );
        assert_eq!(encode_uri_component("a b+c"), "a%20b%2Bc");
    }
}
