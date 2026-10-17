use std::sync::LazyLock;

use anyhow::Result;
use regex::Regex;
use serde_json::Value;

use super::{Row, arr, body, json, s};
use crate::{Config, net::Net};

pub async fn openalex(net: &Net, q: &str) -> Result<Vec<Row>> {
    let rb = net.http.get("https://api.openalex.org/works").query(&[("search", q), ("per-page", "8")]);
    let v: Value = json(net, rb).await?;
    Ok(arr(&v["results"])
        .map(|w| {
            let url = [&w["doi"], &w["primary_location"]["landing_page_url"], &w["id"]]
                .into_iter()
                .find_map(|u| u.as_str())
                .unwrap_or_default()
                .to_string();
            let year = w["publication_year"].as_i64().map(|y| format!("({y}) ")).unwrap_or_default();
            (s(&w["display_name"]), url, format!("{year}{}", abstract_text(&w["abstract_inverted_index"])))
        })
        .collect())
}

/// OpenAlex ships abstracts as {word: [positions]}.
fn abstract_text(inv: &Value) -> String {
    let Some(map) = inv.as_object() else { return String::new() };
    let mut words: Vec<(i64, &str)> =
        map.iter().flat_map(|(w, pos)| arr(pos).filter_map(Value::as_i64).map(move |p| (p, w.as_str()))).collect();
    words.sort_unstable();
    words.into_iter().map(|(_, w)| w).collect::<Vec<_>>().join(" ")
}

pub async fn semantic_scholar(net: &Net, q: &str) -> Result<Vec<Row>> {
    let rb = net.http.get("https://api.semanticscholar.org/graph/v1/paper/search").query(&[
        ("query", q),
        ("limit", "8"),
        ("fields", "title,url,abstract,year"),
    ]);
    let v: Value = json(net, rb).await?;
    Ok(arr(&v["data"]).map(|p| (s(&p["title"]), s(&p["url"]), s(&p["abstract"]))).collect())
}

pub async fn arxiv(net: &Net, q: &str) -> Result<Vec<Row>> {
    static ENTRY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<entry>(.*?)</entry>").unwrap());
    static FIELD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<(id|title|summary)>(.*?)</").unwrap());
    let sq = format!("all:{q}");
    let rb = net.http.get("https://export.arxiv.org/api/query").query(&[("search_query", sq.as_str()), ("max_results", "8")]);
    let xml = body(net, rb).await?;
    Ok(ENTRY
        .captures_iter(&xml)
        .map(|e| {
            let (mut id, mut title, mut sum) = (String::new(), String::new(), String::new());
            for f in FIELD.captures_iter(&e[1]) {
                let v = f[2].split_whitespace().collect::<Vec<_>>().join(" ");
                match &f[1] {
                    "id" => id = v,
                    "title" => title = v,
                    _ => sum = v,
                }
            }
            (title, id, sum)
        })
        .collect())
}

pub async fn crossref(net: &Net, q: &str) -> Result<Vec<Row>> {
    let rb = net.http.get("https://api.crossref.org/works").query(&[("query", q), ("rows", "8"), ("select", "DOI,title,URL,abstract")]);
    let v: Value = json(net, rb).await?;
    Ok(arr(&v["message"]["items"]).map(|w| (s(&w["title"][0]), s(&w["URL"]), s(&w["abstract"]))).collect())
}

pub async fn hacker_news(net: &Net, q: &str) -> Result<Vec<Row>> {
    let rb = net.http.get("https://hn.algolia.com/api/v1/search").query(&[("query", q), ("hitsPerPage", "8")]);
    let v: Value = json(net, rb).await?;
    Ok(arr(&v["hits"])
        .map(|h| {
            let title = h["title"].as_str().or(h["story_title"].as_str()).unwrap_or("HN comment").to_string();
            let url = h["url"]
                .as_str()
                .map(String::from)
                .unwrap_or_else(|| format!("https://news.ycombinator.com/item?id={}", s(&h["objectID"])));
            let text = h["story_text"].as_str().or(h["comment_text"].as_str()).unwrap_or_default().to_string();
            (title, url, text)
        })
        .collect())
}

