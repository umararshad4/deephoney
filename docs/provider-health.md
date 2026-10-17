# Search provider diagnostics

The `provider_health` example runs a bounded representative search through the existing adapters. It records failures instead of treating a smoke-test exit as proof that every service works. It does not call a model, read result pages, calculate factual accuracy, or test the complete research loop.

## Safe default

```sh
cargo run --locked -p engine --example provider_health
```

This lists all 20 adapters without reading environment configuration, creating an output file, or making network requests. `--provider arxiv` limits the list. Unknown names, unknown/repeated flags, conflicting run flags, and missing output paths are rejected before network work.

## Public diagnostics

```sh
cargo run --locked -p engine --example provider_health -- \
  --public --out .scratch/provider-health-public.json
```

Create the output parent directory first. The file must not exist, including as a symlink. Public mode uses `Config::default()`, ignoring provider credentials and custom endpoints in the environment. It tests these 12 adapters and records the other adapters as skipped:

DuckDuckGo, OpenAlex, Semantic Scholar, arXiv, Crossref, Hacker News, Stack Exchange, certificate transparency, MuckRock, Telegram, Common Crawl, and Wayback.

Queries are fixed: `distributed systems` for general research, `example.com` for domain/archive adapters, `@telegram privacy` for Telegram, and `10.1038/nature14539` for Crossref. These are availability probes, not relevance or recall benchmarks. Services can return challenges, rate limits, empty results, or outages; no bypass is attempted.

For one adapter:

```sh
cargo run --locked -p engine --example provider_health -- \
  --public --provider crossref --out .scratch/provider-health-crossref.json
```

## Configured services and Tor are explicit

`--configured` loads the same environment configuration as the headless CLI, including search-service credentials and the SEC contact. It may consume provider search credits. It does not read desktop Keychain settings. Missing credentials, missing SearxNG endpoint, or missing SEC contact are skipped with a reason. DuckDuckGo is skipped when a configured web adapter disables its fallback role.

```sh
cargo run --locked -p engine --example provider_health -- \
  --configured --provider brave --out .scratch/provider-health-brave.json
```

`--tor` is required independently of `DEEPHONEY_TOR`. It enables the Ahmia adapter with Dark-mode networking and the fixed `Tor Project` query. This probes Ahmia's filtered discovery over its public search interface. It does **not** bootstrap Tor or read an onion page, so success is not a Tor-connectivity check. Wayback uses Hidden mode; other adapters use Normal mode.

```sh
cargo run --locked -p engine --example provider_health -- \
  --public --tor --provider ahmia --out .scratch/provider-health-ahmia.json
```

## Interpret the output

The JSON contains `schema`, configuration type, total elapsed milliseconds, and rows sorted by provider. Each row records:

- Provider name and fixed query.
- `status`: `ok_nonempty`, `ok_empty`, `error`, or `skipped`.
- A bounded reason code, elapsed milliseconds, and returned result count.
- `requests`, `retries`, and `recovered` from that provider's `Net` instance.

No result titles, result URLs, configured endpoints, credentials, raw upstream error strings, or model responses are written. HTTP status is retained when available without exposing the request URL. A zero request count on a skipped row means no adapter request was attempted; Net counts attempts at its own request boundary, not every redirect hop.

Up to four adapters run concurrently. Each provider has a 45-second deadline; the adapter future is dropped at timeout. This may stop a retry sequence before Net's longer overall deadline. With 20 providers, a full run can take several batches. A failed adapter does not discard other rows: the output is written and synced, then the process exits 1 if any row has `error` or all rows were skipped. Invalid arguments exit 2. An all-skipped run exits 1 because no adapter was attempted. An empty-result run can exit 0, so inspect rows rather than treating exit 0 as an “all healthy” claim. Filesystem/write failures also exit 1 and can leave a partial output file; choose a new path for the next run.

