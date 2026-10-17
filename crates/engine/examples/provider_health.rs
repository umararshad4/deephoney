//! Explicit, bounded search-adapter diagnostics. Default invocation performs no network I/O.
use std::{
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    time::{Duration, Instant},
};

use anyhow::{Result, bail};
use engine::{Config, Mode, net::Net, sources::Source};
use futures::{StreamExt, stream};
use serde::Serialize;

const HELP: &str = "provider_health [--public | --configured] [--tor] [--provider NAME] --out NEW_FILE\n\
Without a run flag: list adapters only; no environment configuration or network.\n\
--public: keyless adapters with default configuration.\n\
--configured: environment configuration; searches may consume service credits.\n\
--tor: also permit Ahmia filtered discovery (does not read onion pages).\n\
Runs require a new output file. Errors or an all-skipped run return exit 1 after writing all result rows.";

#[derive(Default, Debug, PartialEq)]
struct Args {
    configured: bool,
    public: bool,
    tor: bool,
    provider: Option<String>,
    out: Option<PathBuf>,
}

fn parse(args: impl IntoIterator<Item = String>) -> Result<Args> {
    let mut args = args.into_iter();
    let mut result = Args::default();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--public" if !result.public => result.public = true,
            "--configured" if !result.configured => result.configured = true,
            "--tor" if !result.tor => result.tor = true,
            "--provider" if result.provider.is_none() => {
                let name = args.next().ok_or_else(|| anyhow::anyhow!("--provider requires a name"))?;
                if Source::by_name(&name).is_none() {
                    bail!("unknown provider name");
                }
                result.provider = Some(name);
            }
            "--out" if result.out.is_none() => {
                let path = args.next().filter(|p| !p.starts_with("--")).ok_or_else(|| anyhow::anyhow!("--out requires a file"))?;
                result.out = Some(PathBuf::from(path));
            }
            _ => bail!("unknown or repeated flag"),
        }
    }
    if result.public && result.configured {
        bail!("choose --public or --configured");
    }
    let running = result.public || result.configured;
    if running != result.out.is_some() {
        bail!("runs require --out NEW_FILE; list-only mode does not write a file");
    }
    if result.tor && !running {
        bail!("--tor requires an explicit run flag");
    }
    Ok(result)
}

fn query(source: Source) -> &'static str {
    match source {
        Source::CrtSh | Source::CommonCrawl | Source::Wayback => "example.com",
        Source::Telegram => "@telegram privacy",
        Source::Crossref => "10.1038/nature14539",
        Source::Ahmia => "Tor Project",
        _ => "distributed systems",
    }
}

fn mode(source: Source) -> Mode {
    match source {
        Source::Ahmia => Mode::Dark,
        Source::Wayback => Mode::Hidden,
        _ => Mode::Normal,
    }
}

fn skip_reason(source: Source, cfg: &Config) -> Option<&'static str> {
    match source {
        Source::Brave if cfg.brave_key.is_empty() => Some("credential_not_configured"),
        Source::Exa if cfg.exa_key.is_empty() => Some("credential_not_configured"),
        Source::Tavily if cfg.tavily_key.is_empty() => Some("credential_not_configured"),
        Source::Searx if cfg.searxng_url.is_empty() => Some("endpoint_not_configured"),
        Source::GitHub if cfg.github_token.is_empty() => Some("credential_not_configured"),
        Source::CourtListener if cfg.courtlistener_token.is_empty() => Some("credential_not_configured"),
        Source::Edgar if cfg.contact.trim().is_empty() => Some("required_contact_not_configured"),
        Source::Ahmia if !cfg.tor_enabled => Some("explicit_tor_opt_in_required"),
        Source::Ddg if !Source::for_mode(Mode::Normal, cfg).contains(&source) => Some("fallback_disabled_by_configured_web_adapter"),
        _ => None,
    }
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Status {
    OkNonempty,
    OkEmpty,
    Error,
    Skipped,
}

