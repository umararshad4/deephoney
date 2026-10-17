//! Config on disk; secrets in the macOS Keychain, never in the JSON file.
use std::{fs, path::PathBuf};

use anyhow::{Result, bail};
use engine::Config;

const SERVICE: &str = "DeepHoney";

pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub secret: bool,
}

pub const FIELDS: &[Field] = &[
    Field { key: "llm_key", label: "LLM API key (Anthropic, or OpenAI-compatible)", secret: true },
    Field { key: "llm_base", label: "OpenAI-compatible base URL (Ollama / LM Studio / OpenRouter)", secret: false },
    Field { key: "strong_model", label: "Planner / writer model", secret: false },
    Field { key: "fast_model", label: "Reader model", secret: false },
    Field { key: "brave_key", label: "Brave Search API key", secret: true },
    Field { key: "exa_key", label: "Exa API key", secret: true },
    Field { key: "tavily_key", label: "Tavily API key", secret: true },
    Field { key: "searxng_url", label: "SearXNG URL (self-hosted)", secret: false },
    Field { key: "github_token", label: "GitHub token (code search)", secret: true },
    Field { key: "courtlistener_token", label: "CourtListener token", secret: true },
    Field { key: "contact", label: "Contact for User-Agent (SEC EDGAR asks for one)", secret: false },
];

pub fn slot<'a>(c: &'a mut Config, key: &str) -> &'a mut String {
    match key {
        "llm_key" => &mut c.llm_key,
        "llm_base" => &mut c.llm_base,
        "strong_model" => &mut c.strong_model,
        "fast_model" => &mut c.fast_model,
        "brave_key" => &mut c.brave_key,
        "exa_key" => &mut c.exa_key,
        "tavily_key" => &mut c.tavily_key,
        "searxng_url" => &mut c.searxng_url,
        "github_token" => &mut c.github_token,
        "courtlistener_token" => &mut c.courtlistener_token,
        "contact" => &mut c.contact,
        _ => unreachable!("unknown field {key}"),
    }
}

pub fn dir() -> PathBuf {
    let d = std::env::var("DEEPHONEY_DATA_DIR")
        .ok()
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("Library/Application Support/DeepHoney"));
    let _ = fs::create_dir_all(&d);
    d
}

fn service() -> String {
    std::env::var("DEEPHONEY_DATA_DIR").ok().filter(|p| !p.is_empty()).map(|p| format!("{SERVICE}:{p}")).unwrap_or_else(|| SERVICE.into())
}

pub fn load() -> Config {
    let mut c: Config = fs::read_to_string(dir().join("config.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    for f in FIELDS.iter().filter(|f| f.secret) {
        if let Ok(v) = keyring::Entry::new(&service(), f.key).and_then(|e| e.get_password()) {
            *slot(&mut c, f.key) = v;
        }
    }
    c
}

pub fn save(c: &Config) -> Result<()> {
    let public = public_config(c);
    let mut secrets = c.clone();
    for f in FIELDS.iter().filter(|f| f.secret) {
        let v = std::mem::take(slot(&mut secrets, f.key));
        let entry = keyring::Entry::new(&service(), f.key)?;
        if v.is_empty() {
            let _ = entry.delete_credential();
        } else {
            entry.set_password(&v)?;
        }
    }
    fs::write(dir().join("config.json"), serde_json::to_string_pretty(&public)?)?;
    Ok(())
}

fn public_config(c: &Config) -> Config {
    let mut public = c.clone();
    for f in FIELDS.iter().filter(|f| f.secret) {
        slot(&mut public, f.key).clear();
    }
    public
}

pub fn page_budget(value: &str) -> Result<usize> {
    let Ok(pages @ 1..=1000) = value.trim().parse::<usize>() else {
        bail!("Maximum page reads must be a whole number between 1 and 1000");
    };
    Ok(pages)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_config_excludes_every_keychain_secret() {
        let mut config = Config { tor_enabled: true, max_pages: 42, ..Config::default() };
        for f in FIELDS.iter().filter(|f| f.secret) {
            *slot(&mut config, f.key) = format!("private-{}", f.key);
        }
        let json = serde_json::to_string(&public_config(&config)).unwrap();
        assert!(!json.contains("private-"));
        let restored: Config = serde_json::from_str(&json).unwrap();
        assert!(restored.tor_enabled);
        assert_eq!(restored.max_pages, 42);
        assert_eq!(config.llm_key, "private-llm_key");
    }

    #[test]
    fn older_settings_keep_safe_defaults() {
        let config: Config = serde_json::from_str(r#"{"contact":"user@example.test"}"#).unwrap();
        assert!(!config.tor_enabled);
        assert_eq!(config.max_pages, Config::default().max_pages);
    }

    #[test]
    fn page_budget_rejects_invalid_or_unbounded_values() {
        for invalid in ["", "0", "1001", "-1", "1.5", "lots"] {
            assert!(page_budget(invalid).is_err());
        }
        assert_eq!(page_budget(" 1 ").unwrap(), 1);
        assert_eq!(page_budget("1000").unwrap(), 1000);
    }
}
