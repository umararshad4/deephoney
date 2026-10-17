//! Repeatable evaluation artifacts. These checks measure structure and literal coverage, not truth.
use std::{collections::HashSet, fs::OpenOptions, io::Write, path::Path, sync::LazyLock};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::{Mode, Report, Request, RunMetrics};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub id: String,
    pub topic: String,
    #[serde(default)]
    pub mode: Mode,
    pub depth: usize,
    pub breadth: usize,
    pub max_pages: usize,
    pub timeout_secs: u64,
    #[serde(default)]
    pub expected: Expectations,
}

/// Optional reference strings supplied by an evaluator, not model-generated gold answers.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Expectations {
    pub facts: Vec<String>,
    pub source_urls: Vec<String>,
    pub forbidden_assertions: Vec<String>,
}

pub fn load_cases(path: &Path) -> Result<Vec<Case>> {
    let cases: Vec<Case> = serde_json::from_reader(std::fs::File::open(path).context("open evaluation cases")?)?;
    validate_cases(&cases)?;
    Ok(cases)
}

pub fn validate_cases(cases: &[Case]) -> Result<()> {
    if cases.is_empty() || cases.len() > 100 {
        bail!("a suite must contain 1..100 cases");
    }
    let mut ids = HashSet::new();
    for case in cases {
        if case.id.is_empty()
            || case.id.len() > 80
            || !case.id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
            || !case.id.as_bytes()[0].is_ascii_alphanumeric()
        {
            bail!(
                "unsafe case ID {:?}: use 1..80 lowercase ASCII letters, digits, hyphens or underscores, starting with a letter or digit",
                case.id
            );
        }
        if ["cases", "results", "run"].contains(&case.id.as_str()) {
            bail!("reserved case ID {} conflicts with evaluation artifacts", case.id);
        }
        if !ids.insert(&case.id) {
            bail!("duplicate case ID {}", case.id);
        }
        if case.topic.trim().is_empty() || case.topic.chars().count() > 10_000 {
            bail!("{}: topic must contain 1..10000 characters", case.id);
        }
        if !(1..=5).contains(&case.depth)
            || !(1..=6).contains(&case.breadth)
            || !(1..=1000).contains(&case.max_pages)
            || !(1..=3600).contains(&case.timeout_secs)
        {
            bail!("{}: depth 1..5, breadth 1..6, max_pages 1..1000, timeout_secs 1..3600 required", case.id);
        }
        for value in case.expected.facts.iter().chain(&case.expected.source_urls).chain(&case.expected.forbidden_assertions) {
            if value.trim().is_empty() || value.chars().count() > 10_000 {
                bail!("{}: expectation strings must contain 1..10000 characters", case.id);
            }
        }
        for raw in &case.expected.source_urls {
            if !http_url(raw) {
                bail!("{}: expected source URL must use HTTP(S), a host and no credentials", case.id);
            }
        }
    }
    Ok(())
}

