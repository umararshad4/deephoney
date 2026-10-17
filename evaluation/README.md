# Research quality evaluation

This workflow produces repeatable artifacts for **structural checks**, **literal reference coverage** and **blind human grading**. It does not produce a factual-accuracy score. No competitive benchmark results are included.

## Validate without network calls

From the repository root:

```sh
cargo run -p engine --example evaluate --
cargo run -p engine --example evaluate -- --case battery-storage
```

With no action flag, the runner only validates and lists the suite. It does not read provider configuration, contact a service or create output artifacts. Compilation can download Rust dependencies if not already cached.

The 12 questions in `cases.json` span technical literature, public records, regional comparisons, counterevidence and archived documentation. They are candidate tasks, **not gold answers**. The Dark case needs `DEEPHONEY_TOR=1`; it is not included automatically when selecting another case.

## Run a bounded investigation

Configure the same environment variables as the headless engine: `ANTHROPIC_API_KEY`, or `OPENAI_BASE_URL`, `MODEL` and `OPENAI_API_KEY`, plus any search-provider configuration. Local loopback OpenAI-compatible models can run without an API key. A non-loopback endpoint requires a key. Optional verifier and browser environment variables use the engine's normal configuration.

```sh
cargo run -p engine --example evaluate -- \
  --case battery-storage --run --out evaluation-runs/battery-01
```

Create the parent directory first (`mkdir -p evaluation-runs`). The output directory itself **must not already exist**, including as a symlink. Every report is also created with an exclusive write; the runner never silently replaces an earlier report. Running the whole suite requires omitting `--case`; this can make many model and search requests and incur provider charges. No requests happen without `--run`.

Case IDs use lowercase ASCII letters, digits, hyphens and underscores, begin with a letter/digit, and are unique. The artifact names `cases`, `results` and `run` are reserved. Unknown fields and invalid suite bounds are rejected before research.

Each case sets depth (1–5), breadth (1–6), page-attempt budget (1–1000), mode and a whole-case timeout (1–3600 seconds). The supplied cases use 2 rounds, breadth 2, 12 page attempts and 600 seconds. Limits bound work; they do not guarantee adequate research. A timeout is recorded as a case error, not a successful quality result.

## Grade saved reports offline

The engine CLI's `--json` output can be saved as `<case-id>.json`. Offline grading reads those files, copies successfully parsed reports to a fresh output directory and runs no model/search requests:

```sh
cargo run -p engine --example evaluate -- \
  --case battery-storage --reports path/to/saved-reports \
  --out evaluation-runs/offline-01
```

The optional `expected` object in a case has `facts`, `source_urls` and `forbidden_assertions` arrays. Supply reference strings from a documented, human-checked reference set; do not generate expected answers using the same research model. Fact/forbidden checks match case-insensitive, whitespace-normalized substrings **in report Markdown**, not semantic meaning. Source checks compare ledger URLs after removing fragments and normal URL parsing. They do not equate mirrors or redirected URLs.

These checks are intentionally brittle: paraphrases can be missed, and a forbidden phrase inside a quotation or negation can be flagged. A match does not prove correctness, entailment or endorsement. Empty expectations mean “not evaluated,” not perfect coverage. Review every mismatch semantically.

### Controlled artificial self-check

`controlled-cases.json` and `fixtures/meridian-reference.json` are synthetic examples, with no claim about a real Meridian project. The report deliberately omits the new run metrics to exercise legacy-report compatibility.

```sh
mkdir -p evaluation-runs
cargo run -p engine --example evaluate -- \
  --cases evaluation/controlled-cases.json --reports evaluation/fixtures \
  --out evaluation-runs/controlled-01
```

Expected: one report graded, one missing-report error, and a **nonzero exit status**. Successful artifacts remain available. The reference report has one matching fact string, one expected source URL and no forbidden phrase. This is an instrumentation self-check, not a research-quality result.

## Artifacts and interpretation

| File | Meaning |
|---|---|
| `run.json` | Engine version, action and safe model names/feature flags; endpoints and keys omitted |
| `cases.json` | Exact selected questions and budgets; no credentials |
| `<id>.json` | Full report, ledger, provenance and engine metrics |
| `<id>.events.jsonl` | Live engine events, written and flushed incrementally; live mode only |
| `errors.jsonl` | Per-case failures; the batch continues after provider, parse or missing-file errors |
| `results.jsonl` | Per-case results flushed as they complete, preserved if a later case/process fails |
| `results.json` | Final structured results; includes all structural checks and reference mismatches |
| `results.csv` | Compact spreadsheet summary, with quoted cells and formula-prefix neutralization |

