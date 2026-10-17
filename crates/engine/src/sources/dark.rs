//! Ahmia's filtered result list is the only source of onion reading targets.
use std::collections::HashSet;

use anyhow::{Context, Result, bail};
use scraper::{Html, Selector};
use url::Url;

use super::Row;
use crate::net::{Net, append_body, onion_url};

pub async fn ahmia(net: &Net, q: &str) -> Result<Vec<Row>> {
    // Ahmia publishes a rolling hidden token in its ordinary GET search form.
    let homepage = bounded_text(net.send(net.http.get("https://ahmia.fi/")).await?).await?;
    let (field, token) = search_token(&homepage).context("Ahmia search form token is missing")?;
    let query = q
        .chars()
        .take(100)
        .collect::<String>()
        .split_whitespace()
        .take(10)
        .collect::<Vec<_>>()
        .join(" ");
    let response = net
        .send(
            net.http
                .get("https://ahmia.fi/search/")
                .query(&[("q", query.as_str()), (&field, &token)]),
        )
        .await?;
    let body = bounded_text(response).await?;
    if !Html::parse_document(&body)
        .select(&Selector::parse("#ahmiaResultsPage").unwrap())
        .any(|_| true)
    {
        bail!(
            "Ahmia did not return a results page; the query may be filtered or search is unavailable"
        );
    }
    let rows = parse(&body);
    for (_, target, _) in &rows {
        net.allow_onion(&onion_url(target).context("invalid Ahmia onion target")?);
    }
    Ok(rows)
}

async fn bounded_text(mut response: reqwest::Response) -> Result<String> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        append_body(&mut body, &chunk)?;
    }
    Ok(String::from_utf8_lossy(&body).into_owned())
}

fn search_token(body: &str) -> Option<(String, String)> {
    let document = Html::parse_document(body);
    let inputs = Selector::parse("#searchForm input[type=hidden]").unwrap();
    let input = document.select(&inputs).next()?;
    Some((
        input.value().attr("name")?.to_string(),
        input.value().attr("value")?.to_string(),
    ))
}

fn parse(body: &str) -> Vec<Row> {
    let document = Html::parse_document(body);
    let results = Selector::parse("ol.searchResults li.result").unwrap();
    let anchor = Selector::parse("h4 a").unwrap();
    let description = Selector::parse("p").unwrap();
    let base = Url::parse("https://ahmia.fi/").unwrap();
    let mut seen = HashSet::new();
    document
        .select(&results)
        .filter_map(|result| {
            let link = result.select(&anchor).next()?;
            let href = link.value().attr("href")?;
            let url = if let Some(url) = onion_url(href) {
                url
            } else {
                let redirect = base.join(href).ok()?;
                if redirect.host_str() != Some("ahmia.fi") || redirect.path() != "/search/redirect"
                {
                    return None;
                }
                let target = redirect
                    .query_pairs()
                    .find(|(k, _)| k == "redirect_url")?
                    .1
                    .into_owned();
                onion_url(&target)?
            };
            if !seen.insert(url.to_string()) {
                return None;
            }
            let title = link
                .text()
                .collect::<String>()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let snippet = result
                .select(&description)
                .next()
                .map(|p| p.text().collect())
                .unwrap_or_default();
            Some((title, url.to_string(), snippet))
        })
        .take(8)
        .collect()
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    #[ignore = "requires live Ahmia access and a Tor bootstrap; run explicitly"]
    async fn live_filtered_ahmia_result_over_builtin_tor() -> anyhow::Result<()> {
        live_read(
            "Tor Project",
            "2gzyxa5ihm7nsggfxnu52rck2vv4rvmdlkiu3zzui5du4xyclen53wid.onion",
        )
        .await
    }

    #[tokio::test]
    #[ignore = "requires live Ahmia access and a Tor bootstrap; run explicitly"]
    async fn live_ahmia_mirror_over_builtin_tor() -> anyhow::Result<()> {
        // Published by https://ahmia.fi/; never used as an app discovery seed.
        let official = "juhanurmihxlp77nkq76byazcldy2hlmovfu2epvl5ankdibsot4csyd.onion";
        live_read("Ahmia", official).await
    }

    async fn live_read(query: &str, official: &str) -> anyhow::Result<()> {
        let cfg = crate::Config {
            tor_enabled: true,
            ..Default::default()
        };
        let net = crate::net::Net::for_mode(&cfg, crate::Mode::Dark)?;
        let rows = super::ahmia(&net, query).await?;
        let (_, url, _) = rows
            .iter()
            .find(|(_, raw, _)| {
                crate::net::onion_url(raw)
                    .and_then(|u| u.host_str().map(str::to_owned))
                    .as_deref()
                    == Some(official)
            })
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Ahmia did not return the official onion in its first eight results"
                )
            })?;
        eprintln!(
            "Ahmia returned {} filtered results; reading {official} via embedded Arti",
            rows.len(),
        );
        let page = net.page(url).await?;
        assert!(!page.text.trim().is_empty());
        eprintln!(
            "Read {} text characters from {}",
            page.text.chars().count(),
            page.url
        );
        Ok(())
    }

    #[test]
    fn reads_the_current_public_search_token_without_javascript() {
        let html = r#"<form id="searchForm" action="/search/" method="get"><input type="search" name="q"><input type="hidden" name="abc123" value="456def"></form>"#;
        assert_eq!(
            super::search_token(html),
            Some(("abc123".into(), "456def".into()))
        );
        assert_eq!(super::search_token("<html>blocked</html>"), None);
    }

    #[test]
    fn parses_official_ahmia_template_without_following_tracking_links() {
        let host = "2gzyxa5ihm7nsggfxnu52rck2vv4rvmdlkiu3zzui5du4xyclen53wid.onion";
        let html = format!(
            r#"<a href="http://{host}/outside">not a result</a><ol class="searchResults">
            <li class="result"><h4><a href="/search/redirect?search_term=test&amp;redirect_url=http%3A%2F%2F{host}%2F"> Tor Project </a></h4><p>Research &amp; privacy</p></li>
            <li class="result"><h4><a href="http://{host}/">duplicate</a></h4></li>
            <li class="result"><h4><a href="/search/redirect?redirect_url=https://example.com/">clearnet</a></h4></li>
            <li class="result"><h4><a href="http://invalid.onion/">invalid</a></h4></li>
            <li class="result"><h4><a href="https://evil.example/search/redirect?redirect_url=http://{host}/">other redirector</a></h4></li>
            </ol>"#
        );
        let rows = super::parse(&html);
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0],
            (
                "Tor Project".into(),
                format!("http://{host}/"),
                "Research & privacy".into()
            )
        );
    }
}
