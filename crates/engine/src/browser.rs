//! Explicit, isolated clearnet rendering; never connects to an existing browser.
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use chromiumoxide::{
    Browser, BrowserConfig, Page,
    async_process::Child,
    cdp::browser_protocol::{
        browser::{SetDownloadBehaviorBehavior, SetDownloadBehaviorParams},
        fetch::{
            ContinueRequestParams, ContinueResponseParams, EnableParams, EventRequestPaused, FailRequestParams, FulfillRequestParams,
            GetResponseBodyParams, HeaderEntry, RequestPattern, RequestStage,
        },
        network::{ErrorReason, ResourceType, SetBypassServiceWorkerParams},
        page::SetBypassCspParams,
    },
    handler::HandlerConfig,
};
use futures::StreamExt;
use url::Url;

const MAX_REQUESTS: usize = 100;
const MAX_DOM_BYTES: usize = 2 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(30);

fn web_url(raw: &str) -> Result<Url> {
    let url = Url::parse(raw).context("Invalid browser URL")?;
    ensure!(matches!(url.scheme(), "http" | "https"), "Browser rendering only accepts HTTP(S)");
    ensure!(url.username().is_empty() && url.password().is_none(), "Browser URLs cannot contain credentials");
    let host = url.host_str().context("Browser URL has no host")?.trim_end_matches('.');
    ensure!(host != "onion" && !host.ends_with(".onion"), "Onion pages cannot use browser rendering");
    Ok(url)
}

fn allowed_request(raw: &str, method: &str, kind: &ResourceType) -> bool {
    method == "GET"
        && web_url(raw).is_ok()
        && matches!(
            kind,
            ResourceType::Document | ResourceType::Script | ResourceType::Stylesheet | ResourceType::Xhr | ResourceType::Fetch
        )
}

fn allowed_response(event: &EventRequestPaused) -> bool {
    if event.response_error_reason.is_some() {
        return false;
    }
    let headers = event.response_headers.as_deref().unwrap_or_default();
    if headers
        .iter()
        .any(|header| header.name.eq_ignore_ascii_case("content-disposition") && header.value.to_ascii_lowercase().contains("attachment"))
    {
        return false;
    }
    if event.response_status_code.is_some_and(|status| (300..400).contains(&status)) {
        // Redirect destinations are checked again at the request stage, before network access.
        return true;
    }
    let mime = headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("content-type"))
        .map(|header| header.value.split(';').next().unwrap_or("").trim().to_ascii_lowercase())
        .unwrap_or_default();
    allowed_content(&event.resource_type, &mime)
}

fn allowed_content(kind: &ResourceType, mime: &str) -> bool {
    match kind {
        ResourceType::Document => matches!(mime, "text/html" | "application/xhtml+xml" | "text/plain"),
        ResourceType::Script => matches!(mime, "text/javascript" | "application/javascript" | "application/ecmascript" | "text/ecmascript"),
        ResourceType::Stylesheet => mime == "text/css",
        ResourceType::Xhr | ResourceType::Fetch => {
            mime.starts_with("text/")
                || matches!(mime, "application/json" | "application/xml" | "application/xhtml+xml")
                || mime.ends_with("+json")
                || mime.ends_with("+xml")
        }
        _ => false,
    }
}

#[derive(Debug)]
pub(crate) struct Rendered {
    pub url: String,
    pub html: String,
}

struct BrowserProcess {
    child: Child,
    profile: Option<tempfile::TempDir>,
}

