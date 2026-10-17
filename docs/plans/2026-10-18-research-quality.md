# Research quality upgrade plan

**Goal:** Fix the demonstrated retrieval and evidence limitations and provide a repeatable evaluation workflow.
**Approach:** HOLD the six improvements identified in the comparison. Extend the existing Net, extraction, ledger, Llm, report, and desktop settings rather than replacing the engine. The minimum approach would add only PDF reading; the selected approach also fixes evidence loss and adds a measurement seam. A new research service and full browser automation product are unnecessary.
**Design:** The user's request to implement the top missing capabilities from the preceding review.

## Global constraints

- Preserve existing saved sessions/settings through serde defaults.
- Keep onion reads on embedded Tor; never render onion pages or allow a browser fallback in Dark mode.
- Clearnet browser rendering is explicit opt-in, isolated from user profiles, GET-only at the request boundary, and bounded by timeout/request/DOM limits. Never bypass challenges, authenticate, or submit forms.
- Use stable pdf-extract 0.12.1 and chromiumoxide 0.9.1, checked against the official registry and documentation. Reuse tempfile already in the dependency graph.
- PDF downloads are at most 12 MiB; document text is at most 180,000 characters with an explicit truncation warning. Readers must inspect all retained text in overlapping sections, not silently keep only the beginning.
- Keep per-source evidence quotes when claim wording merges. Scores remain labeled heuristics.
- Verifier credentials are explicit or inherited only for the same provider/endpoint. No writer key may leak to a separately configured endpoint.
- Report and evidence review batch bounds must be visible and preserve global claim IDs. A separate verifier configuration does not guarantee truth.
- Evaluation reports distinguish structural checks, expected-fact coverage, and human factual judgments; no fabricated quality scores or competitor benchmark results.
- No paid/live model run without available configuration. Finish local work and report live validation separately. No repository publication or release retagging in this pass.

## What already exists

Net has paced HTTP, body limits, and strict onion routing. Extraction produces readable HTML/text. Research already has bounded workers, a ledger, counterevidence, gap checks, and cancellation. Reports have typed claim mappings and a separate checking call. Reuse these seams; fix their coverage and provenance limitations.

## Review focus

1. Quote appearing after character 14,000 or in the middle/end of a Unicode document must reach extraction.
2. Merged wording with different source excerpts must retain both, including after persistence.
3. A verifier endpoint must never inherit another endpoint's API key.
4. Browser resource redirects, POSTs, onion hosts, binary resources, timeouts, and cancellation must be refused or cleaned up.
5. Empty, encrypted, malformed, oversized, or image-only PDFs must produce useful errors, not claimed evidence.

## File map and parallel lanes

| Lane | Files | Depends on | Delivers |
|---|---|---|---|
| Documents | extract.rs, net.rs, new document.rs, PDF fixtures | Shared Config fields; browser.render interface | Bounded PDF/text retention and sections |
| Browser | new browser.rs only | Config contract and parent-owned dependencies | Isolated optional clearnet rendering |
| Evaluation | new evaluation.rs, examples/evaluate.rs, evaluation/ | Report.metrics contract | Repeatable runs and honest grading |
| Main, sequential | lib.rs, ledger.rs, llm.rs, research.rs, report.rs, app settings/UI, tests/docs | Integrate the other lanes | Quote provenance, full-document extraction, verification configuration, usage, and native controls |

### Task 1: Broader document coverage
- [x] Add PDF text extraction with page markers, bounded bodies, useful parse errors, and retained-text truncation warnings.
- [x] Add Unicode-safe 12,000-character windows with 250-character overlap; all retained characters are covered.
- [x] Verify: extraction/net unit fixtures, malformed/binary/oversized PDF rejection, and late-document evidence integration fixture.

### Task 2: Optional dynamic-page retrieval
- [x] Add isolated Chrome/Chromium renderer with bounded GET-only request interception, no onion targets, no user profile reuse, cleanup, timeout, and explicit opt-in.
- [x] Integrate as a fallback for script-heavy clearnet HTML only, with visible warnings.
- [x] Verify: policy negatives, real harmless local JS fixture when Chrome is available, and cancellation/process cleanup.

