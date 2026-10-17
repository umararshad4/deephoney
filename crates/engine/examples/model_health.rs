//! Explicit model transport/JSON smoke test, not a factual accuracy evaluation.
use anyhow::{Result, bail};
use engine::{Config, llm::Llm};
use serde::{Deserialize, Serialize};
use std::{
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    time::{Duration, Instant},
};

const HELP: &str = "model_health [--configured --slot fast|strong|verifier --out NEW_FILE]\nDefault: list slots; no environment configuration or network.\n--configured explicitly permits model calls and charges using CLI environment configuration.\nA run makes one bounded JSON request, with existing Llm retry behavior, and records no model text, key or endpoint.\nThis tests transport and a tiny JSON contract, not research accuracy or actual monetary cost.";

#[derive(Default, Debug, PartialEq)]
struct Args {
    configured: bool,
    slot: Option<String>,
    out: Option<PathBuf>,
}
fn parse(raw: impl IntoIterator<Item = String>) -> Result<Args> {
    let mut args = Args::default();
    let mut raw = raw.into_iter();
    while let Some(flag) = raw.next() {
        match flag.as_str() {
            "--configured" if !args.configured => args.configured = true,
            "--slot" if args.slot.is_none() => {
                let slot = raw.next().ok_or_else(|| anyhow::anyhow!("missing slot"))?;
                if !matches!(slot.as_str(), "fast" | "strong" | "verifier") {
                    bail!("unknown slot");
                }
                args.slot = Some(slot);
            }
            "--out" if args.out.is_none() => {
                args.out = Some(
                    raw.next()
                        .filter(|s| !s.starts_with("--"))
                        .ok_or_else(|| anyhow::anyhow!("missing output"))?
                        .into(),
                );
            }
            _ => bail!("unknown or repeated flag"),
        }
    }
    if args.configured != args.out.is_some() || args.configured != args.slot.is_some() {
        bail!("runs require --configured, --slot and --out together");
    }
    Ok(args)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Answer {
    status: String,
    answer: u32,
}
#[derive(Serialize)]
struct Report {
    schema: u32,
    slot: String,
    status: &'static str,
    reason: Option<&'static str>,
    elapsed_ms: u64,
    model_requests: usize,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cost: Option<f64>,
}
async fn diagnose(cfg: &Config, slot: &str) -> Report {
    let mut report = Report {
        schema: 1,
        slot: slot.into(),
        status: "skipped",
        reason: None,
        elapsed_ms: 0,
        model_requests: 0,
        input_tokens: None,
        output_tokens: None,
        cost: None,
    };
    if slot == "verifier" && cfg.verifier_model.trim().is_empty() {
        report.reason = Some("verifier_not_configured");
        return report;
    }
    let llm = Llm::new(cfg);
    let llm = if slot == "verifier" {
        match llm.verifier() {
            Ok((verifier, _)) => verifier,
            Err(_) => {
                report.reason = Some("verifier_not_configured");
                return report;
            }
        }
    } else {
        llm
    };
    let start = Instant::now();
    let result = tokio::time::timeout(
        Duration::from_secs(60),
        llm.json::<Answer>(
            slot != "fast",
            "MODEL DIAGNOSTIC. Follow the exact output contract.",
            "Return exactly {\"status\":\"ok\",\"answer\":2}. No other fields.",
            128,
        ),
    )
    .await;
    report.elapsed_ms = start.elapsed().as_millis().min(u64::MAX as u128) as u64;
    match result {
        Ok(Ok(answer)) if answer.status == "ok" && answer.answer == 2 => {
            report.status = "response_valid"
        }
        Ok(Ok(_)) => {
            report.status = "invalid_response";
            report.reason = Some("contract_mismatch");
        }
        Ok(Err(_)) => {
            report.status = "error";
            report.reason = Some("transport_or_json_error");
        }
        Err(_) => {
            report.status = "error";
            report.reason = Some("model_timeout_60_seconds");
        }
    }
    let usage = llm.usage();
    report.model_requests = usage.requests;
    report.input_tokens = usage.input_tokens;
    report.output_tokens = usage.output_tokens;
    report
}
async fn execute(args: Args) -> Result<bool> {
    let Some(path) = args.out else {
        println!(
            "fast: extraction model\nstrong: planner/writer model\nverifier: separately configured verifier\nList only; no configuration or network loaded."
        );
        return Ok(false);
    };
    // Reserve output before loading environment credentials or issuing any model call.
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut output = options.open(path)?;
    let report = diagnose(&Config::from_env(), args.slot.as_deref().unwrap()).await;
    serde_json::to_writer_pretty(&mut output, &report)?;
    output.write_all(b"\n")?;
    output.sync_all()?;
    println!(
        "Model diagnostic: {}. Not research accuracy; monetary cost is unknown.",
        report.status
    );
    Ok(report.status != "response_valid")
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
            Err(_) => {
                eprintln!("Diagnostic output failed; choose a new writable file.");
                std::process::exit(1);
            }
        },
        Err(_) => {
            eprintln!("Invalid arguments.\n{HELP}");
            std::process::exit(2);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use engine::Provider;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    #[test]
    fn no_implicit_calls_or_partial_opt_in() {
        assert_eq!(parse(Vec::new()).unwrap(), Args::default());
        for raw in [
            vec!["--configured"],
            vec!["--slot", "fast"],
            vec!["--out", "x"],
            vec!["--configured", "--slot", "unknown", "--out", "x"],
            vec![
                "--configured",
                "--slot",
                "fast",
                "--out",
                "x",
                "--configured",
            ],
        ] {
            assert!(parse(raw.into_iter().map(str::to_owned)).is_err());
        }
        assert!(
            parse(["--configured", "--slot", "strong", "--out", "x"].map(str::to_owned)).is_ok()
        );
    }
    async fn serve(
        status: &str,
        content: serde_json::Value,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let body = content.to_string();
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let handle = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut data = Vec::new();
            loop {
                let mut chunk = [0; 4096];
                let n = socket.read(&mut chunk).await.unwrap();
                assert!(n > 0);
                data.extend_from_slice(&chunk[..n]);
                if let Some(end) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&data[..end]);
                    let length: usize = header
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|s| s.trim().parse().unwrap())
                        })
                        .unwrap();
                    if data.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            let request = String::from_utf8(data).unwrap();
            assert!(request.contains("/chat/completions"));
            assert!(request.contains("synthetic-secret"));
            socket.write_all(response.as_bytes()).await.unwrap();
        });
        (base, handle)
    }
    #[tokio::test]
    async fn actual_transport_contract_errors_and_privacy() {
        for (http, text, expected) in [
            (
                "200 OK",
                "{\"status\":\"ok\",\"answer\":2}",
                "response_valid",
            ),
            (
                "200 OK",
                "{\"status\":\"ok\",\"answer\":3}",
                "invalid_response",
            ),
            ("200 OK", "not JSON synthetic-secret", "error"),
            ("401 Unauthorized", "synthetic-secret", "error"),
        ] {
            let (base, handle) = serve(
                http,
                serde_json::json!({"choices":[{"message":{"content":text}}],
                "usage":{"prompt_tokens":9,"completion_tokens":4}}),
            )
            .await;
            let cfg = Config {
                provider: Provider::OpenAi,
                llm_base: base.clone(),
                llm_key: "synthetic-secret".into(),
                strong_model: "synthetic-secret-model".into(),
                ..Config::default()
            };
            let report = diagnose(&cfg, "strong").await;
            handle.await.unwrap();
            assert_eq!(report.status, expected);
            let json = serde_json::to_string(&report).unwrap();
            assert!(!json.contains("synthetic-secret"));
            assert!(!json.contains(&base));
            assert_eq!(report.cost, None);
            if expected == "response_valid" {
                assert_eq!(report.input_tokens, Some(9));
                assert_eq!(report.output_tokens, Some(4));
            }
        }
    }
    #[tokio::test]
    async fn absent_verifier_is_skipped_without_request() {
        let report = diagnose(&Config::default(), "verifier").await;
        assert_eq!(report.status, "skipped");
        assert_eq!(report.model_requests, 0);
    }
}
