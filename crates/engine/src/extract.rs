use std::sync::LazyLock;

use regex::Regex;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};

/// Max characters of page text kept for the LLM.
pub const MAX_TEXT: usize = 14_000;

#[derive(Clone, Debug, Default)]
pub struct Page {
    pub url: String,
    pub title: String,
    pub text: String,
    pub links: Vec<String>,
    pub published: Option<String>,
}

pub fn page(url: &str, body: &str, html: bool) -> Page {
    if !html {
        return Page { url: url.into(), title: url.into(), text: clip(body), ..Default::default() };
    }
    let links = links(body, url);
    let parsed = dom_smoothie::Readability::new(body, Some(url), None).and_then(|mut r| r.parse());
    let (title, text, published) = match parsed {
        Ok(a) => {
            let md = htmd::convert(&a.content).unwrap_or_else(|_| a.text_content.to_string());
            (a.title.to_string(), md, a.published_time.map(|t| t.to_string()))
        }
        Err(_) => {
            let conv = htmd::HtmlToMarkdown::builder()
                .skip_tags(vec!["script", "style", "nav", "footer", "header", "noscript", "svg", "form"])
                .build();
            (String::new(), conv.convert(body).unwrap_or_default(), None)
        }
    };
    Page { url: url.into(), title, text: clip(&text), links, published }
}

fn clip(s: &str) -> String {
    s.chars().take(MAX_TEXT).collect()
}

pub fn strip_tags(s: &str) -> String {
    static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").unwrap());
    TAG.replace_all(s, "").replace("&amp;", "&").replace("&quot;", "\"").replace("&#39;", "'").trim().to_string()
}

