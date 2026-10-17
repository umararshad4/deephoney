# Research trust and everyday usability

**Goal:** Improve the actual research product toward the user's requested 8/10 standard without manufacturing ratings or benchmark results.
**Approach:** HOLD. Reuse the report structural gate, bounded search stream, native OCR and GPUI controls. Minimal option is more validation tooling, which would not fix everyday behavior. Selected approach fixes demonstrable failure modes and usability friction. A replacement research engine or paid hosted evaluation service is unnecessary.
**Design:** User delegates implementation priorities after the comparison identified evidence quality and reliability gaps.

## Constraints
- Preserve all existing working-tree changes. No commits, pushes or publication.
- No live paid/model calls without configured credentials; none are present in the inspected environment or default desktop config.
- No truth-string substitutions in OCR or lowered benchmark thresholds to create an apparent improvement.
- A model's approval must not override structural numeric evidence checks.
- Failed adapters must not silently remove coverage, disable future investigations or slow healthy searches unnecessarily.
- App setup hints must permit keyless local OpenAI-compatible endpoints and never expose keys in clipboard exports.
- Ratings remain editorial until real independent output comparisons exist.

## Existing code and file map / parallel lanes
| Lane | Files | Deliverable |
|---|---|---|
| OCR | native/document-reader.swift; OCR benchmark scripts and fixture docs | Measured numeric/text improvement if an image-recognition change actually works; otherwise retain baseline and evidence |
| Reports | engine/src/report.rs | Conservative numeric-support guard and durable machine-transcription cautions |
| Recovery | engine/src/research.rs; tests/research.rs | Run-local failing-adapter circuit breaker with visible coverage warning |
| Desktop (parent) | app/src/settings.rs; app/src/ui.rs | Actionable first-run model setup and copyable finished report |
| Integration (parent) | CI/docs/checks | Verify combined behavior and rebuild local artifact |

## Review focus
- Numeric sign, unit or percentage drift accepted by an overpermissive model verifier.
- Valid comma-grouped values or paraphrases rejected by an overbroad guard.
- Failed adapter drains search budget; healthy/empty responses must reset failures.
- Setup hints block valid keyless local models or miss a blank model.
- Copy action exposes credentials or claims an old report belongs to the currently running topic.

## Tasks
- [x] Add numeric support rejection tests and guard; keep semantic support check for meaning/negation.
- [x] Persist machine-transcription cautions in generated reports.
- [x] Add run-local failing-adapter recovery limits, cancellation and healthy/empty reset tests.
- [x] Spike bounded OCR quality improvements against unchanged truth; ship only measured improvements.
- [x] Add setup validation/hint and report-copy action; unit and native UI checks.
- [x] Run combined regression checks, review changes and build/package current source.

## External evidence still required
- Independent human comparisons and representative real-document OCR validation.
- Real-model output validation with configured credentials; no credentials are configured locally.
- Apple credential-backed notarization and first-launch proof remain external from the preceding pass.


## Receipt / rulings
- Numeric guard: before/after tests catch invented values, changed signs/units, leading-decimal tenfold drift and spaced range handling. Independent review found two parser defects, both fixed and rechecked; no residual blocker found. Semantic negation/value-entity association still require the verifier.
- Source circuit: actual HTTP tests bound a 12-query outage to at most six dispatched calls while retaining concurrency four. Three consecutive completed failures pause an adapter; successful empty responses reset it before pausing. Fresh runs reset state; saved reports disclose reduced coverage. No same-run recovery probe (each failed logical read already has bounded retries).
- OCR: rejected higher resolution, cropping, contrast, language-correction and accurate-revision experiments because they regressed cases or timed out. No native recognition change shipped. Fixed a benchmark media-box bug; six corrected cases completed with declared regression gates passing. The corpus correction is not an OCR accuracy gain.
- Desktop: first-run setup and missing-config Plan route to Settings; keyless OpenAI-compatible configuration accepted by unit test. CUA verified Setup action and Copy report/Copied plus expected Markdown paste prefix in an isolated synthetic profile. The visible Settings toggle worked; existing offscreen Close AX activation did not. Narrow/mobile UI sizes not validated.
- Combined: 139 Rust tests, 34 Python tests, strict Clippy and workspace binary/example builds passed. Actual native OCR/visual/retry CLI integration passed with durable machine warning, three retrieval attempts, one retry and one recovered request. Final ad-hoc app packaged and verified at `.scratch/installation/research-trust-final`.
- Ruling: preserve the working native OCR baseline rather than ship a heuristic that improves one example and loses numeric information elsewhere. Independently transcribed real document evidence is still needed.
- Ruling: copy exports only existing report Markdown; no configuration, credentials or raw run environment is copied.
- The user's 8/10 target is not certified by these engineering checks. Live-model validation, independent comparisons, representative OCR accuracy and credential-backed notarization remain external gaps.