### Task 3: Evidence and verification
- [x] Store source-specific quotes/locators, preserve old session JSON, and expose them in the report/claim view.
- [x] Extract across all retained sections, preserve bounded calls and source URL evidence.
- [x] Process report evidence in batches rather than silently discarding everything after claim 80; disclose any output limits.
- [x] Add optional verifier provider/model/endpoint/key, same-endpoint-only credential reuse, and clearly labeled fallback self-checking.
- [x] Record model calls, reported token counts, search/read attempts, and elapsed time without inventing prices.
- [x] Verify: source-specific quote round trip, invalid verifier rejection/key isolation, later-batch evidence, counters, and existing integration fixtures.

### Task 4: Repeatable evaluation
- [x] Add machine-readable report output, evaluation cases, reports/events/results exports, and side-by-side human grading guidance.
- [x] Verify: correct/incorrect/missing evidence, malformed cases, unsafe case IDs, duplicate case IDs, partial failures, and old Report JSON.

### Task 5: Integrate and document
- [x] Add desktop verifier/browser controls, secret stripping, safe defaults, and compatibility tests.
- [x] Update README/reference/changelog with the actual new bounds and opt-in behavior.
- [x] Verify: workspace tests, strict Clippy, app build for this broad change, native settings/research flow, diff review and publication-safety checks.

## Failure modes

| Path | Failure | Handling and proof |
|---|---|---|
| PDF | encrypted/malformed/image-only/too large | Named error and negative fixture; no invented text |
| Long document | Unicode/window boundary/truncation | Overlap/coverage property checks and warning |
| Browser | forbidden request/redirect, missing executable, timeout/cancel | Deny at interception, visible warning, isolated process cleanup |
| Verification | wrong key endpoint or malformed audit | Validate before requests, no unsafe key inheritance, explicit error |
| Evaluation | partial model/provider failure | Preserve per-case errors and completed artifacts; never report failure as quality success |

## Not in scope

- A guarantee of perfection, lack of bias, or top-five accuracy; these require measured results.
- OCR, authenticated browsing, CAPTCHA bypass, arbitrary downloads, browser use over Tor, or paid data subscriptions.
- Running third-party commercial platforms using accounts we do not have. The evaluation workflow supports their exported reports and human scoring.
- Signed installers, automatic public releases, and bulk UI redesign; these do not fix research evidence quality.

## Rulings during verification

- PDF page locations come from parser-generated character spans, never page-like markers in untrusted text. This prevents HTML or PDF content from forging page numbers.
- Browser event handlers remain scoped to each render. An ownership guard kills and reaps Chrome before deleting its temporary profile, with a bounded synchronous cancellation cleanup. Deleting the profile first allowed Chrome to recreate files while exiting.
- Local verification uses debug-info-free, non-incremental Cargo profiles after ordinary debug artifacts exhausted the available disk. Runtime assertions remain enabled.


## Verification receipt

Completed locally on 2026-10-18. Changes remain uncommitted on `feat/research-quality`; no release or publication was performed.

- `cargo test --workspace --all-targets --locked`: 101 passed, 5 ignored. The ignored set contains three live-network diagnostics and two browser tests.
- `cargo test --workspace --lib browser::tests -- --include-ignored --test-threads=1 --nocapture`: all six browser tests passed, including the two installed-Chrome tests. Cancellation also passed three repeat runs.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: passed. The pre-existing `block v0.1.6` future-compatibility notice remains.
- `cargo build --workspace --bins --examples --locked`: passed. Cargo checks used debug-info-free, non-incremental local profiles after an earlier disk exhaustion.
- The rebuilt native app completed plan/run against an isolated local model/search/PDF fixture. Claims and reports showed the quotation with trusted PDF locations. SQLite preserved separate-verifier settings and metrics.
- The CLI produced valid Report JSON and JSON-line events. Its fixture run recorded five model attempts, 55 input tokens and 15 output tokens. These are synthetic provider counters, not a quality measurement.
- The native PDF worker returned parser page spans for a two-page fixture and a named malformed-PDF error for invalid input.
- Offline evaluation validated the suite, preserved one successful result alongside an expected missing-report failure, refused overwrite, and rejected an unconfigured live run before creating output.
- Independent diff reviews, local Markdown links, whitespace checks, and a Gitleaks scan of changed/new project files completed without unresolved findings.

Live cloud-model quality, paid providers, competitive blind grading, signing and notarization were not tested. Live Tor diagnostics from the initial alpha were not repeated in this pass. macOS PDF memory monitoring is sampled and can briefly exceed its threshold; use a sandboxed helper service if a hard allocation limit is required.
