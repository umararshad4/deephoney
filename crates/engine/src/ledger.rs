//! Echo-collapse ledger: claims are scored by *independent* voices, not by repetition.
use std::{
    collections::{HashMap, HashSet},
    hash::{DefaultHasher, Hash, Hasher},
};

use serde::{Deserialize, Serialize};

use crate::Stratum;

/// SimHash bits allowed to differ for two pages to count as one voice (copied / syndicated).
// ponytail: fixed threshold; unrelated pages differ ~32±4 bits, so 10 is ~5σ safe. Tune if real copies split.
const SAME_VOICE_BITS: u32 = 10;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SourceDoc {
    pub url: String,
    pub title: String,
    pub stratum: Stratum,
    pub lens: String,
    /// Index of the first source in this near-duplicate cluster.
    pub cluster: usize,
    pub quality: f32,
    pub published: Option<String>,
    pub first_seen: Option<String>,
    pub simhash: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Claim {
    pub text: String,
    pub quote: String,
    pub lens: String,
    pub sources: Vec<usize>,
    pub contradicts: Vec<usize>,
    /// "supported: …" / "refuted: …" / "unresolved: …" after the debate pass.
    pub verdict: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Ledger {
    #[serde(default)]
    pub lenses: Vec<String>,
    pub sources: Vec<SourceDoc>,
    pub claims: Vec<Claim>,
}

impl Ledger {
    pub fn add_source(&mut self, url: &str, title: &str, text: &str, published: Option<String>, stratum: Stratum, lens: &str) -> usize {
        if let Some(i) = self.sources.iter().position(|s| s.url == url) {
            return i;
        }
        let simhash = simhash(text);
        let cluster = self
            .sources
            .iter()
            .find(|s| (s.simhash ^ simhash).count_ones() <= SAME_VOICE_BITS)
            .map(|s| s.cluster)
            .unwrap_or(self.sources.len());
        self.sources.push(SourceDoc {
            url: url.into(),
            title: title.into(),
            stratum,
            lens: lens.into(),
            cluster,
            quality: quality(url, stratum),
            published: published.map(|p| p.chars().take(10).collect()),
            first_seen: None,
            simhash,
        });
        self.sources.len() - 1
    }

    /// Returns true if the claim is new; otherwise the source is attached to the matching claim.
    pub fn add_claim(&mut self, text: &str, quote: &str, source: usize, lens: &str) -> bool {
        let key = claim_key(text);
        if let Some(c) = self.claims.iter_mut().find(|c| claim_key(&c.text) == key) {
            if !c.sources.contains(&source) {
                c.sources.push(source);
            }
            return false;
        }
        self.claims.push(Claim { text: text.into(), quote: quote.into(), lens: lens.into(), sources: vec![source], ..Default::default() });
        true
    }

    pub fn contradict(&mut self, a: usize, b: usize) {
        if a == b || a >= self.claims.len() || b >= self.claims.len() {
            return;
        }
        for (x, y) in [(a, b), (b, a)] {
            if !self.claims[x].contradicts.contains(&y) {
                self.claims[x].contradicts.push(y);
            }
        }
    }

    pub fn voices(&self, c: &Claim) -> usize {
        c.sources.iter().map(|&s| self.sources[s].cluster).collect::<HashSet<_>>().len()
    }

    pub fn total_voices(&self) -> usize {
        self.sources.iter().map(|s| s.cluster).collect::<HashSet<_>>().len()
    }

    /// Noisy-OR over independent clusters, each weighted by its best source quality.
    pub fn confidence(&self, c: &Claim) -> f32 {
        let mut best: HashMap<usize, f32> = HashMap::new();
        for &s in &c.sources {
            let src = &self.sources[s];
            let q = best.entry(src.cluster).or_default();
            *q = q.max(src.quality);
        }
        let mut conf = 1.0 - best.values().map(|q| 1.0 - 0.55 * q).product::<f32>();
        if !c.contradicts.is_empty() {
            conf *= 0.7;
        }
        match c.verdict.as_deref() {
            Some(v) if v.starts_with("refuted") => conf * 0.25,
            Some(v) if v.starts_with("supported") => conf.max(0.7),
            _ => conf,
        }
    }

    pub fn primary(&self, c: &Claim) -> bool {
        c.sources.iter().any(|&s| self.sources[s].quality >= 0.95)
    }

    /// Earliest date any backing source shows the claim (publication or first Wayback capture).
    pub fn provenance(&self, c: &Claim) -> Option<String> {
        c.sources.iter().flat_map(|&s| [&self.sources[s].published, &self.sources[s].first_seen]).flatten().min().cloned()
    }

    pub fn ranked(&self) -> Vec<usize> {
        let mut idx: Vec<usize> = (0..self.claims.len()).collect();
        idx.sort_by(|&a, &b| self.confidence(&self.claims[b]).total_cmp(&self.confidence(&self.claims[a])));
        idx
    }

    /// Lenses with how many independent voices backed their claims (low = coverage gap).
    pub fn coverage(&self, lenses: &[String]) -> Vec<(String, usize)> {
        lenses
            .iter()
            .map(|l| {
                let clusters: HashSet<usize> =
                    self.claims.iter().filter(|c| &c.lens == l).flat_map(|c| c.sources.iter().map(|&s| self.sources[s].cluster)).collect();
                (l.clone(), clusters.len())
            })
            .collect()
    }
}

fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect()
}

fn claim_key(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase().trim_end_matches(['.', '!', '?']).to_string()
}

/// 64-bit SimHash over 4-word shingles.
pub fn simhash(text: &str) -> u64 {
    let w = words(text);
    let mut acc = [0i32; 64];
    for sh in w.windows(4) {
        let mut h = DefaultHasher::new();
        sh.hash(&mut h);
        let h = h.finish();
        for (bit, a) in acc.iter_mut().enumerate() {
            *a += if h >> bit & 1 == 1 { 1 } else { -1 };
        }
    }
    acc.iter().enumerate().filter(|(_, a)| **a > 0).fold(0, |x, (bit, _)| x | 1 << bit)
}

/// Source quality prior: primary/official > scholarly > press > forums > content farms.
pub fn quality(url: &str, stratum: Stratum) -> f32 {
    let Ok(u) = url::Url::parse(url) else { return 0.5 };
    let host = u.host_str().unwrap_or_default().trim_start_matches("www.");
    let path = u.path().to_ascii_lowercase();
    let primary = host.ends_with(".gov")
        || host.contains(".gov.")
        || host.ends_with(".mil")
        || host.ends_with(".int")
        || ["sec.gov", "courtlistener.com", "doi.org", "arxiv.org", "europa.eu", "nvd.nist.gov"].iter().any(|h| host.ends_with(h));
    if primary {
        return 1.0;
    }
    if host.ends_with(".edu")
        || host.contains(".ac.")
        || ["openalex.org", "semanticscholar.org", "nature.com", "science.org", "pubmed"].iter().any(|h| host.contains(h))
    {
        return 0.9;
    }
    if ["best-", "top-10", "top10", "-vs-", "coupon", "deals", "sponsored", "affiliate", "review"].iter().any(|p| path.contains(p)) {
        return 0.35;
    }
    if ["reddit.com", "ycombinator.com", "stackoverflow.com", "quora.com", "medium.com", "t.me"].iter().any(|h| host.ends_with(h)) {
        return 0.5;
    }
    match stratum {
        Stratum::Buried => 0.7,
        _ => 0.65,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ARTICLE: &str = "The central bank raised interest rates by half a point on Tuesday, citing persistent \
        inflation in housing and services. Officials said further increases were possible if wage growth \
        did not slow. Markets fell sharply after the announcement, with bank stocks leading the decline, \
        and analysts revised their forecasts for the rest of the year.";

    #[test]
    fn copied_article_is_one_voice() {
        let mut l = Ledger::default();
        let a = l.add_source("https://a.com/x", "", ARTICLE, None, Stratum::Surface, "l");
        let copy = format!("Syndicated from Wire. {ARTICLE} Share this story.");
        let b = l.add_source("https://b.com/y", "", &copy, None, Stratum::Surface, "l");
        let other = "Researchers found a new species of frog in the cloud forests of Ecuador, distinguished by \
            its translucent skin and unusual mating call recorded over three rainy seasons by a field team.";
        let c = l.add_source("https://c.com/z", "", other, None, Stratum::Surface, "l");
        assert_eq!(l.sources[a].cluster, l.sources[b].cluster);
        assert_ne!(l.sources[a].cluster, l.sources[c].cluster);

        l.add_claim("The central bank raised rates by half a point", "", a, "l");
        assert!(!l.add_claim("THE central bank raised rates by half a point.", "", b, "l"));
        assert_eq!(l.voices(&l.claims[0]), 1, "a copy must not count as corroboration");
        l.claims[0].sources.push(c);
        assert_eq!(l.voices(&l.claims[0]), 2);
    }

    #[test]
    fn independence_beats_repetition() {
        let mut l = Ledger::default();
        let gov = l.add_source("https://www.sec.gov/f", "", "filing text one two three four five", None, Stratum::Deep, "l");
        l.add_claim("claim", "", gov, "l");
        let farm = l.add_source("https://x.com/best-10-picks", "", "farm text", None, Stratum::Surface, "l");
        l.add_claim("other thing", "", farm, "l");
        assert!(l.confidence(&l.claims[0]) > l.confidence(&l.claims[1]));
        assert!(l.primary(&l.claims[0]));
    }

    #[test]
    fn preserves_claims_that_differ_by_negation_or_a_number() {
        let mut ledger = Ledger::default();
        let source = ledger.add_source("https://example.org", "", ARTICLE, None, Stratum::Surface, "l");
        for text in [
            "The company reported revenue of 20 million dollars in 2024",
            "The company reported revenue of 30 million dollars in 2024",
            "The company did meet its annual safety targets in 2024",
            "The company did not meet its annual safety targets in 2024",
        ] {
            assert!(ledger.add_claim(text, text, source, "l"), "discarded distinct claim: {text}");
        }
        assert_eq!(ledger.claims.len(), 4);
    }

    #[test]
    fn identical_wording_across_sources_still_corroborates() {
        let mut ledger = Ledger::default();
        let a = ledger.add_source("https://a.org", "", ARTICLE, None, Stratum::Surface, "l");
        let b = ledger.add_source(
            "https://b.org",
            "",
            "An unrelated document with different language about wildlife habitat.",
            None,
            Stratum::Deep,
            "l",
        );
        assert!(ledger.add_claim("The launch succeeded in 2024.", "launch succeeded", a, "l"));
        assert!(!ledger.add_claim("THE launch succeeded\nin 2024!", "launch succeeded", b, "l"));
        assert_eq!(ledger.claims.len(), 1);
        assert_eq!(ledger.voices(&ledger.claims[0]), 2);
    }
}
