use crate::app_config::OutgoingHttpTimeouts;
use crate::error::{Error, Result};
use crate::github::client::{
    DEFAULT_ACCEPT, api_error_message, encode_uri_component, next_page_url,
};
use crate::json_value;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use reqwest::header::{ACCEPT, AUTHORIZATION, LINK};
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};

const JWT_BACKDATE_SECONDS: u64 = 30;
const JWT_LIFETIME_SECONDS: u64 = 600;
const USER_AGENT: &str = concat!("mergeable-rust/", env!("CARGO_PKG_VERSION"));

pub(crate) fn build_http_client(timeouts: OutgoingHttpTimeouts) -> Result<reqwest::Client> {
    timeouts
        .apply_to(reqwest::Client::builder())
        .user_agent(USER_AGENT)
        .build()
        .map_err(|error| Error::general(format!("cannot build the HTTP client: {error}")))
}

pub(crate) struct AppSigner {
    issuer: Value,
    signing_key: EncodingKey,
}

impl AppSigner {
    pub(crate) fn new(issuer: Value, private_key_pem: &str) -> Result<Self> {
        let signing_key = EncodingKey::from_rsa_pem(private_key_pem.as_bytes())
            .map_err(|error| Error::general(format!("invalid PRIVATE_KEY: {error}")))?;
        Ok(AppSigner {
            issuer,
            signing_key,
        })
    }

    fn sign(&self) -> Result<String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or_default();
        let issued_at = now.saturating_sub(JWT_BACKDATE_SECONDS);
        let claims = json!({
            "iat": issued_at,
            "exp": issued_at + JWT_LIFETIME_SECONDS,
            "iss": self.issuer,
        });
        jsonwebtoken::encode(&Header::new(Algorithm::RS256), &claims, &self.signing_key)
            .map_err(|error| Error::general(format!("could not sign app JWT: {error}")))
    }

    pub(crate) fn bearer_authorization(&self) -> Result<String> {
        Ok(format!("bearer {}", self.sign()?))
    }
}

pub struct AppJwtClient {
    http: reqwest::Client,
    base_url: String,
    signer: AppSigner,
}

#[must_use]
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
    pub fn new(
        base_url: &str,
        app_id: &str,
        private_key_pem: &str,
        timeouts: OutgoingHttpTimeouts,
    ) -> Result<Self> {
        Ok(AppJwtClient {
            http: build_http_client(timeouts)?,
            base_url: base_url.trim_end_matches('/').to_string(),
            signer: AppSigner::new(Value::String(app_id.to_string()), private_key_pem)?,
        })
    }

    async fn send(&self, request: reqwest::RequestBuilder) -> Result<PageResponse> {
        let response = request
            .header(ACCEPT, DEFAULT_ACCEPT)
            .send()
            .await
            .map_err(|error| Error::http(500, error.to_string()))?;
        let status = response.status().as_u16();
        let next_url = response
            .headers()
            .get(LINK)
            .and_then(|link| link.to_str().ok())
            .and_then(next_page_url);
        let data: Value = response.json().await.unwrap_or(Value::Null);
        if status >= 400 {
            return Err(Error::http(status, api_error_message(&data)));
        }
        Ok(PageResponse { data, next_url })
    }

    pub async fn paginate_as_app(&self, path_and_query: &str) -> Result<Vec<Value>> {
        let mut collected = Vec::new();
        let mut url = format!("{}{path_and_query}", self.base_url);
        loop {
            let request = self
                .http
                .get(&url)
                .header(AUTHORIZATION, self.signer.bearer_authorization()?);
            let page = self.send(request).await?;
            collected.extend(items_of_page(page.data));
            match page.next_url {
                Some(next_url) => url = next_url,
                None => return Ok(collected),
            }
        }
    }

    pub async fn installation_token(&self, installation_id: &str) -> Result<String> {
        let url = format!(
            "{}/app/installations/{}/access_tokens",
            self.base_url,
            encode_uri_component(installation_id)
        );
        let request = self
            .http
            .post(&url)
            .header(AUTHORIZATION, self.signer.bearer_authorization()?);
        let page = self.send(request).await?;
        Ok(json_value::to_display_string(page.data.get("token")))
    }

    pub async fn rate_limit_as_installation(&self, installation_id: &str) -> Result<Value> {
        let token = self.installation_token(installation_id).await?;
        let request = self
            .http
            .get(format!("{}/rate_limit", self.base_url))
            .header(AUTHORIZATION, format!("token {token}"));
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
    }

    #[test]
    fn pages_are_flattened_into_one_list() {
        let page =
            json!({"total_count": 1, "repository_selection": "all", "repositories": [{"id": 2}]});
        assert_eq!(items_of_page(page), vec![json!({"id": 2})]);
        assert_eq!(items_of_page(json!([{"id": 1}])), vec![json!({"id": 1})]);
        assert_eq!(items_of_page(json!({"id": 1})), vec![json!({"id": 1})]);
    }
}
