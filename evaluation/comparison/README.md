# Blind research comparisons

`compare.py` is a Python 3.10+ standard-library coordinator tool. It randomizes report IDs, separates platform/run measurements from grader packets, validates independent human worksheets and summarizes observed results. It never runs a model, buys a report, invents competitor evidence ledgers or treats a language model as a human grader.

The executable synthetic experiment verifies instrumentation only:

```sh
python3 -m unittest discover -s evaluation/comparison -p 'test_*.py' -v
python3 evaluation/comparison/selfcheck.py --out .scratch/my-comparison-selfcheck
```

The fresh output contains a four-attempt study (two artificial platforms, each one completed report and one failure), two explicitly **nonhuman synthetic** worksheets, an intentional citation disagreement, `summary.json` and a subprocess `receipt.json`. Zero observed cost remains zero; missing cost/time remain unknown. The names are fictitious test fixtures, not competing products. No factual-accuracy or competitive claim follows from these results.

## Coordinator protocol

1. Before collecting reports, register the same questions, case IDs, replicate counts, reference evidence, evaluation date, time/spend limits and sampling policy for each platform. Freeze these in private study notes. Use at least ten diverse cases for exploratory comparisons, and avoid statistical superiority claims from a small convenience sample. Counterevidence and contested conclusions belong in the reference set, with explicit attribution and uncertainty.
2. Collect actual exports and original prompts/settings separately. Record **every planned attempt**, including timeouts and failures. Record platform/model/version, timestamps, budgets and billing limitations in the private `measurement_notes`. Measure wall time consistently (including retries); record observed charges with currency only if actually available. A subscription price does not establish per-run cost. Never infer charges from tokens or make an unknown measurement zero.
3. Preserve original exports outside the packet. Make reviewed sanitized Markdown/PDF copies. Remove platform names, logo/cover pages, author/producer metadata, tracking URLs, identifying footers, comments, attachments and embedded scripts while preserving substantive content and citations. Verify a PDF visually and inspect its metadata using your normal PDF editor. Record transformations privately. Do not simply rename an unreviewed PDF. `sanitized_reviewed: true` is the coordinator's explicit attestation, not an automated guarantee. Markdown containing its platform name is rejected as an obvious identity leak; other metadata and writing style can still reveal identity. Abort or label compromised blinding if a grader recognizes a platform.
4. Write a manifest like the one below in the same private directory as reviewed report artifacts. `artifact` accepts relative `.md`/`.pdf` paths only, at most 20 MiB; traversal and symlinks are refused. All platforms must share planned case/replicate attempts. Failed attempts have `artifact: null`, `sanitized_reviewed: false`, and a nonempty `failure_reason`. A completed attempt requires the reverse and `failure_reason: null`.
5. Freeze the claim samples in `samples.json` before giving any material to graders. Real studies require `--samples`; the format is below. Prepare a **fresh** output directory. Keep the coordinator key outside it and never share the key, raw manifest, measurements or final unblinded summary with graders before they lock their independent worksheets.

```sh
python3 evaluation/comparison/compare.py prepare private-study/manifest.json \
  --out private-study/blind-packet --key private-study/coordinator-key.json \
  --samples private-study/samples.json
```

The packet contains only questions, attributed reference evidence, random artifact filenames, completed/failed status, hashes and a worksheet template. The key (mode 0600) retains identities, failures and measurements. Files are created exclusively, not overwritten. Copy only `blind-packet` to graders. Failed attempts stay visible as attempted cases but receive no quality grades.

Example manifest (add other platform attempts with identical case/replicate slots):

```json
{
  "study_id": "pilot-2026-10",
  "synthetic": false,
  "reference": {
    "title": "Human-curated reference packet",
    "provenance": "Coordinator name, evaluation date, selection method, source retrieval dates and limitations",
    "evidence": [{
      "id": "safe-definition",
      "case_id": "http-methods",
      "statement": "Safe semantics concern the requested read-only effect, not all incidental side effects.",
      "source": "https://www.rfc-editor.org/rfc/rfc9110.html#section-9.2.1",
      "attribution": "Fielding, Nottingham and Reschke, RFC 9110, section 9.2.1"
    }]
  },
  "cases": [{"id": "http-methods", "question": "Explain safe HTTP methods and incidental side effects."}],
  "runs": [{
    "platform": "Actual platform and model version",
    "case_id": "http-methods",
    "replicate": 1,
    "status": "completed",
    "artifact": "reviewed-export.md",
    "sanitized_reviewed": true,
    "elapsed_seconds": 127.5,
    "observed_cost": null,
    "currency": null,
    "measurement_notes": "Measured start/finish; exact prompt/settings/budgets recorded privately. Per-run charge unavailable.",
    "failure_reason": null
  }]
}
```

DeepHoney exports can supply Markdown without inventing a competitor ledger:

```sh
python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["markdown"])' \
  private-study/deephoney.json > private-study/reviewed-export.md
```

Review and sanitize that Markdown before preparing the packet. Engine historical zero-default metrics are not measurements; use `null` unless timing/cost was genuinely observed. The tool does not independently authenticate measurement provenance.

## Independent graders

Give at least two human graders a separate copy of `grade-template.json`. Each must set a distinct pseudonymous `grader` ID and `independent_human: true`. For a real study `synthetic` must be false. A synthetic study can use explicitly nonhuman worksheets; never relabel those as human results.

