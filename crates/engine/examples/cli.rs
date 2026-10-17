use anyhow::{Context, Result, bail};
use engine::{Config, DeepResearch, Mode, Request, Researcher};

fn main() -> Result<()> {
    if engine::document::run_pdf_worker() {
        return Ok(());
    }
    tokio::runtime::Builder::new_multi_thread().enable_all().build()?.block_on(run())
}

async fn run() -> Result<()> {
    let mut cfg = Config::from_env();
    let mut request = Request { topic: String::new(), mode: Mode::Normal, depth: 2, breadth: 3, plan: None };
    let mut args = std::env::args().skip(1);
    let mut words = Vec::new();
    let mut json = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--mode" => {
                request.mode = match args.next().as_deref() {
                    Some("normal") => Mode::Normal,
                    Some("hidden") => Mode::Hidden,
                    Some("dark") => Mode::Dark,
                    Some("adversarial") => Mode::Adversarial,
                    _ => bail!("--mode expects normal, hidden, dark, or adversarial"),
                }
            }
            "--depth" => request.depth = args.next().context("missing --depth value")?.parse()?,
            "--breadth" => request.breadth = args.next().context("missing --breadth value")?.parse()?,
            "--pages" => cfg.max_pages = args.next().context("missing --pages value")?.parse()?,
            "--json" => json = true,
            "--browser" => cfg.browser_enabled = true,
            "--visual-documents" => cfg.visual_documents = true,
            "--browser-path" => cfg.browser_path = args.next().context("missing --browser-path value")?,
            "--help" | "-h" => {
                println!(
                    "cargo run -p engine --example cli -- TOPIC [--mode normal|hidden|dark|adversarial] [--depth 1..5] [--breadth 1..6] [--pages 1..1000] [--json] [--browser] [--browser-path PATH] [--visual-documents]\nConfigure ANTHROPIC_API_KEY or OPENAI_BASE_URL, OPENAI_API_KEY and MODEL. Dark mode also needs DEEPHONEY_TOR=1. Report markdown (or full Report JSON with --json) goes to stdout; events go to stderr. --browser opts into isolated clearnet Chrome rendering; never used in Dark mode. --visual-documents sends bounded PDF page images to the configured planner/writer for table/chart interpretation; may incur model charges. Local scanned-PDF OCR is automatic on macOS."
                );
                return Ok(());
            }
            value if value.starts_with('-') => bail!("unknown option {value}"),
            _ => words.push(arg),
        }
    }
    request.topic = words.join(" ");
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let report = tokio::spawn(DeepResearch.run(cfg, request, tx));
    while let Some(event) = rx.recv().await {
        eprintln!("{}", serde_json::to_string(&event)?);
    }
    let report = report.await.context("research task stopped")??;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("{}", report.markdown);
    }
    Ok(())
}