impl Drop for BrowserProcess {
    fn drop(&mut self) {
        // Chrome can recreate profile files while exiting; reap it before removing the profile.
        let deadline = Instant::now() + Duration::from_millis(500);
        let _ = self.child.as_mut_inner().start_kill();
        while matches!(self.child.try_wait(), Ok(None)) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        if let Some(profile) = self.profile.take() {
            let path = profile.path().to_owned();
            let _ = profile.close();
            while path.exists() && Instant::now() < deadline {
                let _ = std::fs::remove_dir_all(&path);
                if path.exists() {
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        }
    }
}

async fn own_websocket(process: &mut BrowserProcess) -> Result<String> {
    let port_file = process.profile.as_ref().context("Browser profile missing")?.path().join("DevToolsActivePort");
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if let Some(status) = process.child.try_wait()? {
                bail!("Isolated Chrome/Chromium exited during startup: {status}");
            }
            if let Ok(text) = tokio::fs::read_to_string(&port_file).await {
                let mut lines = text.lines();
                if let (Some(port), Some(path)) = (lines.next(), lines.next()) {
                    let port = port.parse::<u16>().context("Invalid isolated browser debug port")?;
                    let id = path.strip_prefix("/devtools/browser/").context("Invalid isolated browser debug path")?;
                    ensure!(
                        port > 0 && !id.is_empty() && id.len() <= 256 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
                        "Invalid isolated browser endpoint"
                    );
                    return Ok(format!("ws://127.0.0.1:{port}/devtools/browser/{id}"));
                }
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .context("Isolated Chrome/Chromium startup exceeded 15 seconds")?
}

/// Render public HTTP(S) HTML in a fresh profile, with no detached event tasks.
pub(crate) async fn render(raw_url: &str, executable: &str) -> Result<Rendered> {
    let url = web_url(raw_url)?;
    let profile = tempfile::Builder::new().prefix("deephoney-browser-").tempdir()?;
    let mut builder = BrowserConfig::builder()
        .new_headless_mode()
        .disable_default_args()
        .args([
            "disable-background-networking",
            "disable-component-update",
            "disable-sync",
            "no-first-run",
            "no-default-browser-check",
            "disable-extensions",
            "password-store=basic",
            "use-mock-keychain",
        ])
        .user_data_dir(profile.path());
    if !executable.trim().is_empty() {
        builder = builder.chrome_executable(executable);
    }
    let config = builder.build().map_err(anyhow::Error::msg)?;
    let child = config.launch().context("Could not launch installed Chrome/Chromium")?;
    let mut process = BrowserProcess { child, profile: Some(profile) };
    let mut stderr = process.child.stderr.take().context("Isolated browser stderr unavailable")?.inner;
    tokio::time::timeout(TIMEOUT, async {
        let websocket = own_websocket(&mut process).await?;
        // This endpoint comes only from our just-launched child's fresh profile, never a user browser.
        let (browser, mut handler) = Browser::connect_with_config(
            websocket,
            HandlerConfig {
                ignore_https_errors: false,
                request_intercept: true,
                cache_enabled: false,
                request_timeout: Duration::from_secs(10),
                ..Default::default()
            },
        )
        .await
        .context("Could not connect to isolated Chrome/Chromium")?;
        let work = async {
            browser.execute(SetDownloadBehaviorParams::new(SetDownloadBehaviorBehavior::Deny)).await?;
            let page = browser.new_page("about:blank").await?;
            render_page(&page, url.as_str()).await
        };
        let driver = async {
            let events = async {
                while let Some(event) = handler.next().await {
                    event.context("Browser connection failed")?;
                }
                bail!("Browser connection closed before rendering completed")
            };
            let logs = async {
                tokio::io::copy(&mut stderr, &mut tokio::io::sink()).await?;
                std::future::pending::<Result<Rendered>>().await
            };
            tokio::select! { result = events => result, result = logs => result }
        };
        let result = tokio::select! { result = work => result, result = driver => result };
        process.child.kill().await.context("Could not stop isolated browser")?;
        result
    })
    .await
    .context("Browser rendering exceeded 30 seconds")?
}

async fn render_page(page: &Page, url: &str) -> Result<Rendered> {
    let mut events = page.event_listener::<EventRequestPaused>().await?;
    page.execute(SetBypassServiceWorkerParams::new(true)).await?;
    page.execute(SetBypassCspParams::new(false)).await?;
    page.execute(
        EnableParams::builder()
            .pattern(RequestPattern::builder().url_pattern("*").request_stage(RequestStage::Request).build())
            .pattern(RequestPattern::builder().url_pattern("*").request_stage(RequestStage::Response).build())
            .build(),
    )
    .await?;
    let main_response = AtomicBool::new(false);
    let intercept = async {
        let mut requests = 0;
        let mut document_started = false;
        while let Some(event) = events.next().await {
            let response = event.response_status_code.is_some() || event.response_error_reason.is_some();
            if !response {
                requests += 1;
            }
            let navigation_allowed = if !response && event.resource_type == ResourceType::Document {
                let allowed = !document_started || event.redirected_request_id.is_some();
                document_started = true;
                allowed
            } else {
                true
            };
            let allowed = navigation_allowed
                && requests <= MAX_REQUESTS
                && allowed_request(&event.request.url, &event.request.method, &event.resource_type)
                && (!response || allowed_response(&event));
            if allowed
                && response
                && event.resource_type == ResourceType::Document
                && !event.response_status_code.is_some_and(|status| (300..400).contains(&status))
            {
                ensure!(
                    !event
                        .response_headers
                        .as_deref()
                        .unwrap_or_default()
                        .iter()
                        .any(|header| header.name.eq_ignore_ascii_case("content-length")
                            && header.value.trim().parse::<usize>().is_ok_and(|length| length > MAX_DOM_BYTES)),
                    "Browser document exceeded 2 MiB"
                );
                let body = page.execute(GetResponseBodyParams::new(event.request_id.clone())).await?.result;
                ensure!(body.body.len() <= MAX_DOM_BYTES.div_ceil(3) * 4, "Browser document exceeded 2 MiB");
                let bytes = if body.base64_encoded { BASE64.decode(&body.body)? } else { body.body.into_bytes() };
                ensure!(bytes.len() <= MAX_DOM_BYTES, "Browser document exceeded 2 MiB");
                let mut headers = event.response_headers.clone().unwrap_or_default();
                headers.retain(|header| {
                    !header.name.eq_ignore_ascii_case("content-length")
                        && !header.name.eq_ignore_ascii_case("content-encoding")
                        && !header.name.eq_ignore_ascii_case("transfer-encoding")
                });
                headers.push(HeaderEntry::new("Content-Length", bytes.len().to_string()));
                if !body.base64_encoded {
                    for header in &mut headers {
                        if header.name.eq_ignore_ascii_case("content-type") {
                            header.value = format!("{}; charset=utf-8", header.value.split(';').next().unwrap_or("text/html"));
                        }
                    }
                }
                // Re-fulfill the body so Chrome applies this CSP before running any page scripts.
                headers.push(HeaderEntry::new("Content-Security-Policy", "form-action 'none'; worker-src 'none'; frame-src 'none'; object-src 'none'; script-src http: https: 'unsafe-inline' 'unsafe-eval'; style-src http: https: 'unsafe-inline'; connect-src http: https:; img-src 'none'; media-src 'none'; font-src 'none'"));
                page.execute(
                    FulfillRequestParams::builder()
                        .request_id(event.request_id.clone())
                        .response_code(event.response_status_code.unwrap_or(200))
                        .response_phrase(event.response_status_text.clone().unwrap_or_default())
                        .response_headers(headers)
                        .body(BASE64.encode(bytes))
                        .build()
                        .map_err(anyhow::Error::msg)?,
                )
                .await?;
                main_response.store(true, Ordering::Release);
            } else if allowed && response {
                page.execute(ContinueResponseParams::new(event.request_id.clone())).await?;
            } else if allowed {
                page.execute(
                    ContinueRequestParams::builder()
                        .request_id(event.request_id.clone())
                        .intercept_response(true)
                        .build()
                        .map_err(anyhow::Error::msg)?,
                )
                .await?;
            } else {
                page.execute(FailRequestParams::new(event.request_id.clone(), ErrorReason::BlockedByClient)).await?;
                ensure!(requests <= MAX_REQUESTS, "Browser rendering exceeded 100 requests");
                ensure!(
                    !navigation_allowed || event.resource_type != ResourceType::Document,
                    "Browser refused a forbidden URL, binary response or disallowed document"
                );
            }
        }
        bail!("Browser request interception stopped")
    };
    // Response interception can stall library navigation tracking; wait for the actual accepted DOM.
    let navigation = async {
        page.goto(url).await?;
        std::future::pending::<Result<Rendered>>().await
    };
    let document = async {
        loop {
            if main_response.load(Ordering::Acquire) && page.url().await?.is_some_and(|url| url != "about:blank") {
                let (location, state): (String, String) = page.evaluate("[location.href, document.readyState]").await?.into_value()?;
                web_url(&location)?;
                if state == "complete" {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        // Allow asynchronous DOM updates to settle without clicking, authenticating or submitting forms.
        let html: Option<String> = page
            .evaluate(
                r#"new Promise(resolve => {
            let quiet, limit;
            const observer = new MutationObserver(() => { clearTimeout(quiet); quiet = setTimeout(done, 500); });
            const done = () => {
                observer.disconnect(); clearTimeout(quiet); clearTimeout(limit);
                const html = document.documentElement.outerHTML;
                resolve(new TextEncoder().encode(html).length <= 2097152 ? html : null);
            };
            observer.observe(document, {subtree:true, childList:true, characterData:true});
            quiet = setTimeout(done, 500); limit = setTimeout(done, 2500);
        })"#,
            )
            .await?
            .into_value()?;
        let html = html.context("Rendered HTML exceeded 2 MiB")?;
        ensure!(html.len() <= MAX_DOM_BYTES, "Rendered HTML exceeded 2 MiB");
        ensure!(!html.trim().is_empty(), "Browser returned empty HTML");
        let final_url = page.url().await?.context("Rendered page has no URL")?;
        let url = web_url(&final_url)?.to_string();
        Ok(Rendered { url, html })
    };
    tokio::select! { result = intercept => result, result = navigation => result, result = document => result }
}

#[cfg(test)]
mod tests {
    use super::*;

    static BROWSER_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[test]
    fn rejects_unsafe_browser_requests() {
        for raw in [
            "file:///tmp/test",
            "data:text/html,hello",
            "javascript:alert(1)",
            "https://user:password@example.com",
            "http://abc.onion/",
            "https://abc.ONION./",
            "http://onion/",
        ] {
            assert!(web_url(raw).is_err(), "{raw}");
        }
        for kind in [ResourceType::Image, ResourceType::Media, ResourceType::Font, ResourceType::WebSocket, ResourceType::Other] {
            assert!(!allowed_request("https://example.com/", "GET", &kind));
        }
        assert!(!allowed_request("https://example.com/", "POST", &ResourceType::Fetch));
        assert!(!allowed_request("https://abc.onion./", "GET", &ResourceType::Document));
        assert!(allowed_request("https://example.com/", "GET", &ResourceType::Document));
    }

    #[test]
    fn accepts_only_renderable_response_types() {
        assert!(allowed_content(&ResourceType::Script, "application/javascript"));
        assert!(allowed_content(&ResourceType::Fetch, "application/json"));
        assert!(!allowed_content(&ResourceType::Document, "application/pdf"));
        assert!(!allowed_content(&ResourceType::Fetch, "application/octet-stream"));
        assert!(!allowed_content(&ResourceType::Document, ""));
        assert!(!allowed_content(&ResourceType::Stylesheet, "text/html"));
    }

    #[tokio::test]
    async fn invalid_url_fails_before_browser_launch() {
        assert!(render("https://example.onion./", "/no-such-browser").await.unwrap_err().to_string().contains("Onion"));
    }

    #[tokio::test]
    async fn missing_browser_returns_a_named_error() {
        let error = render("https://example.com/", "/definitely-missing-deephoney-browser").await.unwrap_err();
        assert!(error.to_string().contains("Could not launch installed Chrome/Chromium"), "{error:#}");
    }

    struct Fixture {
        base: String,
        seen: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        server: tokio::task::JoinHandle<()>,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            self.server.abort();
        }
    }

    impl Fixture {
        async fn new() -> Self {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            let requests = seen.clone();
            let server = tokio::spawn(async move {
                let mut connections = tokio::task::JoinSet::new();
                loop {
                    tokio::select! {
                        socket = listener.accept() => {
                            let (mut socket, _) = socket.unwrap();
                            let requests = requests.clone();
                            connections.spawn(async move {
                                let mut bytes = vec![0; 16_384];
                                let n = socket.read(&mut bytes).await.unwrap_or(0);
                                let first = String::from_utf8_lossy(&bytes[..n]).lines().next().unwrap_or("").to_string();
                                requests.lock().unwrap().push(first.clone());
                                let path = first.split_whitespace().nth(1).unwrap_or("");
                                let (status, headers, body) = match path {
                                    "/" => (200, "Content-Type: text/html\r\n", r#"<!doctype html><html><body><p id="result">waiting</p>
                                      <form action="/form" method="GET" id="form"></form><script>
                                      try {const worker=new Worker(URL.createObjectURL(new Blob(["postMessage('executed');fetch("+JSON.stringify(location.origin+'/worker-post')+",{method:'POST',body:'refuse'}).catch(()=>{})"],{type:'text/javascript'})));worker.onerror=()=>document.body.dataset.worker='blocked';worker.onmessage=()=>document.body.dataset.worker='executed';} catch (_) {document.body.dataset.worker='blocked';}
                                      try {const ws=new WebSocket(location.origin.replace('http','ws')+'/ws');ws.onerror=()=>document.body.dataset.websocket='blocked';ws.onopen=()=>document.body.dataset.websocket='opened';} catch (_) {document.body.dataset.websocket='blocked';}
                                      navigator.serviceWorker.register('/worker.js').then(()=>document.body.dataset.serviceworker='allowed').catch(()=>document.body.dataset.serviceworker='blocked');
                                      const dataScript = document.createElement('script');dataScript.src='data:text/javascript,document.body.dataset.datascript="executed"';document.head.append(dataScript);
                                      document.querySelector('#form').submit();
                                      const image = new Image(); image.src = '/image';
                                      fetch('/post', {method:'POST',body:'refuse'}).catch(()=>{});
                                      fetch('http://blocked.onion./').catch(()=>{});
                                      fetch('/redirect').catch(()=>{});
                                      fetch('/binary').then(()=>document.body.dataset.binary='loaded').catch(()=>document.body.dataset.binary='blocked');
                                      fetch('/data').then(r=>r.text()).then(text=>document.querySelector('#result').textContent=text);
                                      </script></body></html>"#.to_string()),
                                    "/redirect-document" => (302, "Location: /\r\n", String::new()),
                                    "/gzip" => (200, "Content-Type: text/html\r\nContent-Encoding: gzip\r\n", String::new()),
                                    "/data" => (200, "Content-Type: text/plain\r\n", "Harmless JavaScript rendered evidence".into()),
                                    "/binary" => (200, "Content-Type: application/octet-stream\r\n", "not evidence".into()),
                                    "/redirect" => (302, "Location: http://blocked.onion./\r\n", String::new()),
                                    "/oversized" => (200, "Content-Type: text/html\r\n", format!("<html><body>{}</body></html>", "x".repeat(MAX_DOM_BYTES + 1))),
                                    "/storm" => (200, "Content-Type: text/html\r\n", "<html><script>for(let i=0;i<110;i++)fetch('/data?n='+i).catch(()=>{});</script></html>".into()),
                                    "/hang" => { std::future::pending::<()>().await; unreachable!() },
                                    _ => (200, "Content-Type: text/plain\r\n", "unexpected request".into()),
                                };
                                let body = if path == "/gzip" { include_bytes!("../tests/fixtures/browser/rendered.html.gz").to_vec() } else { body.into_bytes() };
                                let mut response = format!("HTTP/1.1 {status} Fixture\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n", body.len()).into_bytes();
                                response.extend(body);
                                let _ = socket.write_all(&response).await;
                            });
                        }
                        _ = connections.join_next(), if !connections.is_empty() => {}
                    }
                }
            });
            Self { base, seen, server }
        }
    }

    #[tokio::test]
    #[ignore = "requires installed Chrome/Chromium; launches isolated profiles only"]
    async fn installed_browser_renders_js_and_refuses_post_binary_and_forms() {
        let _lock = BROWSER_TEST_LOCK.lock().await;
        let fixture = Fixture::new().await;
        let rendered = render(&fixture.base, "").await.unwrap();
        assert_eq!(rendered.url, format!("{}/", fixture.base));
        let html = rendered.html;

        assert!(
            html.contains("<p id=\"result\">Harmless JavaScript rendered evidence</p>"),
            "{html}; requests: {:?}",
            fixture.seen.lock().unwrap()
        );
        assert!(html.contains("data-binary=\"blocked\""), "{html}");
        assert!(html.contains("data-worker=\"blocked\""), "{html}");
        assert!(html.contains("data-websocket=\"blocked\""), "{html}");
        assert!(html.contains("data-serviceworker=\"blocked\""), "{html}");
        assert!(!html.contains("data-datascript=\"executed\""), "{html}");
        {
            let seen = fixture.seen.lock().unwrap();
            assert!(seen.iter().any(|request| request.starts_with("GET /data ")), "{seen:?}");
            assert!(
                !seen.iter().any(|request| request.starts_with("POST ")
                    || request.contains(" /form")
                    || request.contains(" /image")
                    || request.contains(" /ws")
                    || request.contains(" /worker.js")),
                "{seen:?}"
            );
        }

        let redirected = render(&format!("{}/redirect-document", fixture.base), "").await.unwrap();
        assert_eq!(redirected.url, format!("{}/", fixture.base));
        let gzip = render(&format!("{}/gzip", fixture.base), "").await.unwrap();
        assert!(gzip.html.contains("data-gzip=\"rendered\""), "{}", gzip.html);
        assert!(render(&format!("{}/oversized", fixture.base), "").await.unwrap_err().to_string().contains("2 MiB"));
        assert!(render(&format!("{}/storm", fixture.base), "").await.unwrap_err().to_string().contains("100 requests"));
    }

    #[tokio::test]
    #[ignore = "requires installed Chrome/Chromium and Unix process inspection"]
    async fn cancelled_browser_removes_profile_and_stops_its_process() {
        let _lock = BROWSER_TEST_LOCK.lock().await;
        let fixture = Fixture::new().await;
        let url = format!("{}/hang", fixture.base);
        let render_task = tokio::spawn(async move { render(&url, "").await });
        tokio::time::timeout(Duration::from_secs(15), async {
            while !fixture.seen.lock().unwrap().iter().any(|request| request.contains(" /hang ")) {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .unwrap();
        let output = std::process::Command::new("ps").args(["-axo", "command"]).output().unwrap();
        let process_list = String::from_utf8_lossy(&output.stdout);
        let profile = process_list
            .lines()
            .find_map(|line| {
                line.split_whitespace()
                    .find_map(|word| word.strip_prefix("--user-data-dir=").filter(|path| path.contains("deephoney-browser-")))
            })
            .unwrap()
            .to_owned();
        assert!(std::path::Path::new(&profile).exists());
        render_task.abort();
        assert!(render_task.await.unwrap_err().is_cancelled());
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let output = std::process::Command::new("ps").args(["-axo", "command"]).output().unwrap();
                if !String::from_utf8_lossy(&output.stdout).contains(&format!("--user-data-dir={profile}")) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .unwrap();
        assert!(!std::path::Path::new(&profile).exists());
    }
}
