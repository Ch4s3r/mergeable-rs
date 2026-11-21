use crate::app_process::WEBHOOK_SECRET;
use anyhow::Result;
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use std::time::Duration;

pub struct Webhook {
    pub event: String,
    pub delivery_id: String,
    pub body: Vec<u8>,
}

#[derive(Clone)]
pub struct WebhookSender {
    client: reqwest::Client,
}

pub fn signature_header(secret: &str, body: &[u8]) -> Result<String> {
    let mut signer = Hmac::<Sha256>::new_from_slice(secret.as_bytes())?;
    signer.update(body);
    Ok(format!(
        "sha256={}",
        hex::encode(signer.finalize().into_bytes())
    ))
}

impl WebhookSender {
    pub fn new() -> Result<WebhookSender> {
        Ok(WebhookSender {
            client: reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(60))
                .build()?,
        })
    }

    pub async fn send(&self, port: u16, webhook_path: &str, webhook: &Webhook) -> Result<u16> {
        let response = self
            .client
            .post(format!("http://127.0.0.1:{port}{webhook_path}"))
            .header("content-type", "application/json")
            .header("x-github-event", &webhook.event)
            .header("x-github-delivery", &webhook.delivery_id)
            .header(
                "x-hub-signature-256",
                signature_header(WEBHOOK_SECRET, &webhook.body)?,
            )
            .body(webhook.body.clone())
            .send()
            .await?;
        let status = response.status().as_u16();
        response.bytes().await?;
        Ok(status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_matches_github_documentation_example() -> Result<()> {
        assert_eq!(
            signature_header("It's a Secret to Everybody", b"Hello, World!")?,
            "sha256=757107ea0eb2509fc211221cce984b8a37570b6d7586c22c46f4379c8b043e17"
        );
        Ok(())
    }
}
