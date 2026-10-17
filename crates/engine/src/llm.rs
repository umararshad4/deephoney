use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use base64::Engine;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::{Config, Provider};

/// Thin client over Anthropic Messages or any OpenAI-compatible chat endpoint.
#[derive(Clone)]
pub struct Llm {
    http: reqwest::Client,
    cfg: Config,
    usage: Arc<Mutex<Usage>>,
}

impl Llm {
    pub fn new(cfg: &Config) -> Self {
        let http = reqwest::Client::builder().timeout(Duration::from_secs(300)).build().expect("http client");
        Self { http, cfg: cfg.clone(), usage: Arc::new(Mutex::new(Usage::default())) }
    }

    pub fn verifier(&self) -> Result<(Self, bool)> {
        let (cfg, separate) = verification_config(&self.cfg)?;
        Ok((Self { http: self.http.clone(), cfg, usage: self.usage.clone() }, separate))
    }

    pub fn model(&self) -> &str {
        &self.cfg.strong_model
    }

    pub fn usage(&self) -> Usage {
        self.usage.lock().unwrap().clone()
    }

    pub async fn text(&self, strong: bool, system: &str, user: &str, max_tokens: u32) -> Result<String> {
        self.message(strong, system, json!(user), json!(user), max_tokens).await
    }

