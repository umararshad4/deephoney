# Research and report reference

[Back to DeepHoney](../README.md)

## Research loop

`DeepResearch` is installed in the desktop app. It performs these bounded stages:

1. The strong model proposes up to eight perspective lenses. Each has a question, up to two enabled adapters, and a bounded set of queries. Unsupported adapters fall back to an enabled surface provider. The preview performs no searches or page reads.
2. Searches run with at most four concurrent requests. URLs and adapter/query pairs are deduplicated. Search snippets remain discovery hints and never become evidence.
3. Up to four page reads and extractions run concurrently. The loop ranks leads by relevance, identifier specificity, and novelty. It can follow up to eight links from a result page, preferring primary records, without recursively crawling those links.
4. The reader visits every retained 12,000-character section with 250-character overlap. It extracts up to eight claims per section and retains up to 64 per document, selected round-robin across sections. Omitted claims produce a warning. Each accepted claim requires a quote of 16–2,000 characters present in the fetched text after whitespace normalization. Claims longer than 1,000 characters are rejected. The quote-existence check does not establish that the quote entails the claim.
5. Between rounds, the planner checks coverage and evidence gaps. Depth is 1–5 rounds. Breadth is 1–6 queries per lens in the first round and halves each round, with a minimum of one. Unproductive lenses are pruned. The loop stops on completion, no new leads, exhausted budget, or two rounds without new claims or independent voices.
6. Each run allows up to sixteen identifier searches. Hidden, Dark, and Adversarial modes also allow at most four archive checks. Pivots include DOI, arXiv, CVE, docket, hash, email, username, CIK, ticker, domain, exact quotation, wallet address, and onion identifiers. Specialized adapters receive suitable identifiers. Identifiers from onion-page text are not sent to direct search APIs.
7. Adversarial mode reserves up to one quarter of the page budget, capped at eight attempts, for up to eight counterevidence searches. Ordinary queued leads are discarded before that pass. A further evidence debate runs when contradictions exist.
8. The configured verifier reviews every claim in batches of at most 80 ranked claims. Cross-batch contradiction detection is not exhaustive. Gap planning and the optional final adversarial debate use the strongest 80 claims. Indices are validated. A supported verdict needs at least two independent voices and no recorded contradiction. A refuted verdict needs cited contradictory evidence. Uncertain claims remain unresolved or unreviewed.
9. The report writer drafts claim-linked statements, checks them in a separate model pass, and renders the report. The desktop saves the returned report and ledger to SQLite.

The ledger has one mutation owner. Workers return results instead of editing shared evidence. Each claim retains source-specific quotes and locations, including when saved and reopened. Legacy sessions with one unmapped quote attribute it only to their first source, labeled as a legacy mapping. Claims merge only when their wording matches after case, whitespace, and trailing sentence-punctuation normalization. Numeric differences and negation remain separate. Corroborating paraphrases can therefore remain separate claims.

Search, read, extraction, gap, and review failures produce warnings where possible. A failed gap or review pass preserves collected evidence. Initial planning failures, missing grounded claims, and failed report verification return errors. Dropping the research future or closing its progress receiver cancels in-flight work.

## Report writer

```rust
let markdown = engine::report::write(&llm, &topic, &questions, &ledger, &ghosts).await?;
```

`write()` derives the configured checker through `Llm::verifier()`, defaulting to the writer-model self-check. `write_with_verifier(&writer, &verifier, ...)` accepts a separately configured checker. `brief()` produces one packet of up to 80 ranked claims; the main writer processes all packets with stable global claim IDs and source-specific quotes and locations, source numbers, independent voices, heuristic confidence, primary-source flags, provenance dates, contradictions, and verdicts. The packet also includes the research questions and removed passages.

The writer returns structured statements linked to zero-based claim IDs. Structural validation rejects missing evidence, invalid IDs, excessive statements, and oversized text. A separate model call checks each remaining statement against its mapped quotes, including dates, quantities, scope, and contradictions. Rejected statements are omitted. A report with no accepted finding returns an error.

The renderer supplies these sections deterministically:

- Summary
- Key findings
- Contested or unresolved
- Removed or changed content, when archived passages exist
- Gaps and open questions, including lenses with no evidence
- Sources
- Run details, including verifier configuration, elapsed time, attempts, and provider-reported token totals

The report retains at most 256 accepted statements, distributed across batches. It discloses any omissions. Every generated factual statement maps to claims and their quoted sources. Evidence details include each supporting excerpt and its document location, claim IDs, independent voices, primary-source presence, provenance, contradictions, and heuristic confidence. Archived passages appear as direct quotations with capture dates and snapshot URLs; their absence from current text does not establish why they changed.

Citation numbers are `source index + 1`, matching the Sources view. The renderer derives citations from claim mappings and lists only cited sources. Model text is escaped so it cannot add its own links, headings, or citation labels. The public legacy `finish()` helper still cleans numeric citation forms such as `[S2]`, drops invalid source numbers, and appends a source list; the main writer uses structured mappings instead.

These checks reduce unsupported statements but still depend on model judgment. They do not guarantee real-world truth or the absence of bias.

### Evidence scores

- Near-duplicate pages with SimHash distance at most 10 bits share a voice cluster. Copied articles count as one voice.
- Confidence is `1 - product(1 - 0.55 * quality)` over clusters, using the best quality in each cluster. A contested claim is multiplied by 0.7, a refuted claim by 0.25, and a supported claim has a floor of 0.7.
- URL-based quality priors are 1.0 for government, court, DOI, and arXiv sources; 0.9 for academic sources; 0.65 for general press; 0.5 for forums; and 0.35 for listicle or affiliate patterns.

