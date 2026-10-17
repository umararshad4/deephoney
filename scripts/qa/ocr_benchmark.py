#!/usr/bin/env python3
"""Measure the actual bundled native OCR source against synthetic raster PDF truth."""
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import platform
import re
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
TRUTH = ROOT / "crates/engine/tests/fixtures/ocr-benchmark.json"
NATIVE = ROOT / "crates/engine/native/document-reader.swift"
GENERATOR = ROOT / "crates/engine/tests/fixtures/generate-ocr-benchmark.swift"
NUMBERS = re.compile(r"(?<!\w)(?:\(\s*)?[+-]?(?:[$€£]\s*)?\d+(?:,\d{3})*(?:\.\d+)?(?:%|‰)?\)?(?!\w)")


def distance(expected, actual):
    """Levenshtein distance, linear memory; insertion errors can make WER exceed 1."""
    previous = list(range(len(actual) + 1))
    for i, left in enumerate(expected, 1):
        current = [i]
        for j, right in enumerate(actual, 1):
            current.append(min(current[-1] + 1, previous[j] + 1, previous[j - 1] + (left != right)))
        previous = current
    return previous[-1]


def numbers(text):
    # Keep signs, currency, accounting parentheses, decimals and units significant.
    return [re.sub(r"\s", "", match.group()) for match in NUMBERS.finditer(text)]


def counts(expected, actual):
    left, right = Counter(expected), Counter(actual)
    matched = sum((left & right).values())
    return {"expected": len(expected), "observed": len(actual), "matched": matched,
            "recall": matched / len(expected) if expected else None,
            "precision": matched / len(actual) if actual else None,
            "missing": list((left - right).elements()), "unexpected": list((right - left).elements())}


def measure(expected, actual):
    # Unicode whitespace and case are normalized for word metrics only.
    expected_words, actual_words = expected.casefold().split(), actual.casefold().split()
    edits = distance(expected_words, actual_words)
    return {"words": counts(expected_words, actual_words), "word_edits": edits,
            "word_error_rate": edits / len(expected_words) if expected_words else None,
            "numbers_exact": counts(numbers(expected), numbers(actual))}


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", required=True, type=Path, help="New output directory; never overwritten")
    args = parser.parse_args()
    if sys.platform != "darwin":
        parser.error("requires macOS 15+ with Xcode Command Line Tools; no substitute OCR is scored")
    args.out.mkdir(parents=True, exist_ok=False)
    out = args.out.resolve()
    spec = json.loads(TRUTH.read_text())
    helper = out / "document-reader"
    subprocess.run(["xcrun", "swiftc", "-O", "-target",
                    ("arm64" if platform.machine() == "arm64" else "x86_64") + "-apple-macosx15.0",
                    str(NATIVE), "-o", str(helper)], check=True, timeout=180)
    subprocess.run(["swift", str(GENERATOR), str(TRUTH), str(out)], check=True, timeout=120)
    report = {"schema": 1, "synthetic": True, "system": platform.platform(),
              "truth_sha256": sha(TRUTH), "native_source_sha256": sha(NATIVE),
              "generator_sha256": sha(GENERATOR), "scope": spec["description"], "cases": []}
    for case in spec["cases"]:
        expected = "\n".join(case.get("lines", spec["lines"]))
        started = time.monotonic()
        row = {"id": case["id"], "conditions": case, "pdf_sha256": sha(out / (case["id"] + ".pdf"))}
        try:
            result = subprocess.run([str(helper)], input=(out / (case["id"] + ".pdf")).read_bytes(),
                                    capture_output=True, timeout=65, check=True)
            native = json.loads(result.stdout)
            if native.get("error") or native.get("totalPages") != 1 or len(native.get("pages", [])) != 1:
                raise ValueError(native.get("error") or "expected exactly one OCR page")
            actual = "\n".join(line["text"] for line in native["pages"][0]["lines"])
            row.update(status="measured", metrics=measure(expected, actual), observed_text=actual,
                       warnings=native["pages"][0].get("warnings", []),
                       forbidden_tokens_observed=[token for token in case.get("forbidden_observed", [])
                                                  if token in actual])
        except (subprocess.SubprocessError, ValueError, KeyError, TypeError) as error:
            row.update(status="error", error=str(error)[:1000])
        row["elapsed_ms"] = round((time.monotonic() - started) * 1000)
        report["cases"].append(row)
    measured = [row for row in report["cases"] if row["status"] == "measured"]
    report["completion"] = {"measured": len(measured), "attempted": len(spec["cases"])}
    gates = spec["regression_gates"]
    report["regression_checks"] = [{
        "case": row["id"],
        "passed": row["status"] == "measured"
        and row["metrics"]["words"]["recall"] >= gates["min_word_recall"]
        and row["metrics"]["numbers_exact"]["recall"] >= gates["min_exact_numeric_recall"],
    } for row in report["cases"] if row["id"] in gates["cases"]]
    report["regression_checks"].extend({"case": row["id"],
        "passed": row["status"] == "measured" and not row["forbidden_tokens_observed"]}
        for row in report["cases"] if row["conditions"].get("forbidden_observed"))
    report["regression_gates"] = gates
    # Publish per-case metrics, rather than hiding difficult cases in an average.
    (out / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"report": str(out / "report.json"), "completion": report["completion"],
                      "cases": [{"id": row["id"], "status": row["status"], "metrics": row.get("metrics")}
                                for row in report["cases"]]}, indent=2))
    return 0 if len(measured) == len(spec["cases"]) and all(
        item["passed"] for item in report["regression_checks"]) else 1


if __name__ == "__main__":
    raise SystemExit(main())