fn links(body: &str, base: &str) -> Vec<String> {
    let Ok(base) = url::Url::parse(base) else { return vec![] };
    let doc = Html::parse_document(body);
    let a = Selector::parse("a[href]").unwrap();
    let mut out: Vec<String> = doc
        .select(&a)
        .filter_map(|e| base.join(e.value().attr("href")?).ok())
        .filter(|u| matches!(u.scheme(), "http" | "https"))
        .map(|mut u| {
            u.set_fragment(None);
            u.to_string()
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Links that look like primary sources: worth fetching directly instead of trusting the summary.
pub fn is_primary_link(url: &str) -> bool {
    let Ok(u) = url::Url::parse(url) else { return false };
    let host = u.host_str().unwrap_or_default();
    let path = u.path().to_ascii_lowercase();
    host.ends_with(".gov")
        || host.contains(".gov.")
        || host.ends_with(".mil")
        || host.ends_with(".edu")
        || host.contains(".ac.")
        || host.ends_with(".int")
        || matches!(host, "doi.org" | "arxiv.org" | "www.courtlistener.com" | "eur-lex.europa.eu")
        || path.ends_with(".txt")
        || path.contains("/filings/")
        || path.contains("/court")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IdKind {
    Email,
    Doi,
    Arxiv,
    Cve,
    Docket,
    Sha256,
    Username,
    Cik,
    Ticker,
    Onion,
    Bitcoin,
    Ethereum,
    Quote,
    Domain,
}

impl IdKind {
    /// How strongly the identifier pins down one real-world thing. Keyword queries sit at 0.35.
    pub fn specificity(self) -> f32 {
        match self {
            IdKind::Doi | IdKind::Arxiv | IdKind::Docket | IdKind::Sha256 => 0.95,
            IdKind::Cve => 0.9,
            IdKind::Email => 0.8,
            IdKind::Cik | IdKind::Onion | IdKind::Bitcoin | IdKind::Ethereum => 0.95,
            IdKind::Quote => 0.9,
            IdKind::Username | IdKind::Domain => 0.75,
            IdKind::Ticker => 0.65,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            IdKind::Email => "email",
            IdKind::Doi => "DOI",
            IdKind::Arxiv => "arXiv",
            IdKind::Cve => "CVE",
            IdKind::Docket => "docket",
            IdKind::Sha256 => "SHA-256",
            IdKind::Username => "username",
            IdKind::Cik => "CIK",
            IdKind::Ticker => "ticker",
            IdKind::Onion => "onion",
            IdKind::Bitcoin => "Bitcoin address",
            IdKind::Ethereum => "Ethereum address",
            IdKind::Quote => "exact quote",
            IdKind::Domain => "domain",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Ident {
    pub kind: IdKind,
    pub value: String,
}

static PATTERNS: LazyLock<Vec<(IdKind, Regex)>> = LazyLock::new(|| {
    [
        (IdKind::Email, r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b"),
        (IdKind::Doi, r"\b10\.\d{4,9}/[^\s<>\[\]()]+"),
        (IdKind::Arxiv, r"(?i)\barxiv[:\s/]+(\d{4}\.\d{4,5})"),
        (IdKind::Cve, r"\bCVE-\d{4}-\d{4,7}\b"),
        (IdKind::Docket, r"\b\d:\d{2}-[a-z]{2}-\d{3,5}\b"),
        (IdKind::Sha256, r"\b[a-fA-F0-9]{64}\b"),
        (IdKind::Cik, r"(?i)\bCIK[:\s]+(\d{1,10})\b"),
        (IdKind::Ticker, r"\b(?:ticker|symbol|NASDAQ|NYSE)[:\s]+([A-Z]{1,5})\b"),
        (IdKind::Onion, r"\b[a-z2-7]{56}\.onion\b"),
        (IdKind::Ethereum, r"\b0x[a-fA-F0-9]{40}\b"),
        (IdKind::Bitcoin, r"\b(?:bc1[a-z0-9]{25,62}|[13][a-km-zA-HJ-NP-Z1-9]{25,34})\b"),
        (IdKind::Username, r"(?:^|[\s(])@([A-Za-z0-9_]{3,32})\b"),
        (IdKind::Quote, r#""([^"\n]{30,160})""#),
        (IdKind::Domain, r"\b(?:[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?\.)+(?:com|org|net|edu|gov|mil|int|io|co|uk)\b"),
    ]
    .into_iter()
    .map(|(k, p)| (k, Regex::new(p).unwrap()))
    .collect()
});

/// Hard identifiers in text: the pivots an OSINT analyst would follow.
pub fn identifiers(text: &str) -> Vec<Ident> {
    let mut out = Vec::new();
    for (kind, re) in PATTERNS.iter() {
        for c in re.captures_iter(text) {
            let m = c.get(1).or_else(|| c.get(0)).unwrap().as_str();
            let value = m.trim_end_matches(['.', ',', ';', ':', '\'', '"']).to_string();
            if *kind == IdKind::Email && (value.ends_with(".png") || value.ends_with(".jpg") || value.contains("example.")) {
                continue;
            }
            let id = Ident { kind: *kind, value };
            if !out.contains(&id) {
                out.push(id);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_identifiers() {
        let t = "See doi 10.1038/s41586-024-07421-0. Filed as 1:23-cv-01234, contact tips@press.org, \
                 CVE-2024-3094, arXiv:2402.14207, and \
                 sha 9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08.";
        let ids = identifiers(t);
        let has = |k: IdKind, v: &str| ids.iter().any(|i| i.kind == k && i.value == v);
        assert!(has(IdKind::Doi, "10.1038/s41586-024-07421-0"), "{ids:?}");
        assert!(has(IdKind::Docket, "1:23-cv-01234"));
        assert!(has(IdKind::Email, "tips@press.org"));
        assert!(has(IdKind::Cve, "CVE-2024-3094"));
        assert!(has(IdKind::Arxiv, "2402.14207"));
        assert!(has(IdKind::Sha256, "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"));
    }

    #[test]
    fn finds_specific_public_identifier_pivots() {
        let onion = "juhanurmihxlp77nkq76byazcldy2hlmovfu2epvl5ankdibsot4csyd.onion";
        let text = format!(
            r#"CIK: 0000320193 NASDAQ: AAPL @research_team {onion}
            0x0123456789abcdef0123456789abcdef01234567 1BoatSLRHtKNngkdXEeobR76b53LETtpyT
            "A sufficiently distinctive exact quotation for research" https://records.nasa.gov/a"#
        );
        let ids = identifiers(&text);
        for (kind, value) in [
            (IdKind::Cik, "0000320193"),
            (IdKind::Ticker, "AAPL"),
            (IdKind::Username, "research_team"),
            (IdKind::Onion, onion),
            (IdKind::Ethereum, "0x0123456789abcdef0123456789abcdef01234567"),
            (IdKind::Bitcoin, "1BoatSLRHtKNngkdXEeobR76b53LETtpyT"),
            (IdKind::Quote, "A sufficiently distinctive exact quotation for research"),
            (IdKind::Domain, "records.nasa.gov"),
        ] {
            assert!(ids.iter().any(|id| id.kind == kind && id.value == value), "missing {kind:?}: {ids:?}");
        }
    }

    #[test]
    fn primary_links() {
        assert!(is_primary_link("https://www.sec.gov/Archives/edgar/data/1/x.htm"));
        assert!(is_primary_link("https://doi.org/10.1/abc"));
        assert!(!is_primary_link("https://www.buzzfeed.com/top-10"));
    }
}