pub async fn stack_exchange(net: &Net, q: &str) -> Result<Vec<Row>> {
    let rb = net.http.get("https://api.stackexchange.com/2.3/search/advanced").query(&[
        ("order", "desc"),
        ("sort", "relevance"),
        ("q", q),
        ("site", "stackoverflow"),
        ("pagesize", "8"),
    ]);
    let v: Value = json(net, rb).await?;
    Ok(arr(&v["items"]).map(|i| (s(&i["title"]), s(&i["link"]), String::new())).collect())
}

pub async fn edgar(net: &Net, q: &str) -> Result<Vec<Row>> {
    let quoted = format!("\"{q}\"");
    let rb = net.http.get("https://efts.sec.gov/LATEST/search-index").query(&[("q", quoted.as_str())]);
    let v: Value = json(net, rb).await?;
    Ok(arr(&v["hits"]["hits"])
        .filter_map(|h| {
            let (adsh, file) = h["_id"].as_str()?.split_once(':')?;
            let src = &h["_source"];
            let cik = src["ciks"][0].as_str()?.trim_start_matches('0');
            let url = format!("https://www.sec.gov/Archives/edgar/data/{cik}/{}/{file}", adsh.replace('-', ""));
            let title = format!("{} — {} ({})", s(&src["display_names"][0]), s(&src["form"]), s(&src["file_date"]));
            Some((title, url, String::new()))
        })
        .collect())
}

pub async fn github(net: &Net, cfg: &Config, q: &str) -> Result<Vec<Row>> {
    let rb = net
        .http
        .get("https://api.github.com/search/code")
        .query(&[("q", q), ("per_page", "8")])
        .bearer_auth(&cfg.github_token)
        .header("Accept", "application/vnd.github+json");
    let v: Value = json(net, rb).await?;
    Ok(arr(&v["items"])
        .map(|i| (format!("{}/{}", s(&i["repository"]["full_name"]), s(&i["path"])), s(&i["html_url"]), String::new()))
        .collect())
}

pub async fn courtlistener(net: &Net, cfg: &Config, q: &str) -> Result<Vec<Row>> {
    let rb = net
        .http
        .get("https://www.courtlistener.com/api/rest/v4/search/")
        .query(&[("q", q)])
        .header("Authorization", format!("Token {}", cfg.courtlistener_token));
    let v: Value = json(net, rb).await?;
    Ok(arr(&v["results"])
        .map(|r| {
            let title = format!("{} ({})", s(&r["caseName"]), s(&r["dateFiled"]));
            let url = format!("https://www.courtlistener.com{}", s(&r["absolute_url"]));
            (title, url, s(&r["opinions"][0]["snippet"]))
        })
        .collect())
}

pub async fn crtsh(net: &Net, q: &str) -> Result<Vec<Row>> {
    let Some(url) = super::lookup_url(q) else { return Ok(vec![]) };
    let host = url.host_str().unwrap_or_default();
    let value = json(net, net.http.get("https://crt.sh/").query(&[("q", host), ("output", "json")])).await?;
    Ok(certificate_rows(&value))
}

fn certificate_rows(value: &Value) -> Vec<Row> {
    let mut seen = std::collections::HashSet::new();
    arr(value)
        .filter_map(|record| {
            let id = record["id"].as_u64()?;
            if !seen.insert(id) {
                return None;
            }
            // Cite the actual certificate record, not a discovered host as if it were evidence.
            Some((
                format!("Certificate {}", s(&record["name_value"])),
                format!("https://crt.sh/?id={id}"),
                format!(
                    "Issuer: {}; logged: {}; validity: {} to {}",
                    s(&record["issuer_name"]),
                    s(&record["entry_timestamp"]),
                    s(&record["not_before"]),
                    s(&record["not_after"])
                ),
            ))
        })
        .take(8)
        .collect()
}

pub async fn muckrock(net: &Net, q: &str) -> Result<Vec<Row>> {
    // Public request pages remain readable; API request searches now require authentication.
    let html = body(net, net.http.get("https://www.muckrock.com/foi/list/").query(&[("q", q)])).await?;
    Ok(muckrock_rows(&html))
}