    /// Visual reading uses the configured strong model and shares the run usage ledger.
    pub async fn image_json<T: DeserializeOwned>(&self, system: &str, user: &str, png: &str, max_tokens: u32) -> Result<T> {
        if png.len() > 2_796_204 {
            bail!("visual document image exceeds 2 MiB");
        }
        let decoded = base64::engine::general_purpose::STANDARD.decode(png).context("invalid visual image base64")?;
        if decoded.len() > 2 * 1024 * 1024 || !decoded.starts_with(b"\x89PNG\r\n\x1a\n") {
            bail!("visual document requires a PNG image no larger than 2 MiB");
        }
        let system = format!("{system}\nReturn ONLY one JSON object.");
        let anthropic = json!([
            {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": png}},
            {"type": "text", "text": user}
        ]);
        let openai = json!([
            {"type": "text", "text": user},
            {"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{png}")}}
        ]);
        parse_json(&self.message(true, &system, anthropic, openai, max_tokens).await?)
    }

    async fn message(
        &self,
        strong: bool,
        system: &str,
        anthropic_content: Value,
        openai_content: Value,
        max_tokens: u32,
    ) -> Result<String> {
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
                        "messages": [{"role": "user", "content": anthropic_content}],
                    }));
                (rb, true)
            }
            Provider::OpenAi => {
                let url = format!("{}/chat/completions", self.cfg.llm_base.trim_end_matches('/'));
                let mut rb = self.http.post(url).json(&json!({
                    "model": model, "max_tokens": max_tokens,
                    "messages": [{"role": "system", "content": system}, {"role": "user", "content": openai_content}],
                }));
                if !self.cfg.llm_key.is_empty() {
                    rb = rb.bearer_auth(&self.cfg.llm_key);
                }
                (rb, false)
            }
        };

        let mut wait = 2;
        let v: Value = loop {
            self.usage.lock().unwrap().requests += 1;
            let res = rb.try_clone().context("request not cloneable")?.send().await?;
            let status = res.status();
            // 429 rate limit, 529 overloaded: back off and retry.
            if matches!(status.as_u16(), 429 | 500 | 502 | 503 | 529) && wait <= 16 {
                tokio::time::sleep(Duration::from_secs(wait)).await;
                wait *= 2;
                continue;
            }
            if !status.is_success() {
                let body = response_bytes(res, 8192).await.context(format!("LLM {status}"))?;
                bail!("LLM {status}: {}", String::from_utf8_lossy(&body));
            }
            let body = response_bytes(res, 2 * 1024 * 1024).await?;
            break serde_json::from_slice(&body)?;
        };

        self.usage.lock().unwrap().record(&v["usage"], anthropic);
        let out = if anthropic {
            v["content"].as_array().into_iter().flatten().filter_map(|b| b["text"].as_str()).collect::<String>()
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

async fn response_bytes(mut response: reqwest::Response, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len().saturating_add(chunk.len()) > limit {
            bail!("LLM response exceeded its {limit}-byte limit");
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

/// Models sometimes wrap JSON in prose or fences; take the outermost object.
pub fn parse_json<T: DeserializeOwned>(raw: &str) -> Result<T> {
    let (Some(s), Some(e)) = (raw.find('{'), raw.rfind('}')) else {
        bail!("no JSON object in model output: {}", raw.chars().take(200).collect::<String>());
    };
    serde_json::from_str(&raw[s..=e]).with_context(|| format!("bad JSON: {}", raw.chars().take(200).collect::<String>()))
}

#[derive(Clone, Debug, Default)]
pub struct Usage {
    pub requests: usize,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

impl Usage {
    fn record(&mut self, usage: &Value, anthropic: bool) {
        let input = usage[if anthropic { "input_tokens" } else { "prompt_tokens" }].as_u64();
        let output = usage[if anthropic { "output_tokens" } else { "completion_tokens" }].as_u64();
        if let Some(n) = input {
            self.input_tokens = Some(self.input_tokens.unwrap_or(0).saturating_add(n));
        }
        if let Some(n) = output {
            self.output_tokens = Some(self.output_tokens.unwrap_or(0).saturating_add(n));
        }
    }
}

/// A different endpoint must receive its own credential, never an inherited writer key.
pub fn verification_config(cfg: &Config) -> Result<(Config, bool)> {
    if cfg.verifier_model.trim().is_empty() {
        return Ok((cfg.clone(), false));
    }
    let mut out = cfg.clone();
    out.provider = cfg.verifier_provider;
    out.strong_model = cfg.verifier_model.trim().into();
    out.fast_model = out.strong_model.clone();
    let same_provider = cfg.provider == out.provider;
    let same_endpoint = match out.provider {
        Provider::Anthropic => same_provider,
        Provider::OpenAi => {
            out.llm_base = if cfg.verifier_base.trim().is_empty() && same_provider {
                cfg.llm_base.trim().to_string()
            } else {
                cfg.verifier_base.trim().to_string()
            };
            let url = url::Url::parse(&out.llm_base).context("configure a valid verifier base URL")?;
            if !matches!(url.scheme(), "http" | "https")
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
                || url.host_str().is_none()
                || url.host_str().is_some_and(|h| h.trim_end_matches('.').ends_with(".onion"))
            {
                bail!("verifier base URL must be HTTP(S), without credentials, query, fragment, or onion host");
            }
            let normalized = |raw: &str| url::Url::parse(raw).ok().map(|u| u.as_str().trim_end_matches('/').to_string());
            same_provider && normalized(&out.llm_base) == normalized(&cfg.llm_base)
        }
    };
    out.llm_key = if !cfg.verifier_key.is_empty() {
        cfg.verifier_key.clone()
    } else if same_endpoint {
        cfg.llm_key.clone()
    } else {
        String::new()
    };
    if out.provider == Provider::Anthropic && out.llm_key.is_empty() {
        bail!("configure an Anthropic verifier key");
    }
    let separate = !same_endpoint || out.strong_model != cfg.strong_model;
    Ok((out, separate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn model_response_body_limit_stops_oversized_json_before_parse() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            assert!(stream.read(&mut request).await.unwrap() > 0);
            let body = vec![b'x'; 2 * 1024 * 1024 + 1];
            let header = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            stream.write_all(header.as_bytes()).await.unwrap();
            // Client intentionally closes on its read limit; a broken pipe is expected.
            let _ = stream.write_all(&body).await;
        });
        let llm = Llm::new(&Config { provider: Provider::OpenAi, llm_base: format!("http://{address}"), ..Default::default() });
        let error = llm.text(false, "test", "test", 100).await.unwrap_err();
        assert!(error.to_string().contains("byte limit"), "{error:#}");
        assert_eq!(llm.usage().requests, 1);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn image_payload_rejects_non_png_and_oversize_before_network() {
        let llm = Llm::new(&Config::default());
        assert!(llm.image_json::<Value>("a", "b", "bm90IGEgcG5n", 100).await.is_err());
        assert!(llm.image_json::<Value>("a", "b", &"a".repeat(2_796_205), 100).await.is_err());
        assert_eq!(llm.usage().requests, 0);
    }

    #[test]
    fn verifier_never_inherits_writer_secret_on_another_endpoint() {
        let cfg = Config {
            provider: Provider::OpenAi,
            llm_base: "https://writer.test/v1".into(),
            llm_key: "writer-secret".into(),
            verifier_model: "checker".into(),
            verifier_base: "https://checker.test/v1".into(),
            ..Default::default()
        };
        let (check, separate) = verification_config(&cfg).unwrap();
        assert!(check.llm_key.is_empty());
        assert!(separate);
        let (check, _) = verification_config(&Config { verifier_key: "checker-secret".into(), ..cfg }).unwrap();
        assert_eq!(check.llm_key, "checker-secret");
    }

    #[test]
    fn verifier_can_reuse_only_the_same_provider_endpoint() {
        let cfg = Config {
            provider: Provider::OpenAi,
            llm_base: "https://model.test/v1".into(),
            llm_key: "writer-secret".into(),
            verifier_model: "checker".into(),
            verifier_base: "https://model.test/v1/".into(),
            ..Default::default()
        };
        let (check, separate) = verification_config(&cfg).unwrap();
        assert_eq!(check.llm_key, "writer-secret");
        assert!(separate);
        let (_, separate) = verification_config(&Config { verifier_model: cfg.strong_model.clone(), ..cfg }).unwrap();
        assert!(!separate);
    }

    #[test]
    fn verifier_rejects_unsafe_configuration_before_requests() {
        for endpoint in [
            "",
            "file:///a",
            "https://user:pass@model.test",
            "https://hidden.onion./v1",
            "https://model.test?secret=a",
            "https://model.test/#key",
        ] {
            let cfg = Config { verifier_model: "checker".into(), verifier_base: endpoint.into(), ..Default::default() };
            assert!(verification_config(&cfg).is_err(), "{endpoint}");
        }
        let cfg = Config {
            provider: Provider::OpenAi,
            verifier_provider: Provider::Anthropic,
            llm_key: "writer-secret".into(),
            verifier_model: "checker".into(),
            ..Default::default()
        };
        assert!(verification_config(&cfg).is_err());
    }

    #[test]
    fn usage_does_not_invent_zero_tokens_when_provider_omits_them() {
        let mut usage = Usage::default();
        usage.record(&json!({}), false);
        assert_eq!(usage.input_tokens, None);
        usage.record(&json!({"prompt_tokens":10,"completion_tokens":4}), false);
        usage.record(&json!({"input_tokens":3,"output_tokens":2}), true);
        assert_eq!((usage.input_tokens, usage.output_tokens), (Some(13), Some(6)));
    }
}
