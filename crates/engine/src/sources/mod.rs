//! Search backends grouped by stratum. The planner picks which deep sources fit the topic.
mod buried;
mod dark;
mod deep;
mod surface;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Config, Mode, Stratum, extract::strip_tags, net::Net};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Hit {
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub source: String,
    pub stratum: Stratum,
}

type Row = (String, String, String);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Brave,
    Exa,
    Tavily,
    Searx,
    Ddg,
    OpenAlex,
    SemanticScholar,
    Arxiv,
    Crossref,
    HackerNews,
    StackExchange,
    Edgar,
    GitHub,
    CourtListener,
    CrtSh,
    MuckRock,
    Telegram,
    CommonCrawl,
    Wayback,
    Ahmia,
}

impl Source {
    pub const ALL: [Source; 20] = [
        Source::Brave,
        Source::Exa,
        Source::Tavily,
        Source::Searx,
        Source::Ddg,
        Source::OpenAlex,
        Source::SemanticScholar,
        Source::Arxiv,
        Source::Crossref,
        Source::HackerNews,
        Source::StackExchange,
        Source::Edgar,
        Source::GitHub,
        Source::CourtListener,
        Source::CrtSh,
        Source::MuckRock,
        Source::Telegram,
        Source::CommonCrawl,
        Source::Wayback,
        Source::Ahmia,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Source::Brave => "brave",
            Source::Exa => "exa",
            Source::Tavily => "tavily",
            Source::Searx => "searxng",
            Source::Ddg => "duckduckgo",
            Source::OpenAlex => "openalex",
            Source::SemanticScholar => "semanticscholar",
            Source::Arxiv => "arxiv",
            Source::Crossref => "crossref",
            Source::HackerNews => "hackernews",
            Source::StackExchange => "stackexchange",
            Source::Edgar => "sec_edgar",
            Source::GitHub => "github_code",
            Source::CourtListener => "courtlistener",
            Source::CrtSh => "crtsh",
            Source::MuckRock => "muckrock",
            Source::Telegram => "telegram",
            Source::CommonCrawl => "commoncrawl",
            Source::Wayback => "wayback",
            Source::Ahmia => "ahmia",
        }
    }

    /// One line for the planner so it can route queries.
    pub fn about(self) -> &'static str {
        match self {
            Source::OpenAlex => "250M scholarly works, all fields",
            Source::SemanticScholar => "scholarly papers with abstracts",
            Source::Arxiv => "physics/CS/math/bio preprints",
            Source::Crossref => "DOI metadata for journals, books, reports",
            Source::HackerNews => "tech community discussion and insider comments",
            Source::StackExchange => "practitioner Q&A",
            Source::Edgar => "US SEC filings full text: companies, deals, risk disclosures",
            Source::GitHub => "source code, configs, leaked-by-accident strings",
            Source::CourtListener => "US court opinions and dockets",
            Source::CrtSh => "certificate transparency: query an exact domain, not topic words",
            Source::MuckRock => "public freedom-of-information requests and released records",
            Source::Telegram => "public Telegram preview: query @channel or t.me/channel, optional search words",
            Source::CommonCrawl => "URL/domain lookup in latest Common Crawl index; discovers current pages, NOT archived evidence",
            Source::Wayback => "archived HTML/plain-text captures: query an exact URL or domain",
            Source::Ahmia => "filtered onion discovery, fetched through Tor only",
            _ => "web search",
        }
    }

    pub fn stratum(self) -> Stratum {
        match self {
            Source::Brave | Source::Exa | Source::Tavily | Source::Searx | Source::Ddg => Stratum::Surface,
            Source::Wayback => Stratum::Buried,
            Source::Ahmia => Stratum::Dark,
            _ => Stratum::Deep,
        }
    }

    fn enabled(self, cfg: &Config) -> bool {
        match self {
            Source::Brave => !cfg.brave_key.is_empty(),
            Source::Exa => !cfg.exa_key.is_empty(),
            Source::Tavily => !cfg.tavily_key.is_empty(),
            Source::Searx => !cfg.searxng_url.is_empty(),
            // Keyless fallback only, scraping is best-effort.
            Source::Ddg => [&cfg.brave_key, &cfg.exa_key, &cfg.tavily_key, &cfg.searxng_url].iter().all(|k| k.is_empty()),
            Source::GitHub => !cfg.github_token.is_empty(),
            Source::CourtListener => !cfg.courtlistener_token.is_empty(),
            Source::Edgar => !cfg.contact.trim().is_empty(),
            Source::Ahmia => cfg.tor_enabled,
            _ => true,
        }
    }

    pub fn for_mode(mode: Mode, cfg: &Config) -> Vec<Source> {
        Source::ALL.into_iter().filter(|s| mode.strata().contains(&s.stratum()) && s.enabled(cfg)).collect()
    }

    /// Engines that understand `site:` / `filetype:` operators.
    fn takes_operators(self) -> bool {
        matches!(self, Source::Brave | Source::Ddg | Source::Searx)
    }

    pub async fn search(self, net: &Net, cfg: &Config, q: &str) -> Result<Vec<Hit>> {
        let q = if self.takes_operators() || matches!(self, Source::CrtSh | Source::Telegram | Source::CommonCrawl | Source::Wayback) {
            q.to_string()
        } else {
            plain(q)
        };
        let rows = match self {
            Source::Brave => surface::brave(net, cfg, &q).await,
            Source::Exa => surface::exa(net, cfg, &q).await,
            Source::Tavily => surface::tavily(net, cfg, &q).await,
            Source::Searx => surface::searx(net, cfg, &q).await,
            Source::Ddg => surface::ddg(net, &q).await,
            Source::OpenAlex => deep::openalex(net, &q).await,
            Source::SemanticScholar => deep::semantic_scholar(net, &q).await,
            Source::Arxiv => deep::arxiv(net, &q).await,
            Source::Crossref => deep::crossref(net, &q).await,
            Source::HackerNews => deep::hacker_news(net, &q).await,
            Source::StackExchange => deep::stack_exchange(net, &q).await,
            Source::Edgar => deep::edgar(net, &q).await,
            Source::GitHub => deep::github(net, cfg, &q).await,
            Source::CourtListener => deep::courtlistener(net, cfg, &q).await,
            Source::CrtSh => deep::crtsh(net, &q).await,
            Source::MuckRock => deep::muckrock(net, &q).await,
            Source::Telegram => deep::telegram(net, &q).await,
            Source::CommonCrawl => buried::common_crawl(net, &q).await,
            Source::Wayback => buried::wayback(net, &q).await,
            Source::Ahmia => dark::ahmia(net, &q).await,
        }?;
        Ok(rows
            .into_iter()
            .filter(|(_, raw, _)| {
                let Ok(url) = url::Url::parse(raw) else { return false };
                matches!(url.scheme(), "http" | "https")
                    && url.host_str().is_some()
                    && url.username().is_empty()
                    && url.password().is_none()
                    && (self != Source::Ahmia || crate::net::onion_url(raw).is_some())
            })
            .take(8)
            .map(|(title, url, snippet)| Hit {
                title: strip_tags(&title),
                snippet: strip_tags(&snippet).chars().take(400).collect(),
                url,
                source: self.name().into(),
                stratum: self.stratum(),
            })
            .collect())
    }

    pub fn by_name(name: &str) -> Option<Source> {
        Source::ALL.into_iter().find(|s| s.name() == name)
    }
}

