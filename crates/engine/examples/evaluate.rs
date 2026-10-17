//! Explicit opt-in network evaluation, or offline structural/literal grading.
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use engine::{
    Config, DeepResearch, Provider, Report, Researcher,
    evaluation::{self, Case, Outcome},
};
use serde::Serialize;

fn main() -> Result<()> {
    if engine::document::run_pdf_worker() {
        return Ok(());
    }
    tokio::runtime::Builder::new_multi_thread().enable_all().build()?.block_on(run())
}

async fn run() -> Result<()> {
    let mut cases_path = PathBuf::from("evaluation/cases.json");
    let mut output = None;
    let mut reports = None;
    let mut run = false;
    let mut only = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--cases" => cases_path = args.next().context("missing --cases path")?.into(),
            "--out" => output = Some(PathBuf::from(args.next().context("missing --out path")?)),
            "--reports" => reports = Some(PathBuf::from(args.next().context("missing --reports path")?)),
            "--case" => only = Some(args.next().context("missing --case ID")?),
            "--run" => run = true,
            "--help" | "-h" => {
                println!(
                    "cargo run -p engine --example evaluate -- [--cases evaluation/cases.json] [--case ID]\n\nNo action flag: validate/list cases; no network or output writes.\n--run --out NEW_DIRECTORY: research using environment configuration (can incur provider costs).\n--reports REPORT_DIRECTORY --out NEW_DIRECTORY: grade ID.json files offline; never calls providers.\nRun and offline grading cannot be combined. Existing output paths are refused. Results are structural/literal checks, not factual accuracy."
                );
                return Ok(());
            }
            _ => bail!("unknown option {arg}"),
        }
    }
    if run && reports.is_some() {
        bail!("choose --run or --reports, not both");
    }
    let mut cases = evaluation::load_cases(&cases_path)?;
    if let Some(id) = only {
        cases.retain(|case| case.id == id);
        if cases.is_empty() {
            bail!("unknown case ID {id}");
        }
    }
    if !run && reports.is_none() {
        if output.is_some() {
            bail!("--out requires --run or --reports");
        }
        for case in &cases {
            println!(
                "{}: {:?}, depth {}, breadth {}, pages {}, timeout {}s — {}",
                case.id, case.mode, case.depth, case.breadth, case.max_pages, case.timeout_secs, case.topic
            );
        }
        println!("Validated {} cases. No network calls made. Use --run explicitly to execute.", cases.len());
        return Ok(());
    }
    let config = if run {
        let cfg = Config::from_env();
        validate_config(&cfg)?;
        Some(cfg)
    } else {
        None
    };
    let output = output.context("--out NEW_DIRECTORY is required")?;
    evaluation::create_output(&output)?;
    evaluation::write_new_json(&output.join("cases.json"), &cases)?;
    let mut results_log = new_log(&output.join("results.jsonl"))?;
    let mut errors_log = new_log(&output.join("errors.jsonl"))?;
    let manifest = match &config {
        Some(cfg) => serde_json::json!({"engine_version": env!("CARGO_PKG_VERSION"), "network_run": true,
            "provider": cfg.provider, "strong_model": cfg.strong_model, "fast_model": cfg.fast_model,
            "verifier_provider": cfg.verifier_provider, "verifier_model": cfg.verifier_model,
            "browser_enabled": cfg.browser_enabled, "tor_enabled": cfg.tor_enabled}),
        None => serde_json::json!({"engine_version": env!("CARGO_PKG_VERSION"), "network_run": false}),
    };
    evaluation::write_new_json(&output.join("run.json"), &manifest)?;
    let mut outcomes = Vec::new();
    for case in &cases {
        eprintln!("{}: {}", case.id, if run { "researching" } else { "grading offline" });
        let result = if let Some(cfg) = &config {
            let mut cfg = cfg.clone();
            cfg.max_pages = case.max_pages;
            research(case, cfg, &output).await
        } else {
            let source = reports.as_ref().expect("offline path checked").join(format!("{}.json", case.id));
            load_report(&source).and_then(|report| {
                evaluation::write_new_json(&output.join(format!("{}.json", case.id)), &report)?;
                Ok(report)
            })
        };
        let outcome = match result {
            Ok(report) => evaluation::grade(case, &report),
            Err(error) => {
                let outcome = Outcome::failed(case, format!("{error:#}"));
                json_line(&mut errors_log, &outcome)?;
                outcome
            }
        };
        json_line(&mut results_log, &outcome)?;
        outcomes.push(outcome);
    }
    evaluation::write_new_json(&output.join("results.json"), &outcomes)?;
    let mut csv = new_log(&output.join("results.csv"))?;
    csv.write_all(evaluation::csv(&outcomes).as_bytes())?;
    csv.sync_all()?;
    let failures = outcomes.iter().filter(|o| o.status == "error").count();
    eprintln!(
        "{} reports graded, {failures} errors. Artifacts: {}. These are not factual accuracy scores.",
        outcomes.len() - failures,
        output.display()
    );
    if failures > 0 {
        bail!("{failures} case(s) failed; successful reports and per-case errors were preserved");
    }
    Ok(())
}