fn result_status(count: usize) -> Status {
    if count == 0 { Status::OkEmpty } else { Status::OkNonempty }
}

#[derive(Serialize)]
struct Row {
    provider: &'static str,
    query: &'static str,
    status: Status,
    reason: Option<String>,
    elapsed_ms: u64,
    result_count: usize,
    requests: usize,
    retries: usize,
    recovered: usize,
}

fn run_failed(rows: &[Row]) -> bool {
    rows.iter().any(|row| row.status == Status::Error) || rows.iter().all(|row| row.status == Status::Skipped)
}

async fn diagnose(source: Source, cfg: &Config) -> Row {
    let mut row = Row {
        provider: source.name(),
        query: query(source),
        status: Status::Skipped,
        reason: skip_reason(source, cfg).map(str::to_owned),
        elapsed_ms: 0,
        result_count: 0,
        requests: 0,
        retries: 0,
        recovered: 0,
    };
    if row.reason.is_some() {
        return row;
    }
    let start = Instant::now();
    let net = match Net::for_mode(cfg, mode(source)) {
        Ok(net) => net,
        Err(_) => {
            row.status = Status::Error;
            row.reason = Some("network_initialization_failed".into());
            return row;
        }
    };
    match tokio::time::timeout(Duration::from_secs(45), source.search(&net, cfg, row.query)).await {
        Ok(Ok(hits)) => {
            row.result_count = hits.len();
            row.status = result_status(hits.len());
        }
        Ok(Err(error)) => {
            row.status = Status::Error;
            // Never print upstream error strings: reqwest errors can include credentials/query URLs.
            let status = error.chain().find_map(|cause| cause.downcast_ref::<reqwest::Error>().and_then(|e| e.status()));
            row.reason = Some(status.map(|s| format!("http_{}", s.as_u16())).unwrap_or_else(|| "request_or_adapter_error".into()));
        }
        Err(_) => {
            row.status = Status::Error;
            row.reason = Some("provider_timeout_45_seconds".into());
        }
    }
    row.elapsed_ms = start.elapsed().as_millis().min(u64::MAX as u128) as u64;
    let stats = net.stats();
    row.requests = stats.requests;
    row.retries = stats.retries;
    row.recovered = stats.recovered;
    row
}

#[derive(Serialize)]
struct Report {
    schema: u32,
    configuration: &'static str,
    elapsed_ms: u64,
    rows: Vec<Row>,
}

async fn execute(args: Args) -> Result<bool> {
    let sources: Vec<_> = Source::ALL.into_iter().filter(|s| args.provider.as_deref().is_none_or(|p| s.name() == p)).collect();
    let Some(path) = args.out else {
        for source in sources {
            println!("{}: {}", source.name(), source.about());
        }
        println!("List only. No network requests made. Use --help for explicit diagnostics.");
        return Ok(false);
    };
    // Reserve before loading environment configuration or starting any requests. Never overwrite.
    let mut output = OpenOptions::new().write(true).create_new(true).open(path)?;
    let mut cfg = if args.configured { Config::from_env() } else { Config::default() };
    cfg.tor_enabled = args.tor;
    cfg.browser_enabled = false;
    cfg.visual_documents = false;
    let start = Instant::now();
    let mut rows = stream::iter(sources).map(|source| diagnose(source, &cfg)).buffer_unordered(4).collect::<Vec<_>>().await;
    rows.sort_by_key(|row| row.provider);
    let failed = run_failed(&rows);
    let report = Report {
        schema: 1,
        configuration: if args.configured { "environment" } else { "public_defaults" },
        elapsed_ms: start.elapsed().as_millis().min(u64::MAX as u128) as u64,
        rows,
    };
    serde_json::to_writer_pretty(&mut output, &report)?;
    output.write_all(b"\n")?;
    output.sync_all()?;
    eprintln!(
        "Diagnostics written: {} error(s), {} empty response(s), {} skipped. Counts do not prove relevance or research quality.",
        report.rows.iter().filter(|r| r.status == Status::Error).count(),
        report.rows.iter().filter(|r| r.status == Status::OkEmpty).count(),
        report.rows.iter().filter(|r| r.status == Status::Skipped).count()
    );
    Ok(failed)
}