Before graders work, a coordinator freezes the citation/fact sample IDs, exact statements and neutral locations in the packet using `--samples` (for example `paragraph-3-sentence-2`). Include the main conclusion and high-stakes claims. Graders must not select different claims under the same ID. Use the same sample size/policy across platforms and preserve the full sample descriptions privately. Graders work independently on that fixed sample, then lock files before discussing disagreements. Do not change judgments just to make agreement higher.

The private sample registry identifies each completed attempt; the public packet replaces identities with random blind IDs. Include every completed report exactly once and use the same number of citation/fact samples for each report. Failed attempts have no quality samples. Every sample needs an ID, a neutral location and the exact claim to assess:

```json
[{
  "platform": "Actual platform and model version",
  "case_id": "http-methods",
  "replicate": 1,
  "citation_checks": [{"id": "p3-s2", "location": "paragraph 3, sentence 2", "statement": "Exact report claim to check."}],
  "fact_checks": [{"id": "p3-s2", "location": "paragraph 3, sentence 2", "statement": "Exact report claim to check."}]
}]
```

Use the main conclusion and consequential claims, with the same sampling policy across platforms. The frozen descriptions are hashed with the packet; grader IDs must exactly match them. Generated worksheets intentionally contain incomplete reasons and empty source lists until independently filled. The tool cannot determine whether a coordinator chose representative claims or whether graders read the frozen descriptions.

Every completed report requires three nonempty check arrays:

```json
{
  "blind_id": "COPY-FROM-PACKET",
  "citation_checks": [{
    "id": "paragraph-3-sentence-2",
    "judgment": "partial",
    "reason": "The cited source supports the date but not the causal explanation.",
    "source_urls": ["https://source.example/document#section"]
  }],
  "fact_checks": [{
    "id": "paragraph-3-sentence-2",
    "judgment": "unresolved",
    "reason": "The independent reference does not establish the proposed cause.",
    "source_urls": ["https://source.example/document#section"]
  }],
  "reference_checks": [{
    "id": "safe-definition",
    "judgment": "missed",
    "reason": "The report omits the distinction between intended and incidental effects.",
    "source_urls": ["https://www.rfc-editor.org/rfc/rfc9110.html#section-9.2.1"]
  }]
}
```

Allowed judgments: citations `supported/partial/unsupported`; facts `correct/error/unresolved`; reference items `covered/missed/unresolved`. Supply a reason and source attribution for every judgment. Reference checks must cover **every reference ID for that case exactly once**. An unresolvable citation is `unsupported` with the attempted URL and failure explanation; uncertainty about truth is `unresolved`, not an invented factual error. Missing citation support and factual falsity are different questions. Missing references bound coverage only against this packet, not all potentially relevant evidence.

Blank worksheets, unfinished placeholder reasons, missing/duplicate reports, failed-run quality grades, duplicate grader IDs, invalid judgments, duplicate checks/JSON keys, inconsistent sample IDs, changed packets/artifacts and nonfinite/negative measurements are refused. The tool validates records and attestations; it cannot verify the identity or actual independence of a grader. Handle post-lock adjudication separately, retaining original independent files.

## Summarize after grades are locked

```sh
python3 evaluation/comparison/compare.py summarize private-study/blind-packet/packet.json \
  --key private-study/coordinator-key.json \
  --grades private-study/grader-a.json private-study/grader-b.json \
  --out private-study/summary.json
```

The summary unmasks platform identities. It retains individual judgments as counts per grader/report and gives per-platform sampled citation support, factual-error and missed-reference rates **separately per grader**. Partial support and unresolved facts remain separate counts; no majority-vote truth or opaque composite score is calculated. Agreement is the fraction of the declared same sample IDs with unanimous judgments, with numerator and denominator shown. This is raw agreement, not chance-corrected reliability.

Completion uses every planned attempt, including failures. Timing summaries include all attempts with known durations, even failed ones, and show unknown counts. Observed money is summed separately by currency, never converted or treated as complete when any run is unknown. A known zero charge remains zero. Comparison fairness still requires matched budgets and consistent start/stop definitions; the tool cannot infer them from exports.

`reference.json` is a small, attributed RFC 9110 comprehension reference, checked against the RFC Editor's primary text on 2026-10-18. It is deliberately narrow and is not a gold-standard broad research suite. Review it independently before any real study. Actual competitor exports and independent humans are still required to establish real research quality.

Keep raw exports, keys, worksheets and summaries private: they can contain sensitive subjects and billing information. Run the coordinator tool in a trusted local directory, without concurrent file mutation; path checks do not provide a sandbox against a hostile process racing filesystem operations.

## Descriptive-study readiness gate

After independent worksheets are complete, run:

```sh
python3 evaluation/comparison/compare.py readiness private-study/blind-packet/packet.json \
  --key private-study/coordinator-key.json \
  --grades private-study/grader-a.json private-study/grader-b.json \
  --out private-study/readiness.json
```

This revalidates the packet, artifacts and worksheets, writes the missing requirements, and exits 1 for an incomplete study. A study needs real (non-synthetic) data, frozen samples, at least ten distinct cases, two platforms, two planned replicates per platform/case (failures included), and at least five citation and fact samples per completed report. Every platform needs at least one completed report for quality comparison. These are exploratory completeness thresholds, not statistical significance, independent authentication or proof of superiority. Failed attempts remain in completion denominators.

Timing comparison is separately marked ready only when every attempt has an observed duration. Cost comparison also requires every charge to be observed in one currency. Unknown charges and mixed currencies remain visible without invented conversions. A complete descriptive study can still have incomplete timing/cost comparisons. Actual exports, reference review and independent humans remain external requirements.