fn muckrock_rows(html: &str) -> Vec<Row> {
    let doc = scraper::Html::parse_document(html);
    let selector = scraper::Selector::parse("a[href^='/foi/']").unwrap();
    let mut seen = std::collections::HashSet::new();
    doc.select(&selector)
        .filter_map(|link| {
            let href = link.value().attr("href")?;
            // Detail URLs contain jurisdiction and slug-id; omit create/list/feed controls.
            let segments: Vec<_> = href.trim_matches('/').split('/').collect();
            if segments.len() != 3 || segments[2].rsplit('-').next()?.parse::<u64>().is_err() || !seen.insert(href) {
                return None;
            }
            let title = link.text().collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ");
            (!title.is_empty()).then(|| (title, format!("https://www.muckrock.com{href}"), String::new()))
        })
        .take(8)
        .collect()
}

/// Telegram offers public per-channel previews, not a keyless global keyword-search API.
pub async fn telegram(net: &Net, q: &str) -> Result<Vec<Row>> {
    let Some((channel, words)) = telegram_query(q) else { return Ok(vec![]) };
    let url = format!("https://t.me/s/{channel}");
    let html = body(net, net.http.get(url).query(&[("q", words)])).await?;
    Ok(telegram_rows(&html, &channel))
}

fn telegram_query(q: &str) -> Option<(String, &str)> {
    let (first, rest) = q.trim().split_once(char::is_whitespace).unwrap_or((q.trim(), ""));
    let channel = if let Some(name) = first.strip_prefix('@') {
        name.to_string()
    } else {
        let url = super::lookup_url(first)?;
        if !matches!(url.host_str(), Some("t.me" | "telegram.me")) {
            return None;
        }
        url.path().trim_start_matches('/').strip_prefix("s/").unwrap_or(url.path().trim_start_matches('/')).split('/').next()?.to_string()
    };
    (channel.len() >= 5
        && channel.len() <= 32
        && channel.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && channel.starts_with(|c: char| c.is_ascii_alphabetic()))
    .then_some((channel, rest.trim()))
}

fn telegram_rows(html: &str, channel: &str) -> Vec<Row> {
    let doc = scraper::Html::parse_document(html);
    let messages = scraper::Selector::parse(".tgme_widget_message[data-post]").unwrap();
    let text = scraper::Selector::parse(".tgme_widget_message_text").unwrap();
    doc.select(&messages)
        .filter_map(|message| {
            let post = message.value().attr("data-post")?;
            let (owner, id) = post.split_once('/')?;
            if owner != channel || id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let snippet = message.select(&text).next()?.text().collect::<String>();
            (!snippet.trim().is_empty()).then(|| (format!("@{owner} — post {id}"), format!("https://t.me/s/{owner}/{id}"), snippet))
        })
        .take(8)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn certificate_results_cite_records_and_deduplicate() {
        let rows = certificate_rows(&json!([{"id":12,"name_value":"example.com"},{"id":12},{"id":"bad"}]));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1, "https://crt.sh/?id=12");
    }

    #[test]
    fn public_request_parser_omits_controls() {
        let rows = muckrock_rows(
            r#"<a href='/foi/create/'>Create</a><a href='/foi/list/'>List</a>
            <a href='/foi/texas-109/public-records-123/'> Public records </a>
            <a href='/foi/texas-109/public-records-123/'>Duplicate</a>"#,
        );
        assert_eq!(rows, vec![("Public records".into(), "https://www.muckrock.com/foi/texas-109/public-records-123/".into(), "".into())]);
    }

    #[test]
    fn telegram_accepts_public_channels_and_uses_text_posts_only() {
        assert_eq!(telegram_query("@telegram privacy"), Some(("telegram".into(), "privacy")));
        assert_eq!(telegram_query("https://t.me/s/telegram privacy"), Some(("telegram".into(), "privacy")));
        for query in ["keyword search", "https://evil.test/telegram", "https://t.me/+private", "@a/b"] {
            assert!(telegram_query(query).is_none(), "{query}");
        }
        let rows = telegram_rows(
            r#"<div class='tgme_widget_message' data-post='telegram/123'>
            <div class='tgme_widget_message_text'>Public announcement</div></div>
            <div class='tgme_widget_message' data-post='other/1'><div class='tgme_widget_message_text'>Wrong channel</div></div>"#,
            "telegram",
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1, "https://t.me/s/telegram/123");
    }

    #[test]
    fn reconstructs_inverted_abstract() {
        assert_eq!(abstract_text(&json!({"distributed":[0],"systems":[1]})), "distributed systems");
    }
}
