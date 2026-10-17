# Research and report reference

[Back to DeepHoney](../README.md)

## Research loop

`DeepResearch` is installed in the desktop app. It performs these bounded stages:

1. The strong model proposes up to eight perspective lenses. Each has a question, up to two enabled adapters, and a bounded set of queries. Unsupported adapters fall back to an enabled surface provider. The preview performs no searches or page reads.
2. Searches run with at most four concurrent requests. URLs and adapter/query pairs are deduplicated. Search snippets remain discovery hints and never become evidence.
3. Up to four page reads and extractions run concurrently. The loop ranks leads by relevance, identifier specificity, and novelty. It can follow up to eight links from a result page, preferring primary records, without recursively crawling those links.
4. The reader extracts at most eight claims per page. Each accepted claim requires a quote of 16–2,000 characters present in the fetched text after whitespace normalization. Claims longer than 1,000 characters are rejected. The quote-existence check does not establish that the quote entails the claim.
5. Between rounds, the planner checks coverage and evidence gaps. Depth is 1–5 rounds. Breadth is 1–6 queries per lens in the first round and halves each round, with a minimum of one. Unproductive lenses are pruned. The loop stops on completion, no new leads, exhausted budget, or two rounds without new claims or independent voices.
6. Each run allows up to sixteen identifier searches. Hidden, Dark, and Adversarial modes also allow at most four archive checks. Pivots include DOI, arXiv, CVE, docket, hash, email, username, CIK, ticker, domain, exact quotation, wallet address, and onion identifiers. Specialized adapters receive suitable identifiers. Identifiers from onion-page text are not sent to direct search APIs.
7. Adversarial mode reserves up to one quarter of the page budget, capped at eight attempts, for up to eight counterevidence searches. Ordinary queued leads are discarded before that pass. A further evidence debate runs when contradictions exist.
8. The strong model reviews up to 80 ranked claims. Indices are validated. A supported verdict needs at least two independent voices and no recorded contradiction. A refuted verdict needs cited contradictory evidence. Uncertain claims remain unresolved or unreviewed.
9. The report writer drafts claim-linked statements, checks them in a separate model pass, and renders the report. The desktop saves the returned report and ledger to SQLite.

The ledger has one mutation owner. Workers return results instead of editing shared evidence. Claims merge only when their wording matches after case, whitespace, and trailing sentence-punctuation normalization. Numeric differences and negation remain separate. Corroborating paraphrases can therefore remain separate claims.

Search, read, extraction, gap, and review failures produce warnings where possible. A failed gap or review pass preserves collected evidence. Initial planning failures, missing grounded claims, and failed report verification return errors. Dropping the research future or closing its progress receiver cancels in-flight work.

## Report writer

```rust
let markdown = engine::report::write(&llm, &topic, &questions, &ledger, &ghosts).await?;
```

`brief()` supplies up to 80 ranked claims with their stored quotes, source numbers, independent voices, heuristic confidence, primary-source flags, provenance dates, contradictions, and verdicts. The packet also includes the research questions and removed passages.

The writer returns structured statements linked to zero-based claim IDs. Structural validation rejects missing evidence, invalid IDs, excessive statements, and oversized text. A separate model call checks each remaining statement against its mapped quotes, including dates, quantities, scope, and contradictions. Rejected statements are omitted. A report with no accepted finding returns an error.

The renderer supplies these sections deterministically:

- Summary
- Key findings
- Contested or unresolved
- Removed or changed content, when archived passages exist
- Gaps and open questions, including lenses with no evidence
- Sources

Every generated factual statement maps to claims and their sources. Evidence details show claim IDs, independent voices, primary-source presence, provenance, contradictions, and heuristic confidence. Archived passages appear as direct quotations with capture dates and snapshot URLs; their absence from current text does not establish why they changed.

Citation numbers are `source index + 1`, matching the Sources view. The renderer derives citations from claim mappings and lists only cited sources. Model text is escaped so it cannot add its own links, headings, or citation labels. The public legacy `finish()` helper still cleans numeric citation forms such as `[S2]`, drops invalid source numbers, and appends a source list; the main writer uses structured mappings instead.

These checks reduce unsupported statements but still depend on model judgment. They do not guarantee real-world truth or the absence of bias.

### Evidence scores

- Near-duplicate pages with SimHash distance at most 10 bits share a voice cluster. Copied articles count as one voice.
- Confidence is `1 - product(1 - 0.55 * quality)` over clusters, using the best quality in each cluster. A contested claim is multiplied by 0.7, a refuted claim by 0.25, and a supported claim has a floor of 0.7.
- URL-based quality priors are 1.0 for government, court, DOI, and arXiv sources; 0.9 for academic sources; 0.65 for general press; 0.5 for forums; and 0.35 for listicle or affiliate patterns.

Independence, quality, and primary-source labels are heuristics. Confidence is not a calibrated probability. A first-seen date is the earliest backing publication or archive date, not proof of the claim's origin.
