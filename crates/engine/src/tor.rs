//! Text-only HTTP over embedded Arti; no SOCKS daemon or clearnet fallback.
use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result, bail};
use arti_client::{TorClient, TorClientConfig};
use bytes::Bytes;
use http_body_util::{BodyExt, Empty};
use hyper::{Request, header};
use hyper_util::rt::TokioIo;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::OnceCell,
};
use tor_rtcompat::PreferredRuntime;
use url::Url;

use crate::net::{MAX_BODY, append_body, text_kind};

#[derive(Default)]
pub(crate) struct Tor {
    client: OnceCell<Arc<TorClient<PreferredRuntime>>>,
}

#[derive(Debug)]
pub(crate) enum Fetch {
    Redirect(String),
    Retry { status: u16, after: Duration },
    Body { text: String, html: bool },
}

impl Tor {
    pub(crate) async fn get(&self, url: &Url) -> Result<Fetch> {
        let client = self
            .client
            .get_or_try_init(|| async {
                tokio::time::timeout(
                    Duration::from_secs(180),
                    TorClient::create_bootstrapped(TorClientConfig::default()),
                )
                .await
                .context("built-in Tor bootstrap timed out; check network access to Tor")?
                .context("built-in Tor bootstrap failed")
            })
            .await?;
        tokio::time::timeout(Duration::from_secs(90), async {
            let host = url.host_str().context("missing onion host")?;
            let port = url.port_or_known_default().context("missing onion port")?;
            let stream = client
                .connect((host, port))
                .await
                .context("Tor onion connection failed")?;
            if url.scheme() == "https" {
                // Platform certificate and hostname validation stay enabled for HTTPS onion sites.
                let tls = tokio_native_tls::native_tls::TlsConnector::new()?;
                let stream = tokio_native_tls::TlsConnector::from(tls)
                    .connect(host, stream)
                    .await
                    .context("onion TLS certificate validation failed")?;
                request(stream, url).await
            } else {
                request(stream, url).await
            }
        })
        .await
        .context("Tor onion page timed out")?
    }
}