Independence, quality, and primary-source labels are heuristics. Confidence is not a calibrated probability. A first-seen date is the earliest backing publication or archive date, not proof of the claim's origin.

## Document readers

HTML and plain-text bodies are limited to 2 MiB. Text-based PDF bodies are limited to 12 MiB. Extracted text stops at 180,000 Unicode characters; PDFs also stop after 1,000 pages. Truncation produces a warning. Parser-generated PDF page spans and section character ranges accompany supporting quotes. Page-like markers in source text cannot create page metadata. Encrypted, empty, and malformed PDFs return named errors. On macOS, missing text triggers bounded local OCR, including scanned pages in mixed documents. OCR uses Apple Vision's English-focused fast recognition path; it may misread numbers or miss other languages. Page locations, normalized line boxes and recognition confidence accompany the transcription. Recognition confidence is not factual confidence. Evidence locations retain a machine-transcription notice in saved claims and reports, so this provenance survives after the live warnings disappear.

PDF parsing runs in a disposable process with a 20-second wall timeout, 15-second CPU limit, and bounded output. Linux also enforces a 2 GiB address-space limit. macOS uses a parent memory monitor because finite address-space limits are unavailable there. Its 512 MiB memory threshold, sampled every 50 ms, can briefly be exceeded before the monitor terminates the worker. This is not a hard allocation limit.

Local OCR covers at most the first 12 pages, renders a maximum 1,800-pixel page dimension, and has a 60-second wall limit, 45-second CPU limit and sampled 512 MiB memory threshold. Later scanned pages produce warnings; existing selectable text remains available beyond that limit. Unreadable scans return a named error.

Optional visual PDF interpretation processes bounded page images with the configured planner/writer model. The complete OCR/visual pass has a 120-second budget, with at most 45 seconds for each model call. Each PNG is at most 2 MiB; oversized images preserve OCR and a warning. Returned tables must be rectangular and every nonempty cell must cite an exact OCR substring. Chart points require directly printed labels and values; the reader does not interpolate values from axes. Numeric checks reject dropped signs, accounting parentheses, percentage markers and digit truncation. Row/column and label/value associations remain model interpretations that need confirmation against the image or independent text. Failed or rejected interpretations retain readable text and warnings.

Optional JavaScript reading runs only when ordinary clearnet HTML looks like an unusable script shell. It uses installed Chrome/Chromium in a fresh temporary profile. It never attaches to your browser. Request interception rejects non-GET requests, onion URLs, downloads, images/media/fonts, and binary responses. An added content policy blocks forms, frames, and workers without weakening the site's policy. Limits are 100 requests, 30 seconds, and 2 MiB of returned HTML. Missing Chrome or failed rendering produces a warning. Challenge pages are not bypassed. Dark mode disables this fallback for all pages.

## Usage and evaluation

`Report.metrics` records search adapter calls, page/archive attempts, model HTTP attempts including retries, elapsed time, verifier model, and provider-reported input/output tokens. Missing token usage is unknown. Totals may be partial when a provider omits usage; no prices are inferred. A separate verifier configuration reduces dependence on one configuration but does not establish model independence or truth.

Use the [evaluation workflow](../evaluation/README.md) to save repeatable runs and compare exports. Literal expected-fact checks and citation structure are separate from human assessment of factual accuracy and citation entailment.

## Retrieval recovery

Clearnet page and search reads share a maximum of three client attempts across headers and interrupted bodies, with a 75-second deadline per logical read. Temporary timeouts, connection failures and HTTP 408/429/500/502/503/504/529 can retry. Generic requests replay only GET/HEAD; search adapters explicitly opt their read-only POST queries into recovery. Partial response bodies are discarded before retry. Body limits, unsupported types, parser failures, permanent HTTP errors and challenges are not retried as transient failures.

`Retry-After` accepts seconds and HTTP dates. A requested wait above 30 seconds stops the read with an actionable error rather than retrying earlier than requested. Cancellation drops the current request or wait. Metrics distinguish client attempts, retries and recovered reads; automatic redirect hops are not separate attempts in these counters. The [provider diagnostics](provider-health.md) preserve actual failures and configuration skips.


## Numeric support and degraded-source safeguards

Before the report verifier runs, a deterministic preflight rejects digit quantities absent from the mapped source quotations, changed negative signs and recognized explicit unit/currency/percentage changes. It normalizes comma grouping, trailing decimal zeros, leading decimals, Unicode minus, common accounting negatives and numeric/date separators. Numbers derived by arithmetic or unit conversion that do not appear in the quotes are conservatively omitted; this renderer does not establish those calculations. The semantic verifier still checks attribution, entity/value association, negation, scope and meaning. Locale-specific numbers, unusual units and numbers written in words are not fully covered by the literal guard.

Findings using machine-transcribed PDF evidence carry an explicit warning before their confidence badge. The original page must be checked for OCR errors and table/chart associations; passing a text support check cannot establish transcription accuracy.

Searches remain concurrent (four logical searches at once). After three consecutive completed failures from an adapter, the remaining undispatched queries for that adapter are skipped for the current investigation. Up to three previously dispatched requests may still finish. A successful response, including an empty result, resets failures before the pause threshold. Healthy adapters continue. Both the live warning and saved Run details identify the paused adapters and reduced coverage. A new investigation retries adapters automatically; there is no same-run probe that spends more requests on an apparent outage. Search metrics count actual dispatched calls, not skipped queries.
