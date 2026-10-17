use std::{
    collections::{HashMap, HashSet},
    sync::Mutex,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};

use crate::{
    Config, Mode,
    extract::{self, Page},
    tor::{Fetch, Tor},
};

/// Hard cap on any fetched body.
pub const MAX_BODY: usize = 2 * 1024 * 1024;

/// Polite HTTP: one shared client, per-host pacing, retry on 429/5xx, text-only page fetches.
pub struct Net {
    pub http: reqwest::Client,
    next: Mutex<HashMap<String, Instant>>,
    tor: Option<Tor>,
    onion_hosts: Mutex<HashSet<String>>,
}

impl Net {
    pub fn new(cfg: &Config) -> Self {
        let ua = if cfg.contact.is_empty() {
            "DeepHoney/0.1 (desktop research app)".to_string()
        } else {
            format!("DeepHoney/0.1 (desktop research app; {})", cfg.contact)
        };
        let http = reqwest::Client::builder()
            .user_agent(ua)
            .timeout(Duration::from_secs(25))
            .connect_timeout(Duration::from_secs(10))
            .cookie_store(true)
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if is_onion_host(attempt.url().host_str().unwrap_or_default()) {
                    attempt.error("clearnet redirects to .onion are forbidden")
                } else if attempt.previous().len() >= 5 {
                    attempt.error("too many redirects")
                } else {
                    attempt.follow()
                }
            }))
            .build()
            .expect("http client");
        Self { http, next: Mutex::new(HashMap::new()), tor: None, onion_hosts: Mutex::new(HashSet::new()) }
    }

    /// Only onion page reads use Tor. Search and model APIs still use the clearnet client.
    pub fn for_mode(cfg: &Config, mode: Mode) -> Result<Self> {
        let mut net = Self::new(cfg);
        if mode == Mode::Dark {
            if !cfg.tor_enabled {
                bail!("Dark mode requires built-in Tor: enable Tor in settings or set DEEPHONEY_TOR=1");
            }
            net.tor = Some(Tor::default());
        }
        Ok(net)
    }

    /// Only the Ahmia search adapter admits onion hosts to the reading frontier.
    pub(crate) fn allow_onion(&self, url: &url::Url) {
        if let Some(host) = url.host_str() {
            self.onion_hosts.lock().unwrap().insert(host.to_string());
        }
    }

    fn permitted_onion(&self, url: &str) -> Result<url::Url> {
        let url = onion_url(url).context("invalid v3 onion URL")?;
        if self.tor.is_none() {
            bail!("onion pages require Dark mode with built-in Tor enabled");
        }
        if !self.onion_hosts.lock().unwrap().contains(url.host_str().unwrap_or_default()) {
            bail!("onion host was not discovered through Ahmia's filtered index");
        }
        Ok(url)
    }

    /// Reserve the next slot for `host`, then sleep until it.
    async fn pace(&self, host: &str) {
        let gap = Duration::from_millis(match host {
            "export.arxiv.org" => 3000,
            "web.archive.org" | "archive.org" | "crt.sh" | "timetravel.mementoweb.org" => 2500,
            "api.github.com" | "www.courtlistener.com" => 7000,
            _ => 900,
        });
        let at = {
            let mut next = self.next.lock().unwrap();
            let now = Instant::now();
            let slot = next.get(host).copied().filter(|t| *t > now).unwrap_or(now);
            next.insert(host.to_string(), slot + gap);
            slot
        };
        tokio::time::sleep_until(at.into()).await;
    }

    pub async fn send(&self, rb: reqwest::RequestBuilder) -> Result<reqwest::Response> {
        let req = rb.build()?;
        let host = req.url().host_str().unwrap_or_default().to_string();
        if is_onion_host(&host) {
            bail!("direct .onion requests are forbidden; use the built-in Tor page reader");
        }
        let mut attempt = 0;
        loop {
            self.pace(&host).await;
            let res = self.http.execute(req.try_clone().context("request not cloneable")?).await?;
            if matches!(res.status().as_u16(), 429 | 502 | 503) && attempt < 2 {
                attempt += 1;
                tokio::time::sleep(Duration::from_secs(3 * attempt)).await;
                continue;
            }
            return Ok(res.error_for_status()?);
        }
    }

    /// Fetch a page as readable markdown. Rejects anything that is not HTML or plain text.
    pub async fn page(&self, url: &str) -> Result<Page> {
        let parsed = url::Url::parse(url).context("invalid page URL")?;
        if is_onion_host(parsed.host_str().unwrap_or_default()) {
            return self.onion_page(url).await;
        }
        let mut res = self.send(self.http.get(url)).await?;
        let ctype =
            res.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("text/html").to_ascii_lowercase();
        let html = text_kind(&ctype).with_context(|| format!("skipped non-text content ({ctype})"))?;
        let final_url = res.url().to_string();
        if res.content_length().is_some_and(|n| n > MAX_BODY as u64) {
            bail!("page exceeds the 2 MiB body limit");
        }
        let mut body = Vec::new();
        while let Some(chunk) = res.chunk().await? {
            append_body(&mut body, &chunk)?;
        }
        let body = String::from_utf8_lossy(&body);
        if body.trim().is_empty() {
            bail!("empty body");
        }
        Ok(extract::page(&final_url, &body, html))
    }

    async fn onion_page(&self, url: &str) -> Result<Page> {
        let mut url = self.permitted_onion(url)?;
        let tor = self.tor.as_ref().context("built-in Tor is disabled")?;
        for redirects in 0..=5 {
            let mut attempt = 0;
            let response = loop {
                self.pace(url.host_str().unwrap_or_default()).await;
                match tor.get(&url).await? {
                    Fetch::Retry { status, after } => {
                        if attempt == 2 {
                            bail!("onion server returned HTTP {status} after retries");
                        }
                        attempt += 1;
                        tokio::time::sleep(after).await;
                    }
                    response => break response,
                }
            };
            match response {
                Fetch::Redirect(location) => {
                    if redirects == 5 {
                        bail!("too many onion redirects");
                    }
                    let target = url.join(&location).context("invalid onion redirect")?;
                    // Never downgrade an onion request to direct HTTP or an unfiltered onion host.
                    url = self.permitted_onion(target.as_str())?;
                }
                Fetch::Body { text, html } => return Ok(extract::page(url.as_str(), &text, html)),
                Fetch::Retry { .. } => unreachable!(),
            }
        }
        unreachable!()
    }
}