#[tokio::main]
async fn main() {
    let raw: Vec<_> = std::env::args().skip(1).collect();
    if raw == ["--help"] {
        println!("{HELP}");
        return;
    }
    match parse(raw) {
        Ok(args) => match execute(args).await {
            Ok(false) => {}
            Ok(true) => std::process::exit(1),
            Err(error) => {
                eprintln!("Diagnostic output failed: {error}");
                std::process::exit(1);
            }
        },
        Err(error) => {
            eprintln!("{error}\n{HELP}");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(raw: &[&str]) -> Result<Args> {
        parse(raw.iter().map(|s| s.to_string()))
    }
    #[test]
    fn list_only_default_and_explicit_configuration_required() {
        assert_eq!(args(&[]).unwrap(), Args::default());
        assert!(args(&["--public"]).is_err());
        assert!(args(&["--out", "new.json"]).is_err());
        assert!(args(&["--tor"]).is_err());
        assert!(args(&["--public", "--configured", "--out", "new.json"]).is_err());
        assert!(args(&["--public", "--out", "new.json", "--tor"]).unwrap().tor);
    }
    #[test]
    fn invalid_flags_providers_and_repeated_values_refused() {
        for raw in [
            vec!["--unknown"],
            vec!["--provider", "bad"],
            vec!["--provider"],
            vec!["--public", "--out", "--tor"],
            vec!["--provider", "arxiv", "--provider", "crossref"],
            vec!["--public", "--public", "--out", "new.json"],
        ] {
            assert!(args(&raw).is_err());
        }
        assert_eq!(args(&["--provider", "crossref"]).unwrap().provider.as_deref(), Some("crossref"));
    }
    #[test]
    fn twelve_public_adapters_and_explicit_tor_gate() {
        let mut cfg = Config::default();
        assert_eq!(Source::ALL.into_iter().filter(|s| skip_reason(*s, &cfg).is_none()).count(), 12);
        assert_eq!(skip_reason(Source::Ahmia, &cfg), Some("explicit_tor_opt_in_required"));
        cfg.tor_enabled = true;
        assert_eq!(skip_reason(Source::Ahmia, &cfg), None);
        cfg.brave_key = "synthetic-configured-key".into();
        assert_eq!(skip_reason(Source::Ddg, &cfg), Some("fallback_disabled_by_configured_web_adapter"));
    }
    #[test]
    fn failed_or_all_skipped_runs_never_report_success() {
        fn row(status: Status) -> Row {
            Row {
                provider: "brave",
                query: "distributed systems",
                status,
                reason: None,
                elapsed_ms: 0,
                result_count: 0,
                requests: 0,
                retries: 0,
                recovered: 0,
            }
        }
        assert!(run_failed(&[]));
        assert!(run_failed(&[row(Status::Skipped)]));
        assert!(run_failed(&[row(Status::OkNonempty), row(Status::Error)]));
        assert!(!run_failed(&[row(Status::Skipped), row(Status::OkEmpty)]));
        assert!(!run_failed(&[row(Status::OkNonempty)]));
    }
    #[test]
    fn empty_and_nonempty_are_distinct_not_accuracy_claims() {
        assert_eq!(result_status(0), Status::OkEmpty);
        assert_eq!(result_status(1), Status::OkNonempty);
        assert_eq!(serde_json::to_string(&Status::OkNonempty).unwrap(), "\"ok_nonempty\"");
        assert_eq!(query(Source::Telegram), "@telegram privacy");
        assert_eq!(query(Source::CrtSh), "example.com");
        assert_eq!(mode(Source::Wayback), Mode::Hidden);
        assert_eq!(mode(Source::Ahmia), Mode::Dark);
    }
}
