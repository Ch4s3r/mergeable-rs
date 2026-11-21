use crate::error::{JsError, JsResult};
use crate::github::client::encode_uri_component;
use crate::js;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_ACCEPT: &str = "application/vnd.github.v3+json";
const JWT_BACKDATE_SECONDS: u64 = 30;
const JWT_LIFETIME_SECONDS: u64 = 600;

pub struct AppJwtClient {
    http: reqwest::Client,
    base_url: String,
    app_id: String,
    signing_key: EncodingKey,
}

pub fn octokit_error_message(data: &Value) -> String {
    match data {
        Value::String(text) => text.clone(),
        Value::Object(map) => {
            let documentation_suffix = map
                .get("documentation_url")
                .map(|url| format!(" - {}", js::to_js_string(Some(url))))
                .unwrap_or_default();
            match map.get("message") {
                Some(message) => match map.get("errors") {
                    Some(Value::Array(errors)) => format!(
                        "{}: {}{documentation_suffix}",
                        js::to_js_string(Some(message)),
                        errors
                            .iter()
                            .map(js::stringify)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    _ => format!("{}{documentation_suffix}", js::to_js_string(Some(message))),
                },
                None => format!("Unknown error: {}", js::stringify(data)),
            }
        }
        other => format!("Unknown error: {}", js::stringify(other)),
    }
}

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

pub fn items_of_page(data: Value) -> Vec<Value> {
    match data {
        Value::Array(items) => items,
        Value::Object(map) if map.contains_key("total_count") && !map.contains_key("url") => {
            match map.into_iter().find(|(key, _)| {
                !matches!(
                    key.as_str(),
                    "incomplete_results" | "repository_selection" | "total_count"
                )
            }) {
                Some((_, Value::Array(items))) => items,
                Some((_, other)) => vec![other],
                None => Vec::new(),
            }
        }
        other => vec![other],
    }
}

struct PageResponse {
    data: Value,
    next_url: Option<String>,
}

impl AppJwtClient {
    pub fn new(base_url: &str, app_id: &str, private_key_pem: &str) -> JsResult<Self> {
        let signing_key = EncodingKey::from_rsa_pem(private_key_pem.as_bytes())
            .map_err(|error| JsError::error(format!("invalid PRIVATE_KEY: {error}")))?;
        let http = reqwest::Client::builder()
            .user_agent(concat!("mergeable-rust/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| JsError::error(error.to_string()))?;
        Ok(AppJwtClient {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            app_id: app_id.to_string(),
            signing_key,
        })
    }

    fn app_jwt(&self) -> JsResult<String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or_default();
        let issued_at = now.saturating_sub(JWT_BACKDATE_SECONDS);
        let claims = json!({
            "iat": issued_at,
            "exp": issued_at + JWT_LIFETIME_SECONDS,
            "iss": self.app_id,
        });
        jsonwebtoken::encode(&Header::new(Algorithm::RS256), &claims, &self.signing_key)
            .map_err(|error| JsError::error(format!("could not sign app JWT: {error}")))
    }

    async fn send(&self, request: reqwest::RequestBuilder) -> JsResult<PageResponse> {
        let response = request
            .header("accept", DEFAULT_ACCEPT)
            .send()
            .await
            .map_err(|error| JsError::http(500, error.to_string(), None))?;
        let status = response.status().as_u16();
        let next_url = response
            .headers()
            .get("link")
            .and_then(|link| link.to_str().ok())
            .and_then(next_page_url);
        let data: Value = response.json().await.unwrap_or(Value::Null);
        if status >= 400 {
            return Err(JsError::http(
                status,
                octokit_error_message(&data),
                Some(data),
            ));
        }
        Ok(PageResponse { data, next_url })
    }

    pub async fn paginate_as_app(&self, path_and_query: &str) -> JsResult<Vec<Value>> {
        let mut collected = Vec::new();
        let mut url = format!("{}{path_and_query}", self.base_url);
        loop {
            let request = self
                .http
                .get(&url)
                .header("authorization", format!("bearer {}", self.app_jwt()?));
            let page = self.send(request).await?;
            collected.extend(items_of_page(page.data));
            match page.next_url {
                Some(next_url) => url = next_url,
                None => return Ok(collected),
            }
        }
    }

    pub async fn installation_token(&self, installation_id: &str) -> JsResult<String> {
        let url = format!(
            "{}/app/installations/{}/access_tokens",
            self.base_url,
            encode_uri_component(installation_id)
        );
        let request = self
            .http
            .post(&url)
            .header("authorization", format!("bearer {}", self.app_jwt()?));
        let page = self.send(request).await?;
        Ok(js::to_js_string(page.data.get("token")))
    }

    pub async fn rate_limit_as_installation(&self, installation_id: &str) -> JsResult<Value> {
        let token = self.installation_token(installation_id).await?;
        let request = self
            .http
            .get(format!("{}/rate_limit", self.base_url))
            .header("authorization", format!("token {token}"));
        Ok(self.send(request).await?.data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_page_is_taken_from_the_link_header() {
        let link = "<https://api.github.com/app/installations?page=2&per_page=1>; rel=\"next\", <https://api.github.com/app/installations?page=5&per_page=1>; rel=\"last\"";
        assert_eq!(
            next_page_url(link).as_deref(),
            Some("https://api.github.com/app/installations?page=2&per_page=1")
        );
        assert_eq!(next_page_url("<https://x/?page=1>; rel=\"prev\""), None);
    }

    #[test]
    fn pages_are_flattened_like_octokit_paginate() {
        let page =
            json!({"total_count": 1, "repository_selection": "all", "repositories": [{"id": 2}]});
        assert_eq!(items_of_page(page), vec![json!({"id": 2})]);
        assert_eq!(items_of_page(json!([{"id": 1}])), vec![json!({"id": 1})]);
        assert_eq!(items_of_page(json!({"id": 1})), vec![json!({"id": 1})]);
    }

    #[test]
    fn octokit_error_message_appends_the_documentation_url() {
        let data =
            json!({"message": "Not Found", "documentation_url": "https://docs.github.com/rest"});
        assert_eq!(
            octokit_error_message(&data),
            "Not Found - https://docs.github.com/rest"
        );
    }
}