fn is_onion_host(host: &str) -> bool {
    host.trim_end_matches('.').to_ascii_lowercase().ends_with(".onion")
}

/// Validate the address, version and checksum without DNS or opening a connection.
pub fn onion_url(raw: &str) -> Option<url::Url> {
    let mut url = url::Url::parse(raw).ok()?;
    if !matches!(url.scheme(), "http" | "https") || !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    url.host_str()?.parse::<tor_hscrypto::pk::HsId>().ok()?;
    url.set_fragment(None);
    Some(url)
}

pub(crate) fn append_body(body: &mut Vec<u8>, chunk: &[u8]) -> Result<()> {
    if chunk.len() > MAX_BODY.saturating_sub(body.len()) {
        bail!("page exceeds the 2 MiB body limit");
    }
    body.extend_from_slice(chunk);
    Ok(())
}

/// Some(true) = HTML, Some(false) = plain text, None = refuse (images, media, binaries, PDFs).
pub fn text_kind(content_type: &str) -> Option<bool> {
    let kind = content_type.split(';').next()?.trim();
    if kind.eq_ignore_ascii_case("text/html") {
        Some(true)
    } else if kind.eq_ignore_ascii_case("text/plain") {
        Some(false)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_text_is_fetched() {
        assert_eq!(super::text_kind("text/html; charset=utf-8"), Some(true));
        assert_eq!(super::text_kind("text/plain"), Some(false));
        for bad in [
            "image/jpeg",
            "video/mp4",
            "application/octet-stream",
            "application/pdf",
            "application/zip",
            "text/html-malicious",
            "application/xhtml+xml",
            "",
        ] {
            assert_eq!(super::text_kind(bad), None, "{bad}");
        }
    }

    #[test]
    fn body_cap_rejects_instead_of_truncating() {
        let mut body = vec![0; super::MAX_BODY - 1];
        super::append_body(&mut body, b"a").unwrap();
        assert!(super::append_body(&mut body, b"b").is_err());
        assert_eq!(body.len(), super::MAX_BODY);
    }

    #[test]
    fn onion_validation_checks_version_checksum_and_credentials() {
        let host = "2gzyxa5ihm7nsggfxnu52rck2vv4rvmdlkiu3zzui5du4xyclen53wid.onion";
        assert!(super::onion_url(&format!("http://{host}/")).is_some());
        assert!(super::onion_url(&format!("https://{host}/")).is_some());
        for bad in
            ["http://invalid.onion/", "http://example.com/", "http://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.onion/"]
        {
            assert!(super::onion_url(bad).is_none(), "{bad}");
        }
        assert!(super::onion_url(&format!("http://name:secret@{host}/")).is_none());
        assert!(super::onion_url(&format!("ftp://{host}/")).is_none());
    }

    #[tokio::test]
    async fn disabled_or_unfiltered_onions_are_rejected_before_network() {
        let cfg = crate::Config::default();
        assert!(super::Net::for_mode(&cfg, crate::Mode::Dark).is_err());
        let net = super::Net::new(&cfg);
        let onion = "http://2gzyxa5ihm7nsggfxnu52rck2vv4rvmdlkiu3zzui5du4xyclen53wid.onion/";
        assert!(net.page(onion).await.unwrap_err().to_string().contains("Dark mode"));
        assert!(net.send(net.http.get(onion)).await.unwrap_err().to_string().contains("direct .onion"));
        let net = super::Net::for_mode(&crate::Config { tor_enabled: true, ..cfg }, crate::Mode::Dark).unwrap();
        assert!(net.page(onion).await.unwrap_err().to_string().contains("Ahmia"));
        net.allow_onion(&super::onion_url(onion).unwrap());
        assert!(net.permitted_onion(onion).is_ok());
        assert!(net.permitted_onion("https://example.com/").is_err());
    }

    #[tokio::test]
    async fn direct_redirect_to_onion_is_refused() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let net = super::Net::new(&crate::Config::default());
        let server = async {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            assert!(stream.read(&mut request).await.unwrap() > 0);
            stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: http://2gzyxa5ihm7nsggfxnu52rck2vv4rvmdlkiu3zzui5du4xyclen53wid.onion/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
        };
        let url = format!("http://{address}/");
        let (result, _) = tokio::join!(net.page(&url), server);
        assert!(format!("{:#}", result.unwrap_err()).contains("clearnet redirects to .onion are forbidden"));
    }
}