`completed` means a report was loaded or generated and graded. It does **not** mean a check passed or a conclusion was true. Errors have no structural/coverage scores. A batch with any case errors exits nonzero after preserving successful outputs.

Structural counts inspect source URL syntax, claim-to-source index bounds, presence of recorded quotations, numeric citation bounds, and contradictory-claim links/verdict presence. They do not fetch sources, check quote origin, judge whether a quote supports a claim or infer whether all contradictions were discovered. Numeric `[n]` or comma-group `[n, m]` matching (including legacy `S` prefixes) is syntactic, includes source-list labels and may count ordinary bracketed numbers. Occurrence counts refer to bracket groups; a group is invalid if any member is out of range. A “supported” verdict alongside conflict is flagged for inspection; it is not automatically a defect.

Metrics come from the report: elapsed time, search requests, page attempts, model calls and provider-reported tokens. Unknown token counts stay `null`/blank; no prices are invented. Old reports default missing counters to zero, which means unavailable historical telemetry, **not a measured free or instantaneous run**. `separate_verifier` describes configured separation, not verified epistemic independence.

Reports, queries, errors and event files can contain sensitive research subjects and retrieved text. Store these locally, review before sharing, and do not commit real research outputs or API credentials. The runner records only the listed model names and feature flags, not endpoint URLs, credentials or raw environment configuration. Record custom endpoint identity and provider/model versions separately in private evaluation notes. It cannot recover from disk failure or a killed process; incremental logs preserve what was successfully written.

## Executable blind comparison

Use [the comparison coordinator](comparison/README.md) to prepare randomized Markdown/PDF packets, validate independent human worksheets and summarize citation support, factual errors, missed reference evidence, completion, time and observed cost. It retains failed attempts, unknown measurements and grader disagreement. It cannot replace real competing reports or independent human review.

## Blind comparison rubric

Copy the header-only `human-scores.csv` for grader worksheets; leave unavailable measurements blank and explain them in `measurement_limitations`. Do not populate quality scores until a human has checked the report.

Use the same question, evaluation date, reference packet and approximate time/spend budget for every platform. Save the exact prompts, platform/model/version, settings, start/finish times, failures and raw exported reports. Exported reports from other platforms can be graded by humans directly; do not invent a DeepHoney ledger for them or compare platform-specific structural counts as accuracy.

Have a coordinator randomize report IDs and hide platform names from two independent graders. Remove only identifying metadata; preserve citations and substantive content. Have graders record source URLs and a short reason for each judgment. Resolve disagreements only after independent scores are saved. Compare at least 10 diverse cases, repeat stochastic runs where affordable, and publish failures and limitations alongside results. Do not claim statistical superiority from this small candidate suite.

| Human criterion | 0 | 1 | 2 | 3 | 4 |
|---|---|---|---|---|---|
| Citation entailment | Unsupported or unusable | Many citations fail | Mixed support | Most material claims supported | Every sampled material claim directly supported, with qualifiers intact |
| Factual correctness | Materially misleading | Multiple major errors | Some material errors | Minor errors only | No error found against the checked reference set |
| Counterevidence / uncertainty | Decisive contrary evidence omitted | Mostly one-sided | Some limitations considered | Main credible objections addressed | Competing evidence weighed fairly with unresolved issues explicit |
| Usefulness / coverage | Does not answer | Large gaps or unusable structure | Partially useful | Answers the main question clearly | Decision-relevant, well-scoped and clear about remaining gaps |

For citation entailment, sample the same number of material claims from each report, including the main conclusion and all high-stakes claims; open the cited sources and inspect context. Record the sample size and unsupported/partially supported/supported counts. For correctness, check claims against a human-curated independent reference packet, not another model's judgment. Score counterevidence only when the reference packet identifies what matters. “No error found” is bounded by what graders actually checked.

Record latency, reported tokens, observed charges and failure/completion rates separately from the four quality criteria. Use `unknown` when the platform does not expose a measurement. Never infer monetary cost from token counts without dated provider prices and applicable caching/rate information. Report per-case results and grader disagreement; avoid a single opaque “best tool” score.
