# Synthetic visual-document fixtures

These are generated test data, not real research evidence or quality benchmarks. No external content is used.

- `scanned-table-chart.pdf`: one raster-only page, table Year/Revenue with 2024/125 and 2025/175; a directly labeled bar chart uses the same values and USD millions.
- `scanned-rotated.pdf`: the same page with 90-degree PDF rotation metadata.
- `mixed-text-scan.pdf`: selectable prose on page one and the raster table/chart on page two.

Regenerate on macOS with Xcode Command Line Tools:

```sh
swift crates/engine/tests/fixtures/generate-visual.swift crates/engine/tests/fixtures/scanned-table-chart.pdf
```

Native acceptance tests exercise actual CoreGraphics/Vision OCR and check text, values, page metadata, rotation, and uncertainty. The separate vision-model test uses a local synthetic OpenAI-compatible response to verify image transport, OCR-grounded table/chart structure, and usage accounting. It does not measure a commercial model's visual accuracy or confirm real chart associations.

## Reproducible local OCR benchmark

On macOS 15+ with Xcode Command Line Tools, run from the repository root:

```sh
python3 -m unittest discover -s scripts/qa -p 'test_ocr_benchmark.py' -v
python3 scripts/qa/ocr_benchmark.py --out .scratch/ocr-benchmark-new
```

Use a new output directory each run. The script compiles the **actual native helper source**, generates six raster-only PDFs from `ocr-benchmark.json` with `generate-ocr-benchmark.swift`, then performs actual Vision OCR without models, accounts or network access. It records native/generator/truth/PDF SHA-256 hashes, system version, timings, original OCR text, errors, warnings and per-case metrics in `report.json`. This validates the native recognition seam; it does not exercise Rust retrieval or report generation (use `scripts/qa/document_flow.py` for that).

Metrics are case-insensitive, whitespace-tokenized word recall/precision, word Levenshtein error rate (including reading-order errors), and exact numeric multiset recall/precision. Numeric comparison preserves signs, currency, thousands separators, decimal places, percentages and accounting parentheses. Missing and unexpected items remain visible. Word recall ignores order; word error rate checks order. Neither measure confirms table cell associations or chart interpretation.

Measured on 2026-10-18, macOS 27.0.1 arm64, after correcting generator page geometry:

| Synthetic page | Word recall | Word error rate | Exact numeric recall |
|---|---:|---:|---:|
| Clean, 28-point | 94.9% | 5.1% | 80.0% |
| Same page, 90-degree PDF rotation | 94.9% | 5.1% | 80.0% |
| Raster resolution reduced to 40% | 38.5% | 61.5% | 60.0% |
| Gray text, 25% contrast | 94.9% | 5.1% | 90.0% |
| Small print, 14-point | 51.3% | 48.7% | 70.0% |
| Literal punctuation `0/. 12.50/. -3.50/.` | 70.0% | 30.0% | 0.0% |

The benchmark exposed an orientation defect: maximizing character count selected upside-down gibberish (281 characters) over readable text (280). The corrected helper gives a bonus only to strongly recognized English text; ambiguous language guesses retain the character-count baseline. Clean and rotated word recall improved from 0% to 94.9%; numeric recall improved from 0% to 80%. Confidences from this Vision fast path were uniformly 0.5 in the diagnostic, so confidence weighting alone did not distinguish orientations.

Exit 0 means all six cases completed and the clean/rotation regression gates passed (word recall at least 90%, exact numeric recall at least 80%). It does **not** mean every case is accurate: degraded and small text remain weak, and even clean percentage symbols were misread. The literal-punctuation case also fails if OCR returns a percent sign: `0/.` must never be silently treated as `%`. This case currently corrupts slash characters to `1`; that error remains visible in its metrics and is not repaired. These modest gates catch reproduced failures, not general OCR quality. A case failure is recorded and returns nonzero; compilation or fixture-generation failure also returns nonzero.

shortcut: six synthetic English pages, one font, one layout and one local OS are not a representative document corpus. Add independently transcribed real scans, more fonts/languages, photographic skew/noise, table-cell association truth and repeated OS/hardware runs before claiming production OCR accuracy. No commercial visual-model extraction accuracy is measured here.

### Precision investigation and corpus correction

The generator previously passed a bridged `CGRect` as `kCGPDFContextMediaBox`, whose value must be a data object. Quartz consequently used a default 612×792-point Letter page rather than the intended 1000×1200-point page. It now supplies the media box through the typed `CGContext` initializer. This changes the generated raster corpus; changes in the table above are **not an OCR-engine accuracy improvement**. The earlier numbers remain in the prior product-readiness receipt and `.scratch/ocr-precision-before/report.json`; the corrected run is `.scratch/ocr-precision-final/report.json`.

On identical original PDFs, bounded experiments with 1200/2700/3600-pixel render limits, nearest/high interpolation, line crops, grayscale thresholding and contrast attenuation did not improve all cases and sometimes lost numerical signs or accounting parentheses. No such heuristic was shipped. Language correction did not fix the clean percentage errors. Accurate Vision revisions 1 and 2 exceeded a 10-second experimental deadline; accurate cropped revision 3 exceeded 12 seconds. The product retains its bounded fast CPU path, unchanged. A new literal-punctuation case prevents mistaking these observations for permission to replace printed `0/.` with `%`; no text substitution or ground-truth dictionary matching is used.
