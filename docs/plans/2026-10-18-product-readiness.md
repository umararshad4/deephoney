# Product readiness improvements

**Goal:** Close the software gaps behind trustworthy comparisons, document validation, provider reliability evidence and macOS distribution.
**Approach:** HOLD. Reuse the existing comparison coordinator, native OCR helper, provider diagnostics and packaging tool. Add validation and distribution paths rather than a second research engine. Minimal approach: documentation only; rejected because it cannot enforce evidence collection. Ideal approach: hosted evaluation and release services; deferred because local stdlib/platform tools provide the needed checks without another service.
**Design:** Existing research architecture; no new product decision required.

## Global constraints
- Preserve existing working-tree changes. No commit, push, release or paid call in this pass.
- Never turn synthetic reports or model judgments into independent human evidence.
- Native OCR remains local; cloud model diagnostics require explicit opt-in.
- Developer ID/notarization credentials are external requirements; fail clearly without them.
- Results must distinguish failed, skipped, empty and successful attempts.

## What already exists
- `evaluation/comparison/compare.py`: blinding, artifact hashes and independent worksheets; sample locations currently coordinated manually.
- `crates/engine/native/document-reader.swift`: actual local OCR and bounded rendering; a narrow synthetic fixture acceptance exists.
- `crates/engine/examples/provider_health.rs`: one bounded availability probe per adapter.
- `scripts/macos/app.py`: safe local ad-hoc app packaging and installation.

## Parallel lanes / file map
| Lane | Files / responsibility | Depends on |
|---|---|---|
| Comparison (parent) | `evaluation/comparison/compare.py`, tests and README: freeze sample descriptions before graders see reports; readiness checks | Existing coordinator |
| OCR | `scripts/qa/ocr*`, fixture truth/generator and OCR docs: measured local OCR validation | Existing native helper |
| Providers | `scripts/qa/provider*`, optional model diagnostic example, `docs/provider-health.md`: repeated probe matrix and explicit model smoke seam | Existing diagnostics / Llm |
| Distribution | `scripts/macos/app.py`, tests and `docs/installation.md`: optional Developer ID signing/notarization/stapling | Apple tools, external credentials for real submission |
| Integration (parent) | CI, README, development docs, changelog | All lanes |

## Review focus / failure modes
| Path | Failure | Proof and user result |
|---|---|---|
| Comparison | Different claims graded under identical IDs; changed packet | Frozen descriptions and hashes; reject mismatched samples |
| Readiness | Synthetic, small or incomplete study mistaken for proof | Explicit missing requirements; nonzero gate, no ranking |
| OCR | Digit/negative errors hidden by overall text score | Separate expected numeric/text checks and raw output, partial failures retained |
| Providers | All skipped or empty results called reliable research | Separate counters, minimum measured runs; never accuracy claims |
| Distribution | Signed archive differs from notarized app; credentials missing | Ordered sign/submit/staple/verify then archive; reject partial/unverified state |

## Tasks
- [x] Freeze report claim samples in blind packets and enforce them during grading; test mismatch/refusal and backwards-compatible synthetic selfcheck.
- [x] Add a real-study readiness gate with explicit limits and no fabricated competitive rating; test synthetic/undersized/unknown measurements.
- [x] Add and run actual OCR accuracy corpus/runner; retain known errors rather than tuning away failures.
- [x] Add repeated provider diagnostics and explicit model-provider smoke validation; offline tests and bounded public live proof.
- [x] Add optional Developer ID/notarization/stapling distribution; local tests and missing-credential refusal; no submission without credentials.
- [x] Integrate CI/docs, review combined diff and run focused/broad checks appropriate to changed surfaces.

## Not in scope / external completion
- Real comparative performance remains pending actual platform exports and independent human grades; none were found in the prior bounded folder search.
- Live paid cloud/provider quality validation needs configured credentials and usage authorization.
- Notarized release artifact requires Developer ID and Apple notarization access; implementing the path is not notarization proof.
- No new research modes, paid services, auto-publishing or version/tag changes.


## Receipt and rulings
- Comparison: 15 tests passed; synthetic CLI selfcheck passed. Actual CLI readiness returned exit 1 with synthetic/small/unfrozen-sample reasons, as required. No real human study exists.
- OCR: parent reran all five native cases (`.scratch/product-readiness-ocr/report.json`), both clean/rotation regression gates passed. Original helper failed those gates. Clean/rotation word recall 94.9%, numeric recall 80%; degraded results retained. Four metric tests passed.
- Providers: eight matrix tests and three model-diagnostic tests passed. Three live Crossref probes returned nonempty results (median 789 ms, p95 835 ms). No real cloud model or paid service calls.
- Distribution: six tests passed (two real local ad-hoc flows, four mocked credential-backed paths). New current-source app packaged and verified at `.scratch/installation/product-readiness`. No actual Developer ID signing or Apple submission.
- Combined: 129 Rust tests passed, five ignored; 33 Python tests passed; strict Clippy, workspace binaries/examples build, actual CLI OCR/visual/retry integration, actionlint, 34 documentation path checks, Gitleaks on 62 changed/new source files and diff whitespace checks passed.
- Independent read-only review found no blocking concrete defect in this pass. Prior changes were preserved; this pass adds no new product mode or dependency.
- Ruling: real studies now require a frozen claim registry. Synthetic legacy selfchecks remain compatible; archived studies without it fail readiness instead of acquiring retrospective proof.
- Ruling: a high-confidence English-language signal resolves the observed orientation defect. Vision fast confidence was uniform on the failing example, so confidence weighting was rejected by evidence. This remains English-focused heuristic OCR.
- Ruling: completeness thresholds are exploratory defaults (ten cases, two platforms, two planned replicates, five citation/fact samples per completed report), not statistical significance or a product quality score.

## External work still outstanding
- [ ] Collect real matched-budget exports and independent human grades; run readiness and review reference/sampling quality.
- [ ] Run the explicit model and configured-provider diagnostics with authorized live credentials; assess real research outputs separately.
- [ ] Produce a genuine Developer ID build, obtain Apple acceptance and test first launch/OCR/Keychain/Chrome on a Gatekeeper-enabled Mac.
