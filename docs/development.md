# Development reference

[Back to DeepHoney](../README.md)

## Researcher interface

Alternative loops implement both preview and execution:

```rust
pub trait Researcher: Send + Sync + 'static {
	fn plan(&self, cfg: Config, req: Request) -> BoxFuture<anyhow::Result<ResearchPlan>>;
	fn run(&self, cfg: Config, req: Request, tx: Tx) -> BoxFuture<anyhow::Result<Report>>;
}
```

The installation point in `crates/app/src/main.rs` is:

```rust
let researcher: Arc<dyn engine::Researcher> = Arc::new(engine::DeepResearch);
```

`Request` contains `topic`, `mode`, `depth`, `breadth`, and an optional edited `ResearchPlan`. `ResearchPlan` contains `done` and a list of `Lens { name, question, sources, queries }` values. Existing serialized requests and ledgers remain readable through defaults for newly added fields.

Executables embedding the engine must call `if engine::document::run_pdf_worker() { return; }` before creating a runtime or starting their UI. The worker dispatches PDF parsing in the same executable. Adapt the return value to your main signature. The included desktop, CLI, and evaluation entry points already do this.

The app executes the loop on tokio. `Plan` and `Round` events describe planning and yield. `Query`, `Page`, `Claim`, `Pivot`, `Ghost`, `Status`, and `Warn` populate the feed. `Stats { pages, sources, voices, claims }` updates counters. Return `Report { topic, mode, markdown, ledger, ghosts, metrics }` for persistence and the report views; return `Err` for an error banner.

| Need | Building block |
|---|---|
| Model text or JSON | `Llm::new(&cfg)`, `.text(...)`, `.json::<T>(...)` |
| Search | `Source::for_mode(mode, &cfg)`, `source.search(&net, &cfg, query)` |
| Read a page | `Net::for_mode(&cfg, mode)?`, `net.page(url)` |
| Identifiers and primary links | `extract::identifiers(text)`, `extract::is_primary_link(url)` |
| Evidence | `Ledger::add_source`, `add_claim`, `add_evidence`, `contradict`, `ranked`, `coverage` |
| Archives | `ghost::resurrect(&net, url)`, `ghost::diff(&net, url, current_text)` |
| Report | `report::write(...)`, `report::write_with_verifier(...)` |
| Document sections | `extract::chunks(text)` for Unicode character ranges; `Page::chunks()` adds trusted PDF page spans |
| Evaluation | `evaluation::validate_cases`, `evaluation::grade`, batch example |

