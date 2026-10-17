# Research quality, document coverage, and reliability

**Goal:** Make research measurable, read scanned/visual documents with explicit provenance, recover from temporary retrieval failures, and simplify macOS installation.
**Approach:** HOLD the user's three requested outcomes. Extend the existing evaluation runner, bounded PDF worker, network client, and native app. Reuse native macOS document APIs and existing model transports rather than add a second research service.
**Design:** The user's three-item implementation request.

## What already exists

The current local tree has source-specific evidence, a separate verifier, bounded text-PDF and Chrome readers, JSON run metrics, an offline grader, and a human comparison rubric. Preserve those changes and their compatibility defaults. Existing tests prove local behavior, not real-world research accuracy.

## Alternatives considered

- Minimal: add instructions for manual OCR, grading and source builds. This leaves the requested product gaps unresolved.
- Selected: implement bounded visual-document support, a usable blind study workflow, retry/recovery checks and a local installable app bundle.
- Larger: hosted benchmark service, new document-processing service and signed automatic updates. These require new infrastructure or credentials and are unnecessary for this pass.

## Global constraints

- No fabricated human grades, competitor reports, factual accuracy scores or charges. Unknown cost remains unknown. Preserve failures in comparisons.
- No paid-provider runs without available explicit test configuration. Public read-only provider diagnostics may run without credentials.
- Keep the existing uncommitted changes. No commit, push, release publication or tag change in this pass.
- Bound untrusted documents, subprocesses, retries, time and outputs. Preserve parser-generated page provenance and disclose OCR/visual uncertainty.
- Do not bypass challenges, weaken TLS, route onion requests outside Tor, or retry non-idempotent operations indiscriminately.
- Installation must preserve existing app/data, refuse accidental overwrite, derive its version from Cargo and disclose signing status.

## Parallel lanes

| Lane | Owned files | Depends on | Check |
|---|---|---|---|
| Document coverage | engine document.rs, llm.rs, new visual helper/module and native source, document fixtures | Parent integrates configuration and Net call if necessary | Actual scanned/table/chart fixtures, negative/timeout/grounding tests |
| Blind comparison | new evaluation comparison CLI/module/scripts, evaluation reference fixtures and instructions | Existing Report/evaluation exports, no edits to existing engine files | Blind assignment, human-grade validation, errors/coverage/time/cost summaries and controlled run |
| Installation | scripts/macOS packaging/install, packaging assets and install docs | Current app binary and Cargo metadata | Build a real app bundle, verify manifest/executable/version, isolated install/overwrite guards |
| Parent reliability | net.rs, research integration tests, live provider diagnostics, shared config integration, root docs | Document lane interface | Transient failure recovery, permanent error refusal, bounds/cancellation, live diagnostics |

## Tasks

- [x] Implement and verify bounded OCR and table/chart evidence support, including provenance and explicit failure/uncertainty paths.
- [x] Implement and verify blind comparison preparation, validated human judgments, and summaries for citation support, factual errors, missed reference evidence, completion, time and observed cost.
- [x] Run a controlled comparison self-check and public provider diagnostics; record their actual limits. Keep real competitive quality pending without independent human grades.
- [x] Implement and verify bounded transient retrieval recovery and provider failure reporting without swallowing failures.
- [x] Produce and verify a macOS app bundle and safe user-level install workflow.
- [x] Integrate shared interfaces, document behavior, review the combined diff, and run full tests/Clippy/build/security and native checks appropriate to changes.

## Review focus and failure modes

| Path | Failure | Required behavior/proof |
|---|---|---|
| Visual document | scanned, mixed, rotated, unreadable, encrypted, huge | Bounded reader, page references, OCR provenance, named errors; no invented chart values |
| Human study | missing/duplicate grades, unblinded files, failed runs, missing costs | Refuse malformed results; preserve denominators and unknowns; keep key separate |
| Retrieval | temporary 429/5xx/reset, Retry-After, challenge or permanent 4xx | Bounded idempotent retry; no challenge bypass; cancellation stops waiting |
| Installation | old app exists, paths contain spaces, missing helper/toolchain | Safe refusal and actionable message; no user data deletion |

## External validation boundaries

