<div align="center">
  <img src="docs/assets/deephoney-banner.svg" alt="DeepHoney — deep research, traceable evidence" width="100%" />

  <p>A native macOS workspace for research you can inspect, question, and revisit.</p>

  <p>
    <a href="https://github.com/umararshad4/deephoney/releases/tag/v0.1.0-alpha.1"><img src="https://img.shields.io/badge/version-0.1.0--alpha.1-F6B73C?style=flat-square&amp;labelColor=171B21" alt="Version 0.1.0-alpha.1" /></a>
    <img src="https://img.shields.io/badge/macOS-15%2B-AAB4C2?style=flat-square&amp;labelColor=171B21" alt="macOS 15 or later" />
    <img src="https://img.shields.io/badge/Rust-stable-CE916E?style=flat-square&amp;labelColor=171B21" alt="Stable Rust" />
    <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-9DBF9E?style=flat-square&amp;labelColor=171B21" alt="MIT license" /></a>
    <a href="https://github.com/umararshad4/deephoney/actions/workflows/ci.yml"><img src="https://github.com/umararshad4/deephoney/actions/workflows/ci.yml/badge.svg" alt="macOS checks" /></a>
  </p>

  <p>
    <a href="#quickstart">Quickstart</a> ·
    <a href="#four-research-modes">Modes</a> ·
    <a href="docs/research.md">How it works</a> ·
    <a href="docs/configuration.md">Configuration</a> ·
    <a href="https://github.com/umararshad4/deephoney/releases">Releases</a>
  </p>
</div>

---

DeepHoney turns a question into an editable investigation, gathers quoted evidence across public sources, checks disagreements, and writes a cited report. It is built with Rust and GPUI, with a UI-free engine you can also run from the terminal.

> **Development preview · v0.1.0-alpha.1**
>
> The research loop, report writer, and optional built-in Tor are implemented. This release contains source code, with no signed installer. Live cloud-model and paid-provider validation remains incomplete.

## What you can do

- **Shape the investigation.** Review and edit perspective lenses, questions, source adapters, and queries before the searches begin.
- **Follow the evidence.** Watch searches, pages, identifier pivots, claims, and warnings in the live feed. Inspect each report's underlying ledger.
- **Reject unsupported numbers.** A literal support guard catches invented quantities and recognized sign/unit changes before the model verifier. Machine-transcribed findings retain visible cautions.
- **Check disagreement.** Group copied articles into one voice, record contradictions, and use Adversarial mode to seek counterevidence.
- **Look beyond ordinary search.** Query scholarly records, filings, court records, code, archives, public channel previews, and filtered onion results.
- **Keep your work.** Reopen saved reports, claims, sources, and historical passages. API keys stay in the macOS Keychain.

## Quickstart

You need **macOS 15+**, the **current stable Rust toolchain**, **Apple Command Line Tools**, and a configured model provider. Development checks used Rust 1.99.1; older toolchains have not been validated. GPUI compiles its Metal shaders at runtime, so the Xcode `metal` tool is not required.