Related loops to study: [dzhng/deep-research](https://github.com/dzhng/deep-research), [GPT Researcher](https://github.com/assafelovic/gpt-researcher), and [Stanford STORM](https://github.com/stanford-oval/storm).

## Code layout

| Path | Responsibility |
|---|---|
| `crates/engine/src/lib.rs` | Modes, configuration, plans, requests, events, and the Researcher trait |
| `crates/engine/src/research.rs` | Planning, ranked leads, parallel workers, bounded rounds, review, and reports |
| `crates/engine/src/llm.rs` | Anthropic and OpenAI-compatible model clients |
| `crates/engine/src/net.rs`, `tor.rs` | Paced HTTP, strict onion routing, and embedded Arti transport |
| `crates/engine/src/document.rs`, `browser.rs` | Isolated PDF worker and optional Chrome reader |
| `crates/engine/src/evaluation.rs` | Case validation, structural checks, and human grading rubric |
| `crates/engine/src/extract.rs` | Readable text, links, and identifier extraction |
| `crates/engine/src/sources/` | Twenty registered source adapters |
| `crates/engine/src/ledger.rs` | Claims, evidence, voice clusters, confidence, and provenance |
| `crates/engine/src/ghost.rs` | Wayback recovery, dates, and historical text differences |
| `crates/engine/src/report.rs` | Statement validation, verification, deterministic sections, and citations |
| `crates/engine/src/store.rs` | SQLite session storage |
| `crates/engine/examples/cli.rs` | Headless research entry point |
| `crates/app/src/` | Native UI, theme, settings, and Keychain storage |

## Validation

Run the normal checks on macOS:

```sh
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
```

On a disk-constrained machine, prefix Cargo commands with `CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0` to reduce generated debug artifacts. This keeps runtime assertions and tests enabled.

For a broad UI or dependency change, also run `cargo build -p app --locked` and exercise the affected flow in the native app. CI runs workspace tests and strict Clippy on macOS with stable Rust. Existing formatting varies by file; match the surrounding style rather than reformatting the workspace.

Local validation passed 139 Rust tests: 103 engine unit tests, 16 HTTP research integration tests, 7 retrieval recovery tests, 4 app settings tests, 1 evaluation-runner test, 5 provider-diagnostic tests, and 3 model-diagnostic tests. Five engine tests are excluded by default: three live-network diagnostics and two installed-browser tests. Thirty-four Python tests cover blind comparison, app packaging, OCR metrics and repeated provider diagnostics. CI runs all Rust targets and the Python checks; this local pass does not establish hosted CI status.

With Chrome or Chromium installed, run the two browser integration tests explicitly:

```sh
cargo test --workspace --lib browser::tests -- --include-ignored --test-threads=1 --nocapture
```

Both passed locally. They exercise JavaScript rendering, compressed HTML, final URLs, forbidden requests and scripts, response bounds, and cancellation cleanup. The normal reader never uses the browser in Dark mode.

The final packaged executable completed plan/run against an isolated local model/search/scanned-PDF fixture. Claims and reports showed the real OCR quotation with page locations, and SQLite preserved run metrics. Settings exposed the new opt-in control; direct toggle/save automation was not completed because the native automation scroll API returned `noWindowsAvailable`. CLI opt-in and shared configuration paths passed. Earlier native checks covered verifier/browser settings. CLI JSON export and offline evaluation also passed, including expected partial failures and overwrite refusal. The initial alpha checks covered plan editing, reopening sessions, Keychain persistence, and a synthetic Deleted view. These checks do not establish live model quality or live historical deletion accuracy.

Opt-in public-service diagnostics:

```sh
cargo test -p engine --locked public_source_smoke -- --ignored --nocapture
cargo test -p engine --locked live_ahmia_mirror_over_builtin_tor -- --ignored --nocapture
```

A live probe fetched 2,389 text characters from Ahmia's onion about page through embedded Arti. Another onion service failed to provide a descriptor. Nonempty search results came from OpenAlex, arXiv, Crossref, Hacker News, Stack Exchange, crt.sh, MuckRock, and Wayback. DuckDuckGo returned a bot challenge, Semantic Scholar returned HTTP 429, Telegram timed out, and the Common Crawl index returned HTTP 504. The smoke diagnostic reports each adapter's outcome; a successful diagnostic exit does not mean every provider worked.

Live cloud-model investigations and paid or token-gated providers remain unverified without credentials. Live HTTPS onion behavior, exhaustive window-size testing, signing, and notarization were not checked. The build currently emits a future-compatibility notice for transitive `block v0.1.6`.

Model calls return complete JSON responses. The native live feed streams research events, not model tokens. Displayed budgets are request/page bounds, not dollar estimates. These are current implementation choices.


Run the reproducible document-flow acceptance after building the CLI example:

```sh
cargo build --workspace --bins --examples --locked
python3 scripts/qa/document_flow.py --out .scratch/document-flow-check
```

The output directory must be new. This fixture runs real native OCR and a local synthetic image-model response, checks PDF evidence locations and shared usage, and recovers from a temporary PDF HTTP failure. It makes no paid-provider calls and does not measure model accuracy. See [blind comparisons](../evaluation/comparison/README.md), [provider diagnostics](provider-health.md), and [macOS installation](installation.md) for their separate checks.

The 2026-10-18 final public probe returned nonempty results from OpenAlex, Crossref, Hacker News, Stack Exchange, crt.sh, MuckRock and Wayback. arXiv timed out; Semantic Scholar returned HTTP 429; DuckDuckGo and Telegram failed; Common Crawl returned HTTP 404. That probe led to fixes for its literal MIME filter and strict recognition of the upstream no-captures response; the latter now counts as an empty result rather than an outage. Initial corrected live queries encountered HTTP 504. A later focused probe with the corrected adapter returned eight Common Crawl results in 10.5 seconds (two requests, no retries); this confirms that query worked at that moment, not sustained availability. Eight configured/opt-in adapters were skipped. An earlier probe had six successes and six errors, demonstrating variability. Provider errors produced a nonzero diagnostic exit while preserving every row. These observations do not establish relevance, sustained availability or research quality.


### Product-readiness follow-up (2026-10-18)

Blind packets now freeze exact claim statements and neutral locations before independent grading. The readiness command refuses synthetic or undersized comparisons; completion, timing and cost readiness remain separate. No real competitor study was completed: actual exports and independent human grades are still absent.

The native OCR benchmark compiles the same Swift helper source used by Cargo and measures five synthetic raster PDFs. It exposed an orientation bug where upside-down gibberish won by character count. A high-confidence English-language signal now improves that selection while retaining a count fallback. Clean and rotated fixtures improved from zero exact word/numeric recall to 94.9% word recall and 80% exact numeric recall. Low-resolution, low-contrast and small-print results remain separately visible; this does not establish document-level accuracy or correct table/chart associations. See [the corpus and measured limits](../crates/engine/tests/fixtures/VISUAL.md).

```sh
python3 -m unittest discover -s scripts/qa -p 'test_*.py' -v
python3 scripts/qa/ocr_benchmark.py --out .scratch/ocr-readiness-check
```

CI runs the Python checks and native OCR regression corpus. The clean/rotation gates detect the observed orientation regression; they are not a production quality certification. Actual local benchmark results are in ignored scratch output and retain the expected and observed token differences.

A three-run live Crossref diagnostic returned three nonempty responses (median 789 ms, nearest-rank p95 835 ms), with no retries. This is a short fixed-query smoke result, not sustained availability. The new model diagnostic exercises transport and a tiny JSON contract only, with no live paid call in this pass. Optional Developer ID/notarization orchestration passed mocked negative-path checks; no genuine signing or Apple submission occurred. See [provider diagnostics](provider-health.md) and [installation](installation.md).


### Research trust and desktop usability follow-up

The current local workspace passed 139 Rust tests (five opt-in tests remain ignored), 34 Python checks and strict Clippy. The report gate now rejects unsupported digit quantities and recognized sign/unit changes before model verification. Regression cases cover leading decimals, percentages, Unicode minus, accounting negatives, ISO dates and spaced numeric ranges. Independent review caught two parser edge cases; both reproduced before fixes and passed afterward. Literal agreement is not semantic entailment or factual truth.

Actual HTTP fixture regressions bound outage dispatches to six rather than twelve while retaining four concurrent searches; successful empty results reset the failure counter, cancellation closes an in-flight search, and a fresh investigation tries previously paused adapters again. Reports disclose reduced source coverage.

Native desktop QA through CUA used an isolated profile containing only synthetic saved reports, with no credentials copied. The model setup hint and button were visible; clicking the button and pressing Plan without configuration opened Settings. Copy report changed to Copied, and pasting into the question field showed the expected saved Markdown prefix. Keyless local-model setup is covered by the configuration unit test. The existing offscreen Settings Close control did not activate through AX; the visible Settings toggle closed the panel. No settings were saved or paid network calls made. Native narrow-window/mobile layout was not validated.

The OCR precision experiments did not demonstrate a reliable recognition improvement. A benchmark generator media-box bug was fixed and its new geometry asserted; corrected-corpus results must not be attributed to OCR-engine changes. Six native cases completed, including a literal-punctuation negative case, with all declared regression checks passing. Degraded/small-print metrics remain poor and explicit. See [the measured corpus limitations](../crates/engine/tests/fixtures/VISUAL.md).