A blind comparison needs real competing reports and independent human graders. Tooling and synthetic fixtures can be completed locally; they cannot establish competitive superiority. Developer ID signing/notarization requires an identity and account credentials; local app packaging must disclose when unsigned/ad-hoc signed.


## Verification receipt and rulings

Local implementation and validation completed on 2026-10-18; all changes remain uncommitted on `feat/research-quality`. No publication or tag change occurred.

- Rust workspace/all targets: 126 tests passed, five engine tests ignored by default. Two installed-Chrome tests also passed explicitly.
- Strict workspace/all-target Clippy and all app/example builds passed. The existing transitive `block v0.1.6` future-compatibility notice remains.
- Eleven comparison tests and two packaging tests passed. The synthetic comparison CLI preserves failed attempts, unknown cost and disagreement; it does not establish actual research quality.
- Real native OCR passed scanned, rotated, mixed-document and optional image-transport fixtures. A high-entropy PDF retained OCR when its optional PNG exceeded the size bound. Numeric checks cover signs, accounting parentheses, currency and percentage markers.
- `scripts/qa/document_flow.py` exercised the actual CLI with real OCR, a synthetic image-model response, quoted PDF evidence, shared usage and a temporary HTTP PDF failure. Saved evidence retains machine-transcription provenance.
- The installed app completed a scanned-PDF investigation against an isolated local model/search fixture. Report and Claims display and persisted metrics were inspected. Settings control presence was verified through accessibility; direct toggle/save automation was skipped because scrolling returned `noWindowsAvailable`. The CLI opt-in and configuration compatibility paths passed.
- Header failures and interrupted page/search bodies recover within one three-attempt, 75-second budget. Replayed generic POSTs, permanent errors and long Retry-After values are refused. Cancellation, size caps, shared retry budgets and accounting passed.
- The final broad public-provider diagnostic returned seven nonempty results, five errors and eight configuration/opt-in skips. It exited nonzero and preserved all rows. The initial broad probe returned six results and six errors. No paid adapters or cloud models were invoked.
- Probing exposed a Common Crawl MIME-filter bug and a legitimate no-captures 404 misclassification. Both were fixed with HTTP regression tests; unexpected/invalid-index 404s remain errors. Initial corrected queries encountered upstream 504; a later focused live probe returned eight results in 10,481 ms with two requests and no retries (exit 0). Sustained availability is not claimed.
- A versioned app bundle, ZIP, manifest and checksums were produced and installed in an isolated user directory. It includes the MIT license, passes hash/metadata/ad-hoc signature verification, and refuses existing apps or unsafe destinations. The package is not notarized.
- Two independent read-only reviews found body-recovery, numeric-sign, oversized-image and license-notice defects; all were fixed and their checks rerun. CI now tests all Rust targets and both Python suites. Hosted CI was not run.

Ruling: use the native fast English-focused CPU OCR path because actual accurate-mode requests stalled in Apple's internal model compiler on this host. Four orientations are attempted heuristically; recognition and visual associations can still be wrong. Native readers serialize to bound resource use and avoid an observed concurrent helper-startup stall. Limits and timeouts remain enforced.

Ruling: preserve OCR/text when optional model interpretation or image encoding fails. Never infer unlabeled chart values. Preserve direct numeric signs and units and persist machine-transcription notices in evidence locations.

Ruling: retain unknown charges rather than estimate prices from token counts. A comparison remains a set of human judgments on checked samples, not an automatic truth score.

## Remaining external quality study

- [ ] Run an actual blind comparative study with real competitor reports and independent human grades.
  - Pending: the user asked to locate an existing folder. Searches of common local document folders and project evaluation artifacts found generated synthetic packets and a blank scoring template, but no suitable competitor exports with independent grades. No client scorecard or synthetic report was substituted. Search evidence is retained locally in `.scratch/comparison-file-search.json`.
  - Required evidence: matched actual platform reports/failed attempts, observed timing/cost where available, an attributed independently reviewed reference packet, and at least two independent human worksheets. Do not claim competitive accuracy until this exists.

Paid model/search-provider testing and Developer ID signing/notarization remain unverified without explicit test configuration and signing credentials.
