use anyhow::{Result, ensure};
use scraper::{Html, Selector};
use serde_json::{Value, json};

use super::{Row, arr, body, json, s};
use crate::{Config, net::Net};

pub async fn brave(net: &Net, cfg: &Config, q: &str) -> Result<Vec<Row>> {
    let rb = net
        .http
        .get("https://api.search.brave.com/res/v1/web/search")
        .query(&[("q", q), ("count", "10")])
        .header("X-Subscription-Token", &cfg.brave_key);
    let v: Value = json(net, rb).await?;
    Ok(arr(&v["web"]["results"]).map(|r| (s(&r["title"]), s(&r["url"]), s(&r["description"]))).collect())
}

pub async fn exa(net: &Net, cfg: &Config, q: &str) -> Result<Vec<Row>> {
    let rb = net
        .http
        .post("https://api.exa.ai/search")
        .header("x-api-key", &cfg.exa_key)
        .json(&json!({"query": q, "numResults": 10, "contents": {"highlights": true}}));
    let v: Value = json(net, rb).await?;
    Ok(arr(&v["results"])
        .map(|r| {
            let hl = arr(&r["highlights"]).filter_map(Value::as_str).collect::<Vec<_>>().join(" … ");
            (s(&r["title"]), s(&r["url"]), hl)
        })
        .collect())
}

pub async fn tavily(net: &Net, cfg: &Config, q: &str) -> Result<Vec<Row>> {
    let rb = net.http.post("https://api.tavily.com/search").bearer_auth(&cfg.tavily_key).json(&json!({"query": q, "max_results": 10}));
    let v: Value = json(net, rb).await?;
    Ok(arr(&v["results"]).map(|r| (s(&r["title"]), s(&r["url"]), s(&r["content"]))).collect())
}

pub async fn searx(net: &Net, cfg: &Config, q: &str) -> Result<Vec<Row>> {
    let url = format!("{}/search", cfg.searxng_url.trim_end_matches('/'));
    let v: Value = json(net, net.http.get(url).query(&[("q", q), ("format", "json")])).await?;
    Ok(arr(&v["results"]).map(|r| (s(&r["title"]), s(&r["url"]), s(&r["content"]))).collect())
}

pub async fn ddg(net: &Net, q: &str) -> Result<Vec<Row>> {
    let rb = net.http.post("https://html.duckduckgo.com/html/").form(&[("q", q)]);
    let body = body(net, rb).await?;
    ensure!(!body.contains("anomaly.js") && !body.contains("anomaly-modal"), "DuckDuckGo returned a bot challenge");
    Ok(parse_ddg(&body))
}

fn parse_ddg(body: &str) -> Vec<Row> {
    let doc = Html::parse_document(body);
    let (res, a, sn) =
        (Selector::parse(".result").unwrap(), Selector::parse("a.result__a").unwrap(), Selector::parse(".result__snippet").unwrap());
    doc.select(&res)
        .filter_map(|r| {
            let link = r.select(&a).next()?;
            let url = ddg_target(link.value().attr("href")?)?;
            let snippet = r.select(&sn).next().map(|e| e.text().collect()).unwrap_or_default();
            Some((link.text().collect(), url, snippet))
        })
        .collect()
}

/// DDG wraps results in `//duckduckgo.com/l/?uddg=<target>`; ads go through `y.js`.
fn ddg_target(href: &str) -> Option<String> {
    if href.contains("y.js") {
        return None;
    }
    let full = if href.starts_with("//") { format!("https:{href}") } else { href.to_string() };
    let u = url::Url::parse(&full).ok()?;
    match u.query_pairs().find(|(k, _)| k == "uddg") {
        Some((_, v)) => Some(v.into_owned()),
        None => Some(full),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duckduckgo_unwraps_result_urls_and_omits_ads() {
        let rows = parse_ddg(
            r#"<div class="result"><a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Farticle">Article</a><div class="result__snippet">Evidence</div></div>
            <div class="result"><a class="result__a" href="https://duckduckgo.com/y.js?ad=1">Advertisement</a></div>"#,
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1, "https://example.com/article");
    }
}