async fn request<S>(stream: S, url: &Url) -> Result<Fetch>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (mut sender, connection) =
        hyper::client::conn::http1::handshake(TokioIo::new(stream)).await?;
    let request = Request::builder()
        .method("GET")
        .uri(&url[url::Position::BeforePath..url::Position::AfterQuery])
        .header(
            header::HOST,
            &url[url::Position::BeforeHost..url::Position::AfterPort],
        )
        .header(
            header::USER_AGENT,
            "DeepHoney/0.1 (Tor text research reader)",
        )
        .header(header::ACCEPT, "text/html, text/plain")
        .header(header::ACCEPT_ENCODING, "identity")
        .header(header::CONNECTION, "close")
        .body(Empty::<Bytes>::new())?;
    let read = async {
        let response = sender.send_request(request).await?;
        let status = response.status();
        if matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308) {
            let location = response
                .headers()
                .get(header::LOCATION)
                .context("onion redirect without Location")?
                .to_str()?;
            return Ok(Fetch::Redirect(location.to_string()));
        }
        if matches!(status.as_u16(), 429 | 502 | 503) {
            let seconds = response
                .headers()
                .get(header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(3)
                .clamp(1, 60);
            return Ok(Fetch::Retry {
                status: status.as_u16(),
                after: Duration::from_secs(seconds),
            });
        }
        if !status.is_success() {
            bail!("onion server returned HTTP {status}");
        }
        let kind = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        let html =
            text_kind(kind).with_context(|| format!("skipped non-text onion content ({kind})"))?;
        if response
            .headers()
            .get(header::CONTENT_ENCODING)
            .is_some_and(|v| !v.as_bytes().eq_ignore_ascii_case(b"identity"))
        {
            bail!("compressed onion responses are not accepted by the text-only reader");
        }
        if response
            .headers()
            .get(header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .is_some_and(|n| n > MAX_BODY as u64)
        {
            bail!("page exceeds the 2 MiB body limit");
        }
        let mut body = response.into_body();
        let mut bytes = Vec::new();
        while let Some(frame) = body.frame().await {
            if let Ok(data) = frame?.into_data() {
                append_body(&mut bytes, &data)?;
            }
        }
        let text = String::from_utf8_lossy(&bytes).into_owned();
        if text.trim().is_empty() {
            bail!("empty onion body");
        }
        Ok(Fetch::Body { text, html })
    };
    tokio::pin!(read);
    // Drive Hyper here so cancellation drops both futures; a clean EOF may precede body delivery.
    tokio::select! {
        biased;
        result = &mut read => result,
        result = connection => {
            result.context("onion HTTP connection failed")?;
            read.await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn fixture(response: String) -> (Result<Fetch>, String) {
        let (client, mut server) = tokio::io::duplex(8192);
        let serve = async {
            let mut request = Vec::new();
            loop {
                let mut byte = [0];
                server.read_exact(&mut byte).await.unwrap();
                request.push(byte[0]);
                if request.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            // A rejected header may close the reader before the fixture finishes its body.
            let _ = server.write_all(response.as_bytes()).await;
            String::from_utf8(request).unwrap()
        };
        let url = Url::parse(
            "http://2gzyxa5ihm7nsggfxnu52rck2vv4rvmdlkiu3zzui5du4xyclen53wid.onion/path?q=value",
        )
        .unwrap();
        let (result, sent) = tokio::join!(request(client, &url), serve);
        (result, sent)
    }

    #[tokio::test]
    async fn reads_text_without_identity_headers() {
        let (result, sent) = fixture(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 5\r\n\r\nhello".into(),
        )
        .await;
        assert!(matches!(result.unwrap(), Fetch::Body { text, html: false } if text == "hello"));
        let sent = sent.to_ascii_lowercase();
        assert!(sent.starts_with("get /path?q=value http/1.1"));
        assert!(!sent.contains("cookie:"));
        assert!(!sent.contains("authorization:"));
        assert!(!sent.contains("referer:"));
    }

    #[tokio::test]
    async fn rejects_binary_missing_types_and_oversized_bodies() {
        for headers in [
            "Content-Type: image/png\r\nContent-Length: 4".to_string(),
            "Content-Length: 4".to_string(),
            format!(
                "Content-Type: text/plain\r\nContent-Length: {}",
                MAX_BODY + 1
            ),
            "Content-Type: text/plain\r\nContent-Encoding: gzip\r\nContent-Length: 4".to_string(),
        ] {
            let (result, _) = fixture(format!("HTTP/1.1 200 OK\r\n{headers}\r\n\r\nbody")).await;
            assert!(result.is_err(), "{headers}");
        }
        let body = "x".repeat(MAX_BODY + 1);
        let (result, _) = fixture(format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{body}\r\n0\r\n\r\n", body.len())).await;
        assert!(result.unwrap_err().to_string().contains("2 MiB"));
    }

    #[tokio::test]
    async fn surfaces_redirects_and_backoff_without_following() {
        let (result, _) = fixture(
            "HTTP/1.1 302 Found\r\nLocation: https://example.com/\r\nContent-Length: 0\r\n\r\n"
                .into(),
        )
        .await;
        assert!(matches!(result.unwrap(), Fetch::Redirect(url) if url == "https://example.com/"));
        let (result, _) = fixture(
            "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 300\r\nContent-Length: 0\r\n\r\n"
                .into(),
        )
        .await;
        assert!(
            matches!(result.unwrap(), Fetch::Retry { status: 429, after } if after == Duration::from_secs(60))
        );
    }

    #[tokio::test]
    async fn cancelling_a_read_drops_its_connection() {
        let (client, mut server) = tokio::io::duplex(8192);
        let url =
            Url::parse("http://2gzyxa5ihm7nsggfxnu52rck2vv4rvmdlkiu3zzui5du4xyclen53wid.onion/")
                .unwrap();
        let read = tokio::time::timeout(Duration::from_millis(50), request(client, &url));
        let observe = async {
            let mut bytes = Vec::new();
            server.read_to_end(&mut bytes).await.unwrap();
            assert!(bytes.starts_with(b"GET / HTTP/1.1"));
        };
        let (result, ()) = tokio::time::timeout(Duration::from_secs(2), async {
            tokio::join!(read, observe)
        })
        .await
        .expect("cancelled reader left its connection alive");
        assert!(result.is_err());
    }
}
