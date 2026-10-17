use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::{Config, Provider};

/// Thin client over Anthropic Messages or any OpenAI-compatible chat endpoint.
#[derive(Clone)]
pub struct Llm {
    http: reqwest::Client,
    cfg: Config,
}

impl Llm {
    pub fn new(cfg: &Config) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .expect("http client");
        Self { http, cfg: cfg.clone() }
    }

    pub async fn text(&self, strong: bool, system: &str, user: &str, max_tokens: u32) -> Result<String> {
        let model = if strong { &self.cfg.strong_model } else { &self.cfg.fast_model };
        let (rb, anthropic) = match self.cfg.provider {
            Provider::Anthropic => {
                if self.cfg.llm_key.is_empty() {
                    bail!("missing Anthropic API key");
                }
                let rb = self
                    .http
                    .post("https://api.anthropic.com/v1/messages")
                    .header("x-api-key", &self.cfg.llm_key)
                    .header("anthropic-version", "2023-06-01")
                    .json(&json!({
                        "model": model, "max_tokens": max_tokens, "system": system,
                        "messages": [{"role": "user", "content": user}],
                    }));
                (rb, true)
            }
            Provider::OpenAi => {
                let url = format!("{}/chat/completions", self.cfg.llm_base.trim_end_matches('/'));
                let mut rb = self.http.post(url).json(&json!({
                    "model": model, "max_tokens": max_tokens,
                    "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}],
                }));
                if !self.cfg.llm_key.is_empty() {
                    rb = rb.bearer_auth(&self.cfg.llm_key);
                }
                (rb, false)
            }
        };

        let mut wait = 2;
        let v: Value = loop {
            let res = rb.try_clone().context("request not cloneable")?.send().await?;
            let status = res.status();
            // 429 rate limit, 529 overloaded: back off and retry.
            if matches!(status.as_u16(), 429 | 500 | 502 | 503 | 529) && wait <= 16 {
                tokio::time::sleep(Duration::from_secs(wait)).await;
                wait *= 2;
                continue;
            }
            if !status.is_success() {
                bail!("LLM {status}: {}", res.text().await.unwrap_or_default());
            }
            break res.json().await?;
        };

        let out = if anthropic {
            v["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|b| b["text"].as_str())
                .collect::<String>()
        } else {
            v["choices"][0]["message"]["content"].as_str().unwrap_or_default().to_string()
        };
        if out.trim().is_empty() {
            bail!("LLM returned no text");
        }
        Ok(out)
    }

    pub async fn json<T: DeserializeOwned>(&self, strong: bool, system: &str, user: &str, max_tokens: u32) -> Result<T> {
        let system = format!("{system}\n\nRespond with ONLY one JSON object. No prose, no code fences.");
        let raw = self.text(strong, &system, user, max_tokens).await?;
        parse_json(&raw)
    }
}

/// Models sometimes wrap JSON in prose or fences; take the outermost object.
pub fn parse_json<T: DeserializeOwned>(raw: &str) -> Result<T> {
    let (Some(s), Some(e)) = (raw.find('{'), raw.rfind('}')) else {
        bail!("no JSON object in model output: {}", raw.chars().take(200).collect::<String>());
    };
    serde_json::from_str(&raw[s..=e]).with_context(|| format!("bad JSON: {}", raw.chars().take(200).collect::<String>()))
}