impl Case {
    pub fn request(&self) -> Request {
        Request { topic: self.topic.clone(), mode: self.mode, depth: self.depth, breadth: self.breadth, plan: None }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct StructuralChecks {
    pub sources: usize,
    pub invalid_source_urls: usize,
    pub claims: usize,
    pub claims_with_source_refs: usize,
    pub invalid_claim_source_refs: usize,
    pub invalid_evidence_source_refs: usize,
    pub claims_with_recorded_quotes: usize,
    pub invalid_contradiction_refs: usize,
    pub asymmetric_contradiction_refs: usize,
    pub conflicting_claims: usize,
    pub conflicting_claims_without_verdict: usize,
    pub conflicting_claims_marked_supported: usize,
    pub numeric_citation_occurrences: usize,
    pub invalid_numeric_citation_occurrences: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LiteralChecks {
    pub expected_facts: usize,
    pub matched_fact_strings: usize,
    pub missing_fact_strings: Vec<String>,
    pub expected_source_urls: usize,
    pub matched_source_urls: usize,
    pub missing_source_urls: Vec<String>,
    pub forbidden_string_matches: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Outcome {
    pub case_id: String,
    pub topic: String,
    /// "completed" means a report was graded, not that its conclusions were correct.
    pub status: String,
    pub error: Option<String>,
    pub structural: Option<StructuralChecks>,
    pub literal_checks: Option<LiteralChecks>,
    pub metrics: Option<RunMetrics>,
}

impl Outcome {
    pub fn failed(case: &Case, error: impl std::fmt::Display) -> Self {
        Self {
            case_id: case.id.clone(),
            topic: case.topic.clone(),
            status: "error".into(),
            error: Some(error.to_string()),
            structural: None,
            literal_checks: None,
            metrics: None,
        }
    }
}

pub fn grade(case: &Case, report: &Report) -> Outcome {
    let mut structural =
        StructuralChecks { sources: report.ledger.sources.len(), claims: report.ledger.claims.len(), ..Default::default() };
    structural.invalid_source_urls = report.ledger.sources.iter().filter(|s| !http_url(&s.url)).count();
    for (id, claim) in report.ledger.claims.iter().enumerate() {
        structural.claims_with_source_refs += usize::from(!claim.sources.is_empty());
        structural.invalid_claim_source_refs += claim.sources.iter().filter(|&&s| s >= structural.sources).count();
        structural.invalid_evidence_source_refs +=
            claim.evidence.iter().filter(|e| e.source >= structural.sources || !claim.sources.contains(&e.source)).count();
        let has_quote = !claim.quote.trim().is_empty() || claim.evidence.iter().any(|e| !e.quote.trim().is_empty());
        structural.claims_with_recorded_quotes += usize::from(has_quote);
        if !claim.contradicts.is_empty() {
            structural.conflicting_claims += 1;
            structural.conflicting_claims_without_verdict += usize::from(claim.verdict.as_deref().is_none_or(|v| v.trim().is_empty()));
            structural.conflicting_claims_marked_supported +=
                usize::from(claim.verdict.as_deref().is_some_and(|v| v.starts_with("supported")));
        }
        for &other in &claim.contradicts {
            if other == id || other >= structural.claims {
                structural.invalid_contradiction_refs += 1;
            } else if !report.ledger.claims[other].contradicts.contains(&id) {
                structural.asymmetric_contradiction_refs += 1;
            }
        }
    }
    static CITATIONS: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"\[(S?\d+(?:\s*,\s*S?\d+)*)\]").expect("literal citation pattern"));
    for matched in CITATIONS.captures_iter(&report.markdown) {
        structural.numeric_citation_occurrences += 1;
        let valid = matched[1]
            .split(',')
            .all(|id| id.trim().trim_start_matches('S').parse::<usize>().ok().is_some_and(|n| n > 0 && n <= structural.sources));
        structural.invalid_numeric_citation_occurrences += usize::from(!valid);
    }
    let markdown = normalized(&report.markdown);
    let missing_fact_strings: Vec<_> = case.expected.facts.iter().filter(|s| !markdown.contains(&normalized(s))).cloned().collect();
    let missing_source_urls: Vec<_> = case
        .expected
        .source_urls
        .iter()
        .filter(|url| !report.ledger.sources.iter().any(|s| canonical_url(&s.url) == canonical_url(url)))
        .cloned()
        .collect();
    let literal_checks = LiteralChecks {
        expected_facts: case.expected.facts.len(),
        matched_fact_strings: case.expected.facts.len() - missing_fact_strings.len(),
        missing_fact_strings,
        expected_source_urls: case.expected.source_urls.len(),
        matched_source_urls: case.expected.source_urls.len() - missing_source_urls.len(),
        missing_source_urls,
        forbidden_string_matches: case
            .expected
            .forbidden_assertions
            .iter()
            .filter(|s| markdown.contains(&normalized(s)))
            .cloned()
            .collect(),
    };
    Outcome {
        case_id: case.id.clone(),
        topic: case.topic.clone(),
        status: "completed".into(),
        error: None,
        structural: Some(structural),
        literal_checks: Some(literal_checks),
        metrics: Some(report.metrics.clone()),
    }
}

fn normalized(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

fn http_url(raw: &str) -> bool {
    url::Url::parse(raw).is_ok_and(|u| {
        matches!(u.scheme(), "http" | "https") && u.host_str().is_some() && u.username().is_empty() && u.password().is_none()
    })
}

fn canonical_url(raw: &str) -> String {
    url::Url::parse(raw)
        .map(|mut u| {
            u.set_fragment(None);
            u.to_string()
        })
        .unwrap_or_else(|_| raw.into())
}

/// Create once: existing files, directories and symlinks are all refused.
pub fn create_output(path: &Path) -> Result<()> {
    std::fs::create_dir(path)
        .with_context(|| format!("create fresh output directory {}; choose a new path if it exists", path.display()))?;
    Ok(())
}

pub fn write_new_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    serde_json::to_writer_pretty(&mut file, value)?;
    writeln!(file)?;
    file.sync_all()?;
    Ok(())
}

pub fn csv(outcomes: &[Outcome]) -> String {
    let mut output = "case_id,topic,status,error,sources,claims,claims_with_recorded_quotes,invalid_claim_source_refs,invalid_numeric_citation_occurrences,matched_fact_strings,expected_facts,matched_source_urls,expected_source_urls,forbidden_string_matches,elapsed_ms,search_requests,page_attempts,model_calls,input_tokens,output_tokens,verifier_model,separate_verifier\n".to_string();
    for o in outcomes {
        let s = o.structural.as_ref();
        let l = o.literal_checks.as_ref();
        let m = o.metrics.as_ref();
        let number = |n: Option<usize>| n.map(|n| n.to_string()).unwrap_or_default();
        let fields = vec![
            o.case_id.clone(),
            o.topic.clone(),
            o.status.clone(),
            o.error.clone().unwrap_or_default(),
            number(s.map(|s| s.sources)),
            number(s.map(|s| s.claims)),
            number(s.map(|s| s.claims_with_recorded_quotes)),
            number(s.map(|s| s.invalid_claim_source_refs)),
            number(s.map(|s| s.invalid_numeric_citation_occurrences)),
            number(l.map(|l| l.matched_fact_strings)),
            number(l.map(|l| l.expected_facts)),
            number(l.map(|l| l.matched_source_urls)),
            number(l.map(|l| l.expected_source_urls)),
            number(l.map(|l| l.forbidden_string_matches.len())),
            m.map(|m| m.elapsed_ms.to_string()).unwrap_or_default(),
            number(m.map(|m| m.search_requests)),
            number(m.map(|m| m.page_attempts)),
            number(m.map(|m| m.model_calls)),
            m.and_then(|m| m.input_tokens).map(|n| n.to_string()).unwrap_or_default(),
            m.and_then(|m| m.output_tokens).map(|n| n.to_string()).unwrap_or_default(),
            m.map(|m| m.verifier_model.clone()).unwrap_or_default(),
            m.map(|m| m.separate_verifier.to_string()).unwrap_or_default(),
        ];
        output.push_str(&fields.iter().map(|s| csv_cell(s)).collect::<Vec<_>>().join(","));
        output.push('\n');
    }
    output
}

fn csv_cell(raw: &str) -> String {
    let formula = raw.trim_start_matches(|c: char| c.is_whitespace() || c.is_control()).starts_with(['=', '+', '-', '@']);
    let prefix = if formula { "'" } else { "" };
    format!("\"{prefix}{}\"", raw.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::{Claim, SourceDoc};

    fn case() -> Case {
        Case {
            id: "controlled".into(),
            topic: "Artificial reference fixture".into(),
            mode: Mode::Normal,
            depth: 1,
            breadth: 1,
            max_pages: 2,
            timeout_secs: 10,
            expected: Expectations {
                facts: vec!["Meridian launched in 2024".into()],
                source_urls: vec!["https://example.org/reference".into()],
                forbidden_assertions: vec!["Meridian launched in 2025".into()],
            },
        }
    }

    #[test]
    fn rejects_unsafe_duplicate_ids_and_bad_bounds() {
        for id in ["../escape", "/absolute", "x/y", "x\\y", ".", "", "Mixed", "=formula", "-x", "cases", "results", "run"] {
            let mut c = case();
            c.id = id.into();
            assert!(validate_cases(&[c]).is_err(), "{id}");
        }
        assert!(validate_cases(&[case(), case()]).is_err());
        assert!(validate_cases(&[]).is_err());
        for change in 0..8 {
            let mut c = case();
            match change {
                0 => c.depth = 0,
                1 => c.depth = 6,
                2 => c.breadth = 0,
                3 => c.breadth = 7,
                4 => c.max_pages = 0,
                5 => c.max_pages = 1001,
                6 => c.timeout_secs = 0,
                _ => c.timeout_secs = 3601,
            }
            assert!(validate_cases(&[c]).is_err());
        }
    }

    #[test]
    fn grades_controlled_reference_without_claiming_factual_accuracy() {
        let mut report = Report { markdown: "Meridian launched in 2024. [1]".into(), ..Default::default() };
        report.ledger.sources.push(SourceDoc { url: "https://example.org/reference#section".into(), ..Default::default() });
        report.ledger.claims.push(Claim {
            text: "Meridian launched in 2024".into(),
            quote: "launched in 2024".into(),
            sources: vec![0],
            ..Default::default()
        });
        let outcome = grade(&case(), &report);
        let literal = outcome.literal_checks.unwrap();
        assert_eq!(literal.matched_fact_strings, 1);
        assert_eq!(literal.matched_source_urls, 1);
        assert!(literal.forbidden_string_matches.is_empty());
        assert_eq!(outcome.structural.unwrap().invalid_numeric_citation_occurrences, 0);
        report.markdown = "Meridian launched in 2025. [9]".into();
        report.ledger.claims[0].sources = vec![5];
        report.ledger.claims[0].quote.clear();
        let outcome = grade(&case(), &report);
        let literal = outcome.literal_checks.unwrap();
        assert_eq!(literal.matched_fact_strings, 0);
        assert_eq!(literal.forbidden_string_matches.len(), 1);
        let structural = outcome.structural.unwrap();
        assert_eq!(structural.invalid_claim_source_refs, 1);
        assert_eq!(structural.claims_with_recorded_quotes, 0);
        assert_eq!(structural.invalid_numeric_citation_occurrences, 1);
    }

    #[test]
    fn grouped_and_legacy_citations_check_every_source_number() {
        let mut report = Report::default();
        report.ledger.sources.push(SourceDoc { url: "https://example.org".into(), ..Default::default() });
        for citation in ["[1, 99]", "[S1, S99]", "[0, 1]", "[1, 999999999999999999999999999999999999999]"] {
            report.markdown = citation.into();
            let checks = grade(&case(), &report).structural.unwrap();
            assert_eq!(checks.numeric_citation_occurrences, 1, "{citation}");
            assert_eq!(checks.invalid_numeric_citation_occurrences, 1, "{citation}");
        }
        report.markdown = "Supported [1, 1] and legacy [S1, S1].".into();
        let checks = grade(&case(), &report).structural.unwrap();
        assert_eq!(checks.numeric_citation_occurrences, 2);
        assert_eq!(checks.invalid_numeric_citation_occurrences, 0);
    }

    #[test]
    fn malformed_old_reports_are_safe_to_grade_and_metrics_default() {
        let report: Report = serde_json::from_value(serde_json::json!({"topic":"old", "mode":"Normal", "markdown":"[0] [999999999999999999999999999999999999999]",
            "ledger":{"sources":[],"claims":[{"text":"a","quote":"","lens":"a","sources":[99],"contradicts":[0,8],"verdict":null}]},"ghosts":[]})).unwrap();
        assert_eq!(report.metrics.model_calls, 0);
        let s = grade(&case(), &report).structural.unwrap();
        assert_eq!(s.invalid_claim_source_refs, 1);
        assert_eq!(s.invalid_contradiction_refs, 2);
        assert_eq!(s.invalid_numeric_citation_occurrences, 2);
    }

    #[test]
    fn surfaces_unhandled_asymmetric_and_supported_conflicts() {
        let mut report = Report::default();
        report.ledger.claims = vec![
            Claim { contradicts: vec![1], verdict: Some("supported: weak review".into()), ..Default::default() },
            Claim { contradicts: vec![0], ..Default::default() },
        ];
        let s = grade(&case(), &report).structural.unwrap();
        assert_eq!(s.conflicting_claims, 2);
        assert_eq!(s.conflicting_claims_without_verdict, 1);
        assert_eq!(s.conflicting_claims_marked_supported, 1);
        assert_eq!(s.asymmetric_contradiction_refs, 0);
        report.ledger.claims[1].contradicts.clear();
        assert_eq!(grade(&case(), &report).structural.unwrap().asymmetric_contradiction_refs, 1);
    }

    #[test]
    fn csv_quotes_cells_and_neutralizes_spreadsheet_formulas() {
        for value in ["=HYPERLINK(\"evil\")", "+SUM(1)", " -2+3", "\t@evil", "\n=1+1"] {
            assert!(csv_cell(value).starts_with("\"'"));
        }
        assert_eq!(csv_cell("a,\"b\"\nc"), "\"a,\"\"b\"\"\nc\"");
        assert!(csv(&[Outcome::failed(&case(), "=danger")]).contains("\"'=danger\""));
    }

    #[test]
    fn source_specific_evidence_is_counted_and_checked() {
        let mut report = Report::default();
        report.ledger.sources.push(SourceDoc { url: "https://example.org".into(), ..Default::default() });
        report.ledger.claims.push(Claim {
            sources: vec![0],
            evidence: vec![crate::ledger::Evidence { source: 5, quote: "Artificial quote".into(), locator: None }],
            ..Default::default()
        });
        let s = grade(&case(), &report).structural.unwrap();
        assert_eq!(s.claims_with_recorded_quotes, 1);
        assert_eq!(s.invalid_evidence_source_refs, 1);
    }

    #[test]
    fn malformed_suite_files_and_invalid_expectations_fail_before_execution() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("cases.json");
        std::fs::write(&path, "not JSON").unwrap();
        assert!(load_cases(&path).is_err());
        let mut value = serde_json::to_value(vec![case()]).unwrap();
        value[0]["unexpected_option"] = serde_json::json!(true);
        std::fs::write(&path, value.to_string()).unwrap();
        assert!(load_cases(&path).is_err());
        let mut c = case();
        c.expected.source_urls = vec!["file:///etc/passwd".into()];
        assert!(validate_cases(&[c]).is_err());
        let mut c = case();
        c.expected.facts = vec!["   ".into()];
        assert!(validate_cases(&[c]).is_err());
    }

    #[test]
    fn refuses_overwrite_and_preserves_partial_success() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("run");
        create_output(&dir).unwrap();
        assert!(create_output(&dir).is_err());
        let path = dir.join("report.json");
        write_new_json(&path, &Report::default()).unwrap();
        let before = std::fs::read(&path).unwrap();
        assert!(write_new_json(&path, &Report::default()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let outcomes = [grade(&case(), &Report::default()), Outcome::failed(&case(), "provider failed")];
        write_new_json(&dir.join("results.json"), &outcomes).unwrap();
        assert_eq!(outcomes[0].status, "completed");
        assert_eq!(outcomes[1].status, "error");
        assert!(outcomes[1].structural.is_none());
    }
}
