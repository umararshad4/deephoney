# Changelog

Changes follow [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and [Semantic Versioning](https://semver.org/spec/v2.0.0.html). Alpha releases may change APIs and storage behavior.

## [Unreleased]

- Reject unquoted digit quantities and recognized sign/unit changes before report model verification; retain explicit machine-transcription cautions.
- Pause repeatedly failing adapters within an investigation while keeping healthy searches concurrent and disclosing reduced coverage in saved reports.
- Add actionable first-run model setup and one-click report Markdown copying.
- Correct OCR benchmark page geometry and add literal-punctuation negative coverage; native OCR accuracy is not claimed to improve.


- Freeze exact citation/fact samples before blind grading and gate incomplete descriptive studies.
- Measure native OCR word/numeric preservation across clean, rotated and degraded synthetic pages; fix upside-down orientation selection.
- Repeat provider availability probes with explicit pass thresholds and add opt-in model transport/JSON diagnostics.
- Support optional Developer ID hardened-runtime signing, Apple notarization and stapling with verified-install enforcement. Real Apple acceptance remains unvalidated without credentials.


### Added

- Local macOS scanned-PDF OCR with page/line geometry and bounded optional table/chart interpretation.
- Blind comparison packets, validated independent grading, and summaries retaining failed attempts and unknown costs.
- Explicit per-provider live diagnostics and retrieval recovery metrics.
- Versioned, ad-hoc signed macOS app packaging and verified user-level installation without overwriting existing apps.

- Text-based PDF reading with page locations and isolated parsing.
- Optional isolated Chrome/Chromium reading for clearnet JavaScript shells.
- Separate verifier provider, model, endpoint, and Keychain credential settings.
- Provider-reported token totals, request counters, elapsed time, and JSON report export.
- Repeatable evaluation cases, batch artifacts, structural checks, and a blind human grading rubric.

### Changed

- Transient clearnet retrieval failures, including interrupted bodies, recover within one bounded attempt budget.

- Readers inspect every retained document section instead of only its beginning, with explicit retention limits and warnings.
- Merged claims preserve source-specific quotes and locations in saved sessions and report evidence.
- Evidence review and report writing process all claims in bounded batches with stable IDs and disclosed output limits.

## [0.1.0-alpha.1] - 2026-10-17

### Added

- Native macOS GPUI application with editable research plans, a live progress feed, report views, and saved sessions.
- Bounded parallel research, ranked links and identifier pivots, gap checks, and saturation stops.
- Normal, Hidden data, Dark web, and Adversarial modes across twenty registered source adapters.
- Quoted claims, source similarity clustering, contradiction review, and heuristic confidence scores.
- Claim-linked report drafting, a separate support check, deterministic sections, and derived source citations.
- Wayback recovery and historical text comparisons, plus explicit Common Crawl index discovery.
- Optional embedded Arti transport for Ahmia-discovered onion pages, with text and body limits.
- macOS Keychain credentials, configurable providers and budgets, and an isolated data-directory option.
- Headless CLI, local HTTP fixtures, source documentation, and macOS CI.

### Known limitations

- Source release only. No signed or notarized macOS installer.
- Full live cloud-model and paid or token-gated provider validation remains incomplete.
- Some public endpoints can return rate limits, bot challenges, or timeouts.
- Evidence support and source scores still depend on model judgment and heuristics.
- Tor applies only to onion-page reads. Local session evidence is plaintext.

[Unreleased]: https://github.com/umararshad4/deephoney/compare/v0.1.0-alpha.1...HEAD
[0.1.0-alpha.1]: https://github.com/umararshad4/deephoney/releases/tag/v0.1.0-alpha.1
