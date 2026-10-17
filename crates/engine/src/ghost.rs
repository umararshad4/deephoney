//! Wayback Machine: resurrect dead pages, date first appearance, and diff out deleted passages.
use serde::{Deserialize, Serialize};

use crate::{
    extract::{self, Page},
    net::Net,
};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Ghost {
    pub url: String,
    pub snapshot: String,
    pub captured: String,
    pub removed: Vec<String>,
}

fn cdx_date(ts: &str) -> String {
    if ts.len() >= 8 { format!("{}-{}-{}", &ts[..4], &ts[4..6], &ts[6..8]) } else { ts.into() }
}

/// Distinct-content captures (collapse=digest), oldest first.
async fn captures(net: &Net, url: &str) -> Vec<String> {
    if !archivable(url) {
        return vec![];
    }
    let rb = net.http.get("https://web.archive.org/cdx/search/cdx").query(&[
        ("url", url),
        ("output", "json"),
        ("fl", "timestamp"),
        ("filter", "statuscode:200"),
        ("collapse", "digest"),
        ("limit", "50"),
    ]);
    let Ok(v) = crate::sources::json(net, rb).await else { return vec![] };
    v.as_array()
        .into_iter()
        .flatten()
        .skip(1) // header row
        .filter_map(|r| r[0].as_str().filter(|ts| ts.len() == 14 && ts.bytes().all(|b| b.is_ascii_digit())).map(String::from))
        .collect()
}

pub(crate) fn archivable(raw: &str) -> bool {
    let Ok(url) = url::Url::parse(raw) else { return false };
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    match url.host() {
        Some(url::Host::Domain(host)) => {
            let host = host.trim_end_matches('.');
            host.contains('.') && ![".onion", ".localhost", ".local", ".test"].iter().any(|suffix| host.ends_with(suffix))
        }
        Some(url::Host::Ipv4(ip)) => {
            !(ip.is_private() || ip.is_loopback() || ip.is_link_local() || ip.is_unspecified() || ip.is_broadcast())
        }
        Some(url::Host::Ipv6(ip)) => !(ip.is_loopback() || ip.is_unspecified() || ip.is_unique_local() || ip.is_unicast_link_local()),
        None => false,
    }
}

async fn snapshot(net: &Net, ts: &str, url: &str) -> Option<Page> {
    // `id_` returns the original bytes without the Wayback toolbar.
    net.page(&format!("https://web.archive.org/web/{ts}id_/{url}")).await.ok()
}

/// A dead link's last archived copy.
pub async fn resurrect(net: &Net, url: &str) -> Option<Page> {
    let ts = captures(net, url).await.pop()?;
    let mut p = snapshot(net, &ts, url).await?;
    p.url = url.into();
    Some(p)
}

/// Compare the oldest distinct capture with today's text. Returns the first-seen date too.
pub async fn diff(net: &Net, url: &str, current: &str) -> (Option<String>, Option<Ghost>) {
    let caps = captures(net, url).await;
    let Some(first) = caps.first() else { return (None, None) };
    let first_seen = Some(cdx_date(first));
    if caps.len() < 2 {
        return (first_seen, None);
    }
    let Some(old) = snapshot(net, first, url).await else { return (first_seen, None) };
    let removed = removed_passages(&old.text, current);
    let ghost = (!removed.is_empty()).then(|| Ghost {
        url: url.into(),
        snapshot: format!("https://web.archive.org/web/{first}/{url}"),
        captured: cdx_date(first),
        removed,
    });
    (first_seen, ghost)
}

fn norm(s: &str) -> String {
    s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect::<Vec<_>>().join(" ")
}

/// Substantial paragraphs present in the old text but gone from the new.
pub fn removed_passages(old: &str, new: &str) -> Vec<String> {
    let new = norm(new);
    old.lines().map(str::trim).filter(|p| p.len() >= 60).filter(|p| !new.contains(&norm(p))).take(8).map(String::from).collect()
}

pub fn page_text(p: &Page) -> &str {
    let _ = extract::MAX_TEXT;
    &p.text
}

#[cfg(test)]
mod tests {
    #[test]
    fn private_and_onion_urls_are_not_sent_to_public_archives() {
        for url in [
            "http://127.0.0.1:8080/a",
            "http://[::1]/a",
            "http://10.0.0.1/a",
            "http://internal.local/a",
            "http://hidden.onion/a",
            "http://hidden.onion./a",
        ] {
            assert!(!super::archivable(url), "{url}");
        }
        assert!(super::archivable("https://www.nasa.gov/report"));
    }

    #[test]
    fn finds_deleted_paragraph() {
        let old = "Intro line that is long enough to be counted as a real paragraph here.\n\
                   We received $2M from Acme Corp to fund this study, which was not disclosed elsewhere.\n";
        let new = "Intro line that is long enough to be counted as a real paragraph here.\n";
        let r = super::removed_passages(old, new);
        assert_eq!(r.len(), 1);
        assert!(r[0].contains("Acme"));
    }
}