Common Crawl validation note (2026-10-18): its CDX API uses literal substring filters unless `~` explicitly selects a regular expression. The adapter now requests `~mime:^text/(html|plain)$`. A bounded 404 JSON response containing only `message: "No Captures found for: <the exact requested URL>"` is an empty lookup, not an outage. An invalid collection, a mismatched message, HTML 404, or another error remains a failure. Live inspection confirmed the distinct no-captures and invalid-index responses; initial corrected-filter probes returned upstream 504 responses. A later focused `--public --provider commoncrawl` run returned `ok_nonempty` with eight results in 10,481 ms (two requests, no retries), exiting successfully. This confirms one working live query, not sustained availability. See the [official index API](https://index.commoncrawl.org/) and its linked CDX API reference.

A single successful response proves only that this adapter returned that response at the time of the run. Repeat across time and representative queries before relying on a service operationally. Use the separate blind-comparison workflow for research quality.

## Focused checks

```sh
cargo test --workspace --example provider_health --locked
```

These checks validate default list-only behavior, explicit opt-ins, argument refusal, public-adapter selection, missing-configuration skips, query/mode routing, and separate empty/nonempty statuses. They make no external requests.

## Repeated availability gates

A successful single probe is too weak to establish operational reliability. The stdlib matrix runner repeats the existing diagnostic, keeps every raw run, and writes a separate allowlisted summary:

```sh
cargo build --locked -p engine --example provider_health
python3 scripts/qa/provider_matrix.py run --public \
  --provider crossref --runs 3 --interval 30 \
  --out .scratch/crossref-matrix
```

The output directory must be new. Omit `--provider` to retain all adapters, or repeat it to select a subset. Explicit `--configured` replaces `--public` when intentionally allowing environment credentials and possible service credits; it does not load desktop Keychain settings. No model is called. Matrix runs do not enable Tor or bypass challenges. At most 20 repetitions are accepted, with at least one second between repetitions. Repeating every 30 seconds is a short smoke check, not evidence of availability over days; use separately recorded runs across relevant operating periods for that claim.

Default per-provider gates require at least three attempted runs, response rate at least 0.8, and nonempty-result rate at least 0.8. Both rates divide by attempted runs, including errors; skipped runs are reported separately and never count toward the minimum. Empty results count as responses but not as nonempty successes. A provider missing from an input run cannot pass. All-skipped, insufficient, or failed providers make the summary exit 1. Exit 2 denotes invalid input or orchestration failure. Custom `--minimum-runs`, `--response-rate` and `--nonempty-rate` are recorded in the summary; these are operator-selected gates, not statistical confidence or a product quality score.

Median and nearest-rank p95 latency include all attempted outcomes, including errors and timeouts. Counts of errors, skips, empty results, recoveries and request attempts remain visible. No query text, provider error text, endpoint, imported arbitrary metadata, or local source path is copied into the summary. Canonically identical run files are rejected to prevent accidentally counting the same observation twice; mixed public/configured reports and differing queries for one provider must be summarized separately.

Existing raw runs can be summarized without network or environment configuration:

```sh
python3 scripts/qa/provider_matrix.py summarize \
  .scratch/crossref-matrix/run-01-crossref.json \
  .scratch/crossref-matrix/run-02-crossref.json \
  .scratch/crossref-matrix/run-03-crossref.json \
  --out .scratch/crossref-summary.json
python3 -m unittest discover -s scripts/qa -p 'test_provider_matrix.py' -v
```

Three healthy responses for a fixed query demonstrate only those three observations. They establish neither relevance/recall nor independent research quality. Adapter selection, time of day, operating region, API plan, query diversity and number of observations affect the applicability of the result.

## Explicit model transport smoke test

Search diagnostics cannot validate a cloud model. A separate example exercises the same `Llm` JSON client and usage accounting as research. Its default invocation only lists the fast, strong and verifier slots; it reads no environment configuration and makes no request:

```sh
cargo run --locked -p engine --example model_health
```

To intentionally permit one configured model smoke request and possible charges:

```sh
cargo run --locked -p engine --example model_health -- \
  --configured --slot strong --out .scratch/model-strong.json
```

`--slot fast` tests extraction; `strong` tests planning/writing; `verifier` tests an explicitly configured verifier. The output must be a new file and is reserved before environment keys are read. An absent verifier is skipped rather than silently testing the writer twice. The example uses CLI environment configuration, never desktop Keychain settings. Each logical request is limited to 128 output tokens and a 60-second overall timeout; the existing client can retry retryable HTTP responses within this budget. This is not a hard monetary limit: input and provider billing rules still apply.

The fixed prompt requests a tiny exact JSON contract. Output statuses distinguish `response_valid`, `invalid_response`, `error` and `skipped`; only `response_valid` exits 0. A malformed JSON response or transport error is recorded with a static reason code. Outputs contain slot, timing, model HTTP attempt count and reported token counts; absent usage and actual monetary cost remain `null`. Neither API keys, configured model names/endpoints, raw error strings nor generated model text are printed or persisted. No price is inferred. Files on Unix are created with mode `0600`.

This tests endpoint compatibility and basic structured response handling. It does not validate model factual accuracy, visual support, long-context behavior, citation quality or the entire research loop. The offline tests use a real local HTTP server with synthetic OpenAI-compatible responses, including valid JSON, wrong values, malformed JSON and authorization failure. No cloud calls or provider charges are incurred:

```sh
cargo test --locked -p engine --example model_health
```

Measured local smoke (2026-10-18): three public Crossref repetitions returned three nonempty responses, with median 789 ms, nearest-rank p95 835 ms, three request attempts and no retries. The matrix default gates passed for this selected adapter. These closely spaced, fixed-query observations do not establish sustained availability or research accuracy. No paid search or real cloud-model request was made in this validation pass.