/// Drop search operators for APIs that would treat them as literal text.
pub fn plain(q: &str) -> String {
    q.split_whitespace()
        .filter(|t| {
            !["site:", "filetype:", "inbody:", "intitle:", "inurl:"].iter().any(|op| t.starts_with(op))
                && !t.starts_with('-')
                && *t != "NOT"
                && *t != "OR"
        })
        .map(|t| t.trim_matches('"'))
        .collect::<Vec<_>>()
        .join(" ")
}

fn s(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

fn arr(v: &Value) -> impl Iterator<Item = &Value> {
    v.as_array().into_iter().flatten()
}

/// Search APIs share the same body limit as page fetches.
async fn body(net: &Net, request: reqwest::RequestBuilder) -> Result<String> {
    net.search_body(request).await
}

pub(crate) async fn json(net: &Net, request: reqwest::RequestBuilder) -> Result<Value> {
    Ok(serde_json::from_str(&body(net, request).await?)?)
}

/// Exact domain/URL queries only: archive and certificate indexes are not keyword engines.
fn lookup_url(query: &str) -> Option<url::Url> {
    let query = query.trim();
    if query.chars().any(char::is_whitespace) {
        return None;
    }
    let raw = if query.contains("://") { query.to_string() } else { format!("https://{query}") };
    let url = url::Url::parse(&raw).ok()?;
    url.host_str()?;
    (matches!(url.scheme(), "https" | "http")
        && crate::ghost::archivable(url.as_str())
        && url.username().is_empty()
        && url.password().is_none())
    .then_some(url)
}

#[cfg(test)]
mod tests {
    #[test]
    fn plain_strips_operators() {
        assert_eq!(super::plain(r#""lithium mine" site:gov filetype:pdf -reddit"#), "lithium mine");
        assert_eq!(super::plain("https://doi.org/10.123/example site:gov"), "https://doi.org/10.123/example");
    }

    #[test]
    fn mode_and_credentials_gate_source_families() {
        use super::*;
        let mut cfg = Config::default();
        let normal = Source::for_mode(Mode::Normal, &cfg);
        assert!(normal.contains(&Source::CommonCrawl));
        assert!(!normal.contains(&Source::Wayback));
        assert!(!normal.contains(&Source::Ahmia));
        assert!(!normal.contains(&Source::Edgar));
        assert!(!normal.contains(&Source::GitHub));
        assert!(!normal.contains(&Source::CourtListener));
        assert!(normal.contains(&Source::Ddg));
        assert!(Source::for_mode(Mode::Hidden, &cfg).contains(&Source::Wayback));
        assert!(!Source::for_mode(Mode::Dark, &cfg).contains(&Source::Ahmia));
        cfg.tor_enabled = true;
        cfg.contact = "researcher@example.test".into();
        cfg.brave_key = "test-key".into();
        let dark = Source::for_mode(Mode::Dark, &cfg);
        assert!(dark.contains(&Source::Ahmia));
        assert!(dark.contains(&Source::Edgar));
        assert!(dark.contains(&Source::Brave));
        assert!(!dark.contains(&Source::Ddg));
        assert_eq!(Source::CommonCrawl.stratum(), Stratum::Deep);
        assert_eq!(Source::Wayback.stratum(), Stratum::Buried);
        assert_eq!(Source::Ahmia.stratum(), Stratum::Dark);
    }

    #[test]
    fn domain_indexes_reject_keywords_and_non_web_inputs() {
        assert_eq!(super::lookup_url("example.com").unwrap().as_str(), "https://example.com/");
        for query in [
            "distributed systems",
            "file:///etc/passwd",
            "https://user:pass@example.com",
            "http://example.onion/",
            "http://example.onion./",
            "http://127.0.0.1/",
            "http://internal.local/",
        ] {
            assert!(super::lookup_url(query).is_none(), "{query}");
        }
    }

    async fn serve(response_body: String) -> String {
        use tokio::{
            io::{AsyncReadExt, AsyncWriteExt},
            net::TcpListener,
        };
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            let _ = socket.read(&mut request).await;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response_body}",
                response_body.len()
            );
            let _ = socket.write_all(response.as_bytes()).await;
        });
        format!("http://{address}")
    }

    #[tokio::test]
    async fn search_results_reject_non_http_and_credential_urls() {
        use super::*;
        let endpoint = serve(
            serde_json::json!({"results": [
                {"title":"valid", "url":"https://example.com/article"},
                {"title":"invalid", "url":"httpx://example.com/"},
                {"title":"invalid", "url":"https://user:password@example.com/"},
                {"title":"invalid", "url":"javascript:alert(1)"}
            ]})
            .to_string(),
        )
        .await;
        let cfg = Config { searxng_url: endpoint, ..Config::default() };
        let hits = Source::Searx.search(&Net::new(&cfg), &cfg, "topic").await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].url, "https://example.com/article");
    }

    #[tokio::test]
    async fn search_bodies_are_bounded() {
        use super::*;
        let endpoint = serve("x".repeat(crate::net::MAX_BODY + 1)).await;
        let cfg = Config::default();
        let net = Net::new(&cfg);
        assert!(body(&net, net.http.get(endpoint)).await.unwrap_err().to_string().contains("body limit"));
    }

    /// Explicitly opt in: exercises adapters without reading keys or Keychain.
    #[tokio::test]
    #[ignore = "live public services are rate-limited and may be unavailable"]
    async fn public_source_smoke() {
        use super::*;
        let cfg = Config::default();
        let net = Net::new(&cfg);
        use futures::{StreamExt, stream};
        let sources = [
            Source::Ddg,
            Source::OpenAlex,
            Source::SemanticScholar,
            Source::Arxiv,
            Source::Crossref,
            Source::HackerNews,
            Source::StackExchange,
            Source::CrtSh,
            Source::MuckRock,
            Source::Telegram,
            Source::CommonCrawl,
            Source::Wayback,
        ];
        stream::iter(sources)
            .map(|source| {
                let net = &net;
                let cfg = &cfg;
                async move {
                    let query = match source {
                        Source::CrtSh | Source::CommonCrawl | Source::Wayback => "example.com",
                        Source::Telegram => "@telegram privacy",
                        _ => "distributed systems",
                    };
                    match source.search(net, cfg, query).await {
                        Ok(rows) => eprintln!(
                            "{}: {} results{}",
                            source.name(),
                            rows.len(),
                            rows.first().map(|r| format!("; {}", r.url)).unwrap_or_default()
                        ),
                        Err(error) => eprintln!("{}: unavailable: {error:#}", source.name()),
                    }
                }
            })
            .buffer_unordered(4)
            .collect::<Vec<_>>()
            .await;
    }
}
