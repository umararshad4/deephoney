//! URL indexes, not full-text keyword search. Only Wayback returns an archived page.
use anyhow::{Context, Result, ensure};
use serde_json::Value;

use super::{Row, arr, json, lookup_url, s};
use crate::net::Net;

pub async fn wayback(net: &Net, q: &str) -> Result<Vec<Row>> {
    let Some(target) = lookup_url(q) else { return Ok(vec![]) };
    let response = json(
        net,
        net.http.get("https://web.archive.org/cdx/search/cdx").query(&[
            ("url", target.as_str()),
            ("output", "json"),
            ("fl", "timestamp,original,mimetype"),
            ("filter", "statuscode:200"),
            ("filter", "mimetype:text/(html|plain)"),
            ("collapse", "digest"),
            ("limit", "8"),
        ]),
    )
    .await?;
    Ok(wayback_rows(&response))
}

fn wayback_rows(value: &Value) -> Vec<Row> {
    arr(value)
        .skip(1)
        .filter_map(|record| {
            let timestamp = record[0].as_str()?;
            if timestamp.len() != 14 || !timestamp.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let original = lookup_url(record[1].as_str()?)?;
            if !matches!(record[2].as_str(), Some("text/html" | "text/plain")) {
                return None;
            }
            Some((
                format!("Archived {} ({timestamp})", original),
                format!("https://web.archive.org/web/{timestamp}id_/{original}"),
                format!("Wayback capture dated {timestamp}; archival content, not the current page."),
            ))
        })
        .take(8)
        .collect()
}

/// Index-only discovery: returned URLs point to today's pages and remain in the Deep stratum.
pub async fn common_crawl(net: &Net, q: &str) -> Result<Vec<Row>> {
    let Some(target) = lookup_url(q) else { return Ok(vec![]) };
    let collections = json(net, net.http.get("https://index.commoncrawl.org/collinfo.json")).await?;
    let endpoint = arr(&collections).next().and_then(|v| v["cdx-api"].as_str()).context("Common Crawl has no collections")?;
    let endpoint = url::Url::parse(endpoint)?;
    ensure!(
        endpoint.scheme() == "https"
            && endpoint.host_str() == Some("index.commoncrawl.org")
            && endpoint.port().is_none()
            && endpoint.username().is_empty()
            && endpoint.password().is_none(),
        "unexpected Common Crawl index endpoint"
    );
    common_crawl_lookup(net, endpoint, target.as_str()).await
}

async fn common_crawl_lookup(net: &Net, endpoint: url::Url, target: &str) -> Result<Vec<Row>> {
    let (status, text) = net
        .lookup_body(net.http.get(endpoint).query(&[
            ("url", target),
            ("output", "json"),
            ("filter", "status:200"),
            // PyWB's unprefixed filters are literal substring matches, not regexes.
            ("filter", "~mime:^text/(html|plain)$"),
            ("collapse", "urlkey"),
            ("limit", "8"),
        ]))
        .await?;
    if status == 404 {
        let response = serde_json::from_str::<Value>(&text).ok();
        let expected = format!("No Captures found for: {target}");
        if response
            .as_ref()
            .and_then(Value::as_object)
            .is_some_and(|object| object.len() == 1 && object.get("message").and_then(Value::as_str) == Some(expected.as_str()))
        {
            return Ok(vec![]);
        }
        anyhow::bail!("Common Crawl returned HTTP 404 without the expected no-captures response");
    }
    ensure!(status == 200, "Common Crawl returned unexpected HTTP {status}");
    common_crawl_rows(&text)
}

fn common_crawl_rows(text: &str) -> Result<Vec<Row>> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .take(8)
        .map(|line| {
            let record: Value = serde_json::from_str(line)?;
            let original = lookup_url(record["url"].as_str().context("index result has no URL")?).context("invalid indexed URL")?;
            ensure!(matches!(record["mime"].as_str(), Some("text/html" | "text/plain")), "non-text index result");
            Ok((
                format!("Indexed {}", original),
                original.to_string(),
                format!(
                    "URL discovered in Common Crawl capture {}; fetched content is the CURRENT page, not this archived capture.",
                    s(&record["timestamp"])
                ),
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn common_crawl_http_distinguishes_no_captures_from_invalid_indexes_and_errors() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        async fn probe(status: u16, body: &str, declared_length: Option<usize>) -> Result<Vec<Row>> {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = format!("http://{}/index", listener.local_addr().unwrap());
            let body = body.to_owned();
            let server = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    let mut block = [0; 2048];
                    let size = stream.read(&mut block).await.unwrap();
                    assert!(size > 0);
                    request.extend_from_slice(&block[..size]);
                    assert!(request.len() < 8192);
                }
                let first = String::from_utf8_lossy(&request).lines().next().unwrap().to_owned();
                let query = first.split_whitespace().nth(1).unwrap();
                let parsed = url::Url::parse(&format!("http://fixture{query}")).unwrap();
                assert!(parsed.query_pairs().any(|(k, v)| k == "filter" && v == "~mime:^text/(html|plain)$"));
                let length = declared_length.unwrap_or(body.len());
                let response = format!(
                    "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n{body}"
                );
                let _ = stream.write_all(response.as_bytes()).await;
            });
            let net = Net::new(&crate::Config::default());
            let result = common_crawl_lookup(&net, endpoint.parse().unwrap(), "https://example.com/").await;
            server.await.unwrap();
            assert_eq!(net.stats().requests, 1);
            result
        }
        assert!(probe(404, r#"{"message":"No Captures found for: https://example.com/"}"#, None).await.unwrap().is_empty());
        for body in [
            r#""No index found for collection 'CC-MAIN-INVALID'""#,
            r#"{"message":"No Captures found for: https://different.example/"}"#,
            r#"{"message":"No Captures found for: https://example.com/","unexpected":true}"#,
            "<html>Not Found</html>",
        ] {
            assert!(probe(404, body, None).await.is_err());
        }
        assert!(probe(403, "Forbidden", None).await.is_err());
        assert!(probe(404, "", Some(crate::net::MAX_BODY + 1)).await.is_err());
        let valid = r#"{"url":"https://example.com/","mime":"text/html","timestamp":"20261018"}"#;
        assert_eq!(probe(200, valid, None).await.unwrap().len(), 1);
    }

    #[test]
    fn wayback_returns_immutable_text_capture_urls() {
        let rows = wayback_rows(&json!([
            ["timestamp", "original", "mimetype"],
            ["20200102123456", "https://example.com/deleted", "text/html"],
            ["bad", "https://example.com/", "text/html"],
            ["20200102123456", "https://example.com/image", "image/jpeg"]
        ]));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1, "https://web.archive.org/web/20200102123456id_/https://example.com/deleted");
    }

    #[test]
    fn common_crawl_never_labels_current_content_as_archived_evidence() {
        let rows = common_crawl_rows(r#"{"url":"https://example.com/","mime":"text/html","timestamp":"20200102123456"}"#).unwrap();
        assert_eq!(rows[0].1, "https://example.com/");
        assert!(rows[0].2.contains("CURRENT page"));
        assert!(common_crawl_rows(r#"{"url":"file:///etc/passwd","mime":"text/html"}"#).is_err());
        assert!(common_crawl_rows(r#"{"url":"https://example.com/","mime":"application/pdf"}"#).is_err());
    }
}