If the Apple tools are missing, run `xcode-select --install` and complete the installer before building. Install Rust with [rustup](https://rustup.rs/).

```sh
git clone https://github.com/umararshad4/deephoney.git
cd deephoney
cargo run -p app --locked
```

1. Use **Set up model** on a fresh profile, or open **Settings**. Add an Anthropic API key or configure an OpenAI-compatible endpoint, including a local model server.
2. Add a search provider key or your SearXNG URL. Without either, DuckDuckGo HTML is a best-effort fallback.
3. Enter a topic, choose a mode, and click **Plan**.
4. Edit the questions or queries, then click **Run**.
5. Read **Report**, **Claims**, **Sources**, and **Deleted**. Completed investigations save to the sidebar. **Copy report** copies its Markdown, citations and cautions for reuse.

Depth limits the rounds. Breadth limits queries per lens. Settings also lets you set a page budget, opt into built-in Tor, enable isolated JavaScript reading, and configure a separate verifier. You can cancel an investigation while it runs.

For the exact preview version, check out `v0.1.0-alpha.1` before running Cargo. See [configuration](docs/configuration.md) for models, credentials, environment variables, and data locations.

For an app bundle and safe user-level installation, see [macOS installation](docs/installation.md).

## Four research modes

| Mode | What it adds |
|---|---|
| **Normal** | Public web, scholarly, filing, court, code, and community sources, with quoted claims and gap checks. |
| **Hidden data** | Wayback discovery, archive recovery, and historical text comparisons. |
| **Dark web** | Filtered Ahmia discovery and onion-page reads through built-in Arti. Requires Tor opt-in. |
| **Adversarial** | Reserved counterevidence searches and a further review of contested evidence. |

Dark web and Adversarial are separate modes. Availability depends on the source, your credentials, and rate limits. See the [source catalog and transport reference](docs/sources.md).

## From question to report

```mermaid
flowchart LR
    A[Your question] --> B[Editable plan]
    B --> C[Parallel search and reading]
    C --> D[Quoted evidence ledger]
    D --> E{Gaps or disagreements?}
    E -->|Within budget| C
    E -->|Ready to write| F[Claim-linked draft]
    F --> G[Separate support check]
    G --> H[Cited report and saved session]
```

The engine bounds concurrency, queries, rounds, and page attempts. It follows useful links and identifiers, prunes unproductive branches, and stops after two rounds without new claims or independent voices.

Report statements must map to recorded claims and pass a model check against their source-specific quotes. You can configure a different verifier model or provider; the default uses the writer model for a separate self-check. Citation numbers come from those mappings. Required sections include findings, contested evidence, coverage gaps, and sources. Historical passages retain their archive provenance.

**Evidence scores are heuristics, not truth guarantees.** Textual similarity estimates source independence. URL patterns estimate source quality. Models still judge support and contradictions. Read the [research and report reference](docs/research.md) for the exact rules and limits.

## Run research from the terminal

The engine works without GPUI. It reads environment variables rather than desktop Keychain settings.

```sh
# Configure ANTHROPIC_API_KEY in your environment first.
cargo run -p engine --example cli --locked -- \
  "How has battery recycling changed?" \
  --mode hidden --depth 3 --breadth 3 --pages 80 > report.md
```

Markdown goes to stdout. JSON progress events go to stderr. For OpenAI-compatible providers and the full flag list, see [headless configuration](docs/configuration.md#headless-research).

## Privacy and boundaries

- Only onion-page reads use Tor. Search requests, Ahmia discovery, and configured model APIs use direct connections.
- A remote model receives research evidence, including evidence read through Tor. This is not a tool for whole-session anonymity.
- Keys use the macOS Keychain. Settings and session evidence are stored locally; the session database is plaintext.
- Readers accept HTML, plain text, and text-based PDFs. They inspect all retained text in overlapping sections. PDF reading stops at 12 MiB, 1,000 pages, or 180,000 extracted characters, with warnings for truncated content.
- JavaScript reading is optional and requires installed Chrome/Chromium. It uses a fresh profile for clearnet pages, blocks forms and non-GET requests, and has request/time/DOM bounds. It does not bypass challenges.
- Scanned PDF OCR runs locally on macOS, with an English-focused fast reader and explicit limits. Optional table/chart interpretation sends page images to your configured model and preserves OCR references; interpretations still need verification.
- Authenticated browsing, arbitrary media extraction, and browser rendering over Tor are not included.
- Common Crawl discovers URLs to current pages. It does not replay archived WARC content. Wayback provides archive captures.

## Documentation

| Guide | Contents |
|---|---|
| [Configuration](docs/configuration.md) | Providers, keys, storage, budgets, Tor, and CLI variables |
| [macOS installation](docs/installation.md) | Build, verify, and install a local app bundle |
| [Provider diagnostics](docs/provider-health.md) | Explicit live probes, failures, and retry metrics |
| [Blind comparisons](evaluation/comparison/README.md) | Randomized report packets and validated independent grading |
| [Evaluation workflow](evaluation/README.md) | Repeatable cases, machine-readable results, and blind human comparison |
| [Research and reports](docs/research.md) | Planning, pivots, stopping rules, statement verification, citations, and scoring |
| [Sources and transport](docs/sources.md) | Twenty registered adapters, credentials, archive semantics, and onion routing |
| [Development](docs/development.md) | Engine interface, code map, checks, and validation boundaries |
| [Contributing](CONTRIBUTING.md) | Local workflow and useful bug reports |
| [Changelog](CHANGELOG.md) | Versioned changes and release policy |

## Development status

Local workspace validation passed **139 Rust tests**, strict Clippy, and a desktop build. Thirty-four Python comparison, packaging and diagnostic tests and two additional tests with installed Chrome passed, covering JavaScript rendering, request restrictions, cancellation, and profile cleanup. The packaged app completed a scanned-PDF plan/run and displayed OCR quotations with page locations. An end-to-end CLI fixture exercised real OCR, optional image transport with a synthetic model, and temporary retrieval recovery. Keychain persistence and a real onion fetch through embedded Arti were validated during the initial alpha pass.

The current development tools freeze blind grading samples, enforce descriptive-study completeness, measure OCR word/numeric preservation, repeat provider probes and support optional Developer ID signing/notarization. The OCR benchmark found and fixed upside-down orientation selection; small/degraded scans remain weak. Remaining checks include actual independent human comparisons, a live cloud-model investigation, paid or token-gated providers, broader UI testing, and credential-backed macOS signing and notarization. Some public endpoints returned bot challenges, rate limits, or timeouts. See [validation details](docs/development.md#validation) for the distinction between local fixtures and live probes.

## License

[MIT](LICENSE) · Copyright 2026 Umar Arshad.
