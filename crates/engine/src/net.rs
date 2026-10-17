use std::{
    collections::{HashMap, HashSet},
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant, SystemTime},
};

use anyhow::{Context, Result, bail};

use crate::{
    Config, Mode, document,
    extract::{self, Page},
    tor::{Fetch, Tor},
};

/// Hard cap for HTML/plain text, including onion reads; clearnet PDFs have a separate cap.
pub const MAX_BODY: usize = 2 * 1024 * 1024;

/// Polite HTTP: one shared client, per-host pacing, retries, and bounded readable documents.
#[derive(Clone, Copy, Debug, Default)]
pub struct RetrievalStats {
    pub requests: usize,
    pub retries: usize,
    pub recovered: usize,
}

#[derive(Clone)]
struct Attempts(usize);

#[derive(Clone, Copy)]
enum ReadKind {
    Document,
    Search,
    Lookup,
}

struct Download {
    status: u16,
    url: String,
    content_type: String,
    body: Vec<u8>,
    retries: usize,
}

pub struct Net {
    pub http: reqwest::Client,
    next: Mutex<HashMap<String, Instant>>,
    tor: Option<Tor>,
    onion_hosts: Mutex<HashSet<String>>,
    browser_enabled: bool,
    browser_path: String,
    reader: Option<crate::llm::Llm>,
    requests: AtomicUsize,
    retries: AtomicUsize,
    recovered: AtomicUsize,
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
            .retry(reqwest::retry::never())
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if !matches!(attempt.url().scheme(), "http" | "https")
                    || !attempt.url().username().is_empty()
                    || attempt.url().password().is_some()
                {
                    attempt.error("redirect URL must be HTTP(S) without credentials")
                } else if is_onion_host(attempt.url().host_str().unwrap_or_default()) {
                    attempt.error("clearnet redirects to .onion are forbidden")
                } else if attempt.previous().len() >= 5 {
                    attempt.error("too many redirects")
                } else {
                    attempt.follow()
                }
            }))
            .build()
            .expect("http client");
        Self {
            http,
            next: Mutex::new(HashMap::new()),
            tor: None,
            onion_hosts: Mutex::new(HashSet::new()),
            browser_enabled: cfg.browser_enabled,
            browser_path: cfg.browser_path.clone(),
            reader: cfg.visual_documents.then(|| crate::llm::Llm::new(cfg)),
            requests: AtomicUsize::new(0),
            retries: AtomicUsize::new(0),
            recovered: AtomicUsize::new(0),
        }
    }

    /// Only onion page reads use Tor. Search and model APIs still use the clearnet client.
    pub fn for_mode(cfg: &Config, mode: Mode) -> Result<Self> {
        let mut net = Self::new(cfg);
        if mode == Mode::Dark {
            net.browser_enabled = false;
            net.reader = None;
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

    /// Share the run's model accounting; does not enable visual reading when it is disabled.
    pub fn with_reader(mut self, reader: &crate::llm::Llm) -> Self {
        if self.reader.is_some() {
            self.reader = Some(reader.clone());
        }
        self
    }

    pub fn stats(&self) -> RetrievalStats {
        RetrievalStats {
            requests: self.requests.load(Ordering::Relaxed),
            retries: self.retries.load(Ordering::Relaxed),
            recovered: self.recovered.load(Ordering::Relaxed),
        }
    }

    /// Header-only generic requests retry GET/HEAD. Other methods may have side effects.
    pub async fn send(&self, rb: reqwest::RequestBuilder) -> Result<reqwest::Response> {
        let request = rb.build()?;
        tokio::time::timeout(Duration::from_secs(75), self.execute_request(&request, false, 0, true, false))
            .await
            .context("retrieval exceeded its 75-second retry deadline")?
    }

    /// Search adapters explicitly opt in: their POST bodies perform read-only searches.
    pub(crate) async fn search_body(&self, rb: reqwest::RequestBuilder) -> Result<String> {
        let download = self.read_body(rb, ReadKind::Search).await?;
        Ok(String::from_utf8_lossy(&download.body).into_owned())
    }

    /// Index lookups may inspect a bounded 404 body to distinguish no matches from failure.
    pub(crate) async fn lookup_body(&self, rb: reqwest::RequestBuilder) -> Result<(u16, String)> {
        let download = self.read_body(rb, ReadKind::Lookup).await?;
        Ok((download.status, String::from_utf8_lossy(&download.body).into_owned()))
    }

    async fn execute_request(
        &self,
        req: &reqwest::Request,
        read_only_search: bool,
        prior: usize,
        count_headers: bool,
        allow_not_found: bool,
    ) -> Result<reqwest::Response> {
        let url = req.url();
        let host = url.host_str().context("request URL has no host")?.to_string();
        if is_onion_host(&host) {
            bail!("direct .onion requests are forbidden; use the built-in Tor page reader");
        }
        if !matches!(url.scheme(), "http" | "https") || !url.username().is_empty() || url.password().is_some() {
            bail!("request URL must be HTTP(S) without embedded credentials");
        }
        let retryable = matches!(*req.method(), reqwest::Method::GET | reqwest::Method::HEAD)
            || (read_only_search && *req.method() == reqwest::Method::POST);
        for attempt in prior..3 {
            self.pace(&host).await;
            self.requests.fetch_add(1, Ordering::Relaxed);
            if attempt > 0 {
                self.retries.fetch_add(1, Ordering::Relaxed);
            }
            let result = self.http.execute(req.try_clone().context("request body cannot be replayed")?).await;
            let delay = match result {
                Ok(mut response) => {
                    if response.status().is_success() || (allow_not_found && response.status() == reqwest::StatusCode::NOT_FOUND) {
                        if count_headers && attempt > 0 {
                            self.recovered.fetch_add(1, Ordering::Relaxed);
                        }
                        response.extensions_mut().insert(Attempts(attempt - prior + 1));
                        return Ok(response);
                    }
                    if !retryable || attempt == 2 || !transient_status(response.status().as_u16()) {
                        let status = response.status();
                        response.error_for_status()?;
                        bail!("unexpected HTTP {status}");
                    }
                    retry_delay(response.headers().get(reqwest::header::RETRY_AFTER), attempt, SystemTime::now())?
                }
                Err(error) => {
                    if !retryable || attempt == 2 || !transient_transport(&error) {
                        return Err(error.into());
                    }
                    Duration::from_secs(1 << attempt)
                }
            };
            tokio::time::sleep(delay).await;
        }
        unreachable!("three-attempt loop always returns")
    }

    /// Headers and body share one three-attempt budget; partial bodies are discarded before replay.
    async fn read_body(&self, rb: reqwest::RequestBuilder, kind: ReadKind) -> Result<Download> {
        let document = matches!(kind, ReadKind::Document);
        let read_only_search = matches!(kind, ReadKind::Search | ReadKind::Lookup);
        let allow_not_found = matches!(kind, ReadKind::Lookup);
        let request = rb.build()?;
        tokio::time::timeout(Duration::from_secs(75), async {
            let mut attempts = 0;
            loop {
                let mut response = self.execute_request(&request, read_only_search, attempts, false, allow_not_found).await?;
                attempts += response.extensions().get::<Attempts>().context("missing request attempt accounting")?.0;
                let content_type = response
                    .headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("text/html")
                    .to_ascii_lowercase();
                let pdf = document && content_type.split(';').next().is_some_and(|kind| kind.trim() == "application/pdf");
                if document && !pdf && text_kind(&content_type).is_none() {
                    bail!("skipped unsupported content ({content_type})");
                }
                let limit = if pdf { document::MAX_PDF_BODY } else { MAX_BODY };
                if response.content_length().is_some_and(|n| n > limit as u64) {
                    bail!("{} exceeds the {} MiB body limit", if pdf { "PDF" } else { "page/search response" }, limit / 1024 / 1024);
                }
                let url = response.url().to_string();
                let status = response.status().as_u16();
                let result: Result<Vec<u8>> = async {
                    let mut body = Vec::new();
                    while let Some(chunk) = response.chunk().await? {
                        append_limited(&mut body, &chunk, limit)?;
                    }
                    Ok(body)
                }
                .await;
                match result {
                    Ok(body) => {
                        if attempts > 1 {
                            self.recovered.fetch_add(1, Ordering::Relaxed);
                        }
                        return Ok(Download { status, url, content_type, body, retries: attempts - 1 });
                    }
                    Err(error) if attempts < 3 && error.downcast_ref::<reqwest::Error>().is_some_and(transient_transport) => {
                        tokio::time::sleep(Duration::from_secs(1 << (attempts - 1))).await;
                    }
                    Err(error) => return Err(error),
                }
            }
        })
        .await
        .context("retrieval exceeded its 75-second retry deadline")?
    }

    /// Fetch readable text; PDFs are parsed only in an isolated worker process.
    pub async fn page(&self, url: &str) -> Result<Page> {
        let parsed = url::Url::parse(url).context("invalid page URL")?;
        if is_onion_host(parsed.host_str().unwrap_or_default()) {
            return self.onion_page(url).await;
        }
        let download = self.read_body(self.http.get(url), ReadKind::Document).await?;
        let recovered = (download.retries > 0).then(|| format!("HTTP retrieval recovered after {} retry attempt(s).", download.retries));
        let ctype = download.content_type;
        let pdf = ctype.split(';').next().is_some_and(|kind| kind.trim() == "application/pdf");
        let html = if pdf { false } else { text_kind(&ctype).context("unsupported page content")? };
        let final_url = download.url;
        let body = download.body;
        if pdf {
            let mut page = match self.reader.as_ref() {
                Some(reader) => document::pdf_with_reader(&final_url, &body, Some(reader)).await?,
                None => document::pdf(&final_url, &body).await?,
            };
            if let Some(note) = recovered {
                page.warnings.push(note);
            }
            return Ok(page);
        }
        let body = String::from_utf8_lossy(&body);
        if body.trim().is_empty() {
            bail!("empty body");
        }
        let mut page = extract::page(&final_url, &body, html);
        if html && browser_candidate(&body, &page.text) {
            if self.browser_enabled {
                match crate::browser::render(&final_url, &self.browser_path).await {
                    Ok(rendered) => {
                        let rendered = extract::page(&rendered.url, &rendered.html, true);
                        if rendered.text.trim().is_empty() {
                            page.warnings.push("Browser rendering returned no readable text; retained the original HTTP text.".into());
                        } else {
                            page = rendered;
                            page.warnings.push("This script-heavy page was read using the optional isolated browser.".into());
                        }
                    }
                    Err(error) => page.warnings.push(format!("Browser fallback failed; retained original HTTP text: {error:#}")),
                }
            } else {
                page.warnings
                    .push("This page may require JavaScript; browser rendering is disabled (and unavailable in Dark mode).".into());
            }
        }
        if page.text.trim().is_empty() {
            bail!(
                "page contains no readable text{}",
                if page.warnings.is_empty() { String::new() } else { format!(": {}", page.warnings.join("; ")) }
            );
        }
        if let Some(note) = recovered {
            page.warnings.push(note);
        }
        Ok(page)
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

fn transient_status(status: u16) -> bool {
    matches!(status, 408 | 429 | 500 | 502 | 503 | 504 | 529)
}

fn transient_transport(error: &reqwest::Error) -> bool {
    if error.is_timeout() {
        return true;
    }
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        if let Some(io) = cause.downcast_ref::<std::io::Error>() {
            return matches!(
                io.kind(),
                std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::ConnectionRefused
                    | std::io::ErrorKind::BrokenPipe
                    | std::io::ErrorKind::UnexpectedEof
                    | std::io::ErrorKind::TimedOut
            );
        }
        if let Some(http) = cause.downcast_ref::<hyper::Error>()
            && (http.is_incomplete_message() || http.is_closed())
        {
            return true;
        }
        source = cause.source();
    }
    false
}

fn retry_delay(header: Option<&reqwest::header::HeaderValue>, attempt: usize, now: SystemTime) -> Result<Duration> {
    let delay = header
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            v.parse::<u64>()
                .ok()
                .map(Duration::from_secs)
                .or_else(|| httpdate::parse_http_date(v).ok().map(|date| date.duration_since(now).unwrap_or_default()))
        })
        .unwrap_or_else(|| Duration::from_secs(1 << attempt));
    if delay > Duration::from_secs(30) {
        bail!("server Retry-After exceeds the 30-second retry wait budget; try again later");
    }
    Ok(delay)
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
    append_limited(body, chunk, MAX_BODY)
}

