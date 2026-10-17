# Changelog

Changes follow [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and [Semantic Versioning](https://semver.org/spec/v2.0.0.html). Alpha releases may change APIs and storage behavior.

## [Unreleased]

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