fn load_report(path: &Path) -> Result<Report> {
    serde_json::from_reader(File::open(path).with_context(|| format!("open {}", path.display()))?)
        .with_context(|| format!("parse report {}", path.display()))
}

fn new_log(path: &Path) -> Result<File> {
    Ok(OpenOptions::new().write(true).create_new(true).open(path)?)
}

fn json_line(file: &mut File, value: &impl Serialize) -> Result<()> {
    serde_json::to_writer(&mut *file, value)?;
    writeln!(file)?;
    file.sync_all()?;
    Ok(())
}

fn validate_config(cfg: &Config) -> Result<()> {
    if cfg.strong_model.trim().is_empty() || cfg.fast_model.trim().is_empty() {
        bail!("model names are required (MODEL for an OpenAI-compatible endpoint)");
    }
    match cfg.provider {
        Provider::Anthropic if cfg.llm_key.trim().is_empty() => bail!("ANTHROPIC_API_KEY is required for --run"),
        Provider::OpenAi => {
            let base = url::Url::parse(&cfg.llm_base).context("invalid OPENAI_BASE_URL")?;
            if !matches!(base.scheme(), "http" | "https")
                || base.host_str().is_none()
                || !base.username().is_empty()
                || base.password().is_some()
            {
                bail!("OPENAI_BASE_URL must use HTTP(S), a host and no embedded credentials");
            }
            let local = match base.host() {
                Some(url::Host::Domain(host)) => host == "localhost",
                Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
                Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
                _ => false,
            };
            if !local && cfg.llm_key.trim().is_empty() {
                bail!("OPENAI_API_KEY is required for a non-loopback --run endpoint");
            }
        }
        _ => {}
    }
    Ok(())
}

async fn research(case: &Case, cfg: Config, output: &Path) -> Result<Report> {
    let mut events = new_log(&output.join(format!("{}.events.jsonl", case.id)))?;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let result = {
        let task = tokio::time::timeout(Duration::from_secs(case.timeout_secs), DeepResearch.run(cfg, case.request(), tx));
        tokio::pin!(task);
        let mut events_open = true;
        loop {
            tokio::select! {
                result = &mut task => break result.context("case research timeout")?,
                event = rx.recv(), if events_open => match event {
                    Some(event) => json_line(&mut events, &serde_json::json!({"case_id": case.id, "event": event}))?,
                    None => events_open = false,
                },
            }
        }
    };
    while let Ok(event) = rx.try_recv() {
        json_line(&mut events, &serde_json::json!({"case_id": case.id, "event": event}))?;
    }
    let report = result?;
    evaluation::write_new_json(&output.join(format!("{}.json", case.id)), &report)?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_remote_runs_require_explicit_configuration() {
        assert!(validate_config(&Config::default()).is_err());
        let mut cfg = Config { provider: Provider::OpenAi, llm_base: "http://127.0.0.1:11434/v1".into(), ..Default::default() };
        assert!(validate_config(&cfg).is_ok());
        cfg.llm_base = "https://example.org/v1".into();
        assert!(validate_config(&cfg).is_err());
        cfg.llm_key = "test-only".into();
        assert!(validate_config(&cfg).is_ok());
        cfg.llm_base = "https://user:password@example.org/v1".into();
        assert!(validate_config(&cfg).is_err());
    }
}