fn append_limited(body: &mut Vec<u8>, chunk: &[u8], limit: usize) -> Result<()> {
    if chunk.len() > limit.saturating_sub(body.len()) {
        bail!("document exceeds the {} MiB body limit", limit / 1024 / 1024);
    }
    body.extend_from_slice(chunk);
    Ok(())
}

fn browser_candidate(body: &str, text: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    if !lower.contains("<script")
        || ["captcha", "verify you are human", "checking your browser", "access denied"].iter().any(|s| lower.contains(s))
    {
        return false;
    }
    let text_lower = text.to_ascii_lowercase();
    text.trim().is_empty()
        || (text.chars().count() < 500
            && (lower.matches("<script").count() >= 3
                || ["id=\"root\"", "id='root'", "id=\"app\"", "id='app'", "id=\"__next\""].iter().any(|s| lower.contains(s))
                || text_lower.trim_start().starts_with("loading")))
        || ["enable javascript", "javascript is required", "requires javascript"].iter().any(|s| text_lower.contains(s))
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
    fn browser_fallback_is_only_for_unreadable_script_pages() {
        assert!(super::browser_candidate("<script src='app.js'></script><div id='root'></div>", "Loading"));
        assert!(!super::browser_candidate("<p>Short article</p>", "Short article"));
        assert!(!super::browser_candidate("<p>Short article</p><script src='analytics.js'></script>", "Short article"));
        assert!(!super::browser_candidate("<script src='app.js'></script>", &"Article text ".repeat(100)));
        assert!(!super::browser_candidate("<script></script> CAPTCHA", "Verify you are human"));
    }

    #[test]
    fn dark_mode_disables_all_browser_fallbacks() {
        let cfg = crate::Config { browser_enabled: true, visual_documents: true, tor_enabled: true, ..Default::default() };
        assert!(super::Net::for_mode(&cfg, crate::Mode::Normal).unwrap().browser_enabled);
        let dark = super::Net::for_mode(&cfg, crate::Mode::Dark).unwrap();
        assert!(!dark.browser_enabled);
        assert!(dark.reader.is_none());
    }

    #[test]
    fn pdf_body_limit_is_independent_of_the_text_limit() {
        let mut body = vec![0; super::MAX_BODY];
        super::append_limited(&mut body, b"a", crate::document::MAX_PDF_BODY).unwrap();
        assert!(super::append_body(&mut body, b"b").is_err());
        let mut body = vec![0; crate::document::MAX_PDF_BODY];
        assert!(super::append_limited(&mut body, b"a", crate::document::MAX_PDF_BODY).is_err());
    }

    #[test]
    fn retry_after_supports_http_dates_and_bounded_delays() {
        use super::*;
        use reqwest::header::HeaderValue;
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let date = HeaderValue::from_str(&httpdate::fmt_http_date(now + Duration::from_secs(12))).unwrap();
        assert_eq!(retry_delay(Some(&date), 0, now).unwrap(), Duration::from_secs(12));
        let past = HeaderValue::from_str(&httpdate::fmt_http_date(now - Duration::from_secs(12))).unwrap();
        assert_eq!(retry_delay(Some(&past), 0, now).unwrap(), Duration::ZERO);
        assert_eq!(retry_delay(Some(&HeaderValue::from_static("invalid")), 1, now).unwrap(), Duration::from_secs(2));
        assert_eq!(retry_delay(Some(&HeaderValue::from_static("0")), 1, now).unwrap(), Duration::ZERO);
        assert!(retry_delay(Some(&HeaderValue::from_static("31")), 0, now).is_err());
        assert!(!transient_status(403));
        assert!(transient_status(504));
    }

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
    async fn http_reader_preserves_late_text_and_surfaces_disabled_browser() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        for (content_type, body, expected, warning) in [
            ("text/plain", format!("{} late evidence", "界".repeat(20_000)), "late evidence", false),
            ("text/html", "<script src='app.js'></script><p>Enable JavaScript to read this page.</p>".into(), "JavaScript", true),
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let net = super::Net::new(&crate::Config::default());
            let server = async {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = [0; 4096];
                assert!(stream.read(&mut request).await.unwrap() > 0);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            };
            let url = format!("http://{address}/");
            let (result, _) = tokio::join!(net.page(&url), server);
            let page = result.unwrap();
            assert!(page.text.contains(expected), "{}", page.text);
            assert_eq!(!page.warnings.is_empty(), warning);
        }
    }

    #[tokio::test]
    async fn http_pdf_header_limit_is_checked_before_reading_or_parsing() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let net = super::Net::new(&crate::Config::default());
        let server = async {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            let received = stream.read(&mut request).await.unwrap();
            assert!(received > 0);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/pdf\r\nContent-Length: 12582913\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
        };
        let url = format!("http://{address}/paper.pdf");
        let (result, _) = tokio::join!(net.page(&url), server);
        assert!(result.unwrap_err().to_string().contains("PDF exceeds the 12 MiB"));
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
