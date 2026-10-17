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

The app executes the loop on tokio. `Plan` and `Round` events describe planning and yield. `Query`, `Page`, `Claim`, `Pivot`, `Ghost`, `Status`, and `Warn` populate the feed. `Stats { pages, sources, voices, claims }` updates counters. Return `Report { topic, mode, markdown, ledger, ghosts }` for persistence and the report views; return `Err` for an error banner.

| Need | Building block |
|---|---|
| Model text or JSON | `Llm::new(&cfg)`, `.text(...)`, `.json::<T>(...)` |
| Search | `Source::for_mode(mode, &cfg)`, `source.search(&net, &cfg, query)` |
| Read a page | `Net::for_mode(&cfg, mode)?`, `net.page(url)` |
| Identifiers and primary links | `extract::identifiers(text)`, `extract::is_primary_link(url)` |
| Evidence | `Ledger::add_source`, `add_claim`, `contradict`, `ranked`, `coverage` |
| Archives | `ghost::resurrect(&net, url)`, `ghost::diff(&net, url, current_text)` |
| Report | `report::write(...)` |

Related loops to study: [dzhng/deep-research](https://github.com/dzhng/deep-research), [GPT Researcher](https://github.com/assafelovic/gpt-researcher), and [Stanford STORM](https://github.com/stanford-oval/storm).

## Code layout

| Path | Responsibility |
|---|---|
| `crates/engine/src/lib.rs` | Modes, configuration, plans, requests, events, and the Researcher trait |
| `crates/engine/src/research.rs` | Planning, ranked leads, parallel workers, bounded rounds, review, and reports |
| `crates/engine/src/llm.rs` | Anthropic and OpenAI-compatible model clients |
| `crates/engine/src/net.rs`, `tor.rs` | Paced HTTP, strict onion routing, and embedded Arti transport |
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
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
```

For a broad UI or dependency change, also run `cargo build -p app --locked` and exercise the affected flow in the native app. CI runs workspace tests and strict Clippy on macOS with stable Rust. Existing formatting varies by file; match the surrounding style rather than reformatting the workspace.

The initial local validation passed 60 tests: 47 engine unit tests, 10 HTTP research integration tests, and 3 app settings tests. Three live-network tests are excluded by default. Fixtures cover edited plans, evidence grounding, counterevidence budgets, saturation, report verification, persistence, cancellation, body limits, and onion routing.

Native UI checks exercised plan/edit/run, report views, reopening a session, settings, and Keychain persistence against an isolated local model/search fixture. The Deleted view used a synthetic archive example. These checks do not establish live model quality or live historical deletion accuracy.

Opt-in public-service diagnostics:

```sh
cargo test -p engine --locked public_source_smoke -- --ignored --nocapture
cargo test -p engine --locked live_ahmia_mirror_over_builtin_tor -- --ignored --nocapture
```

A live probe fetched 2,389 text characters from Ahmia's onion about page through embedded Arti. Another onion service failed to provide a descriptor. Nonempty search results came from OpenAlex, arXiv, Crossref, Hacker News, Stack Exchange, crt.sh, MuckRock, and Wayback. DuckDuckGo returned a bot challenge, Semantic Scholar returned HTTP 429, Telegram timed out, and the Common Crawl index returned HTTP 504. The smoke diagnostic reports each adapter's outcome; a successful diagnostic exit does not mean every provider worked.

Live cloud-model investigations and paid or token-gated providers remain unverified without credentials. Live HTTPS onion behavior, exhaustive window-size testing, signing, and notarization were not checked. The build currently emits a future-compatibility notice for transitive `block v0.1.6`.

Model calls return complete JSON responses. The native live feed streams research events, not model tokens. Displayed budgets are request/page bounds, not dollar estimates. These are current implementation choices.
