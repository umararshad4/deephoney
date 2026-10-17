//! URL indexes, not full-text keyword search. Only Wayback returns an archived page.
use anyhow::{Context, Result, ensure};
use serde_json::Value;

use super::{Row, arr, body, json, lookup_url, s};
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
    let text = body(
        net,
        net.http.get(endpoint).query(&[
            ("url", target.as_str()),
            ("output", "json"),
            ("filter", "status:200"),
            ("filter", "mime:text/(html|plain)"),
            ("collapse", "urlkey"),
            ("limit", "8"),
        ]),
    )
    .await?;
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
