#!/usr/bin/env python3
"""Repeat bounded provider diagnostics or summarize existing runs; no accuracy claims."""
import argparse
from collections import Counter
import hashlib
import json
import math
from pathlib import Path
import statistics
import subprocess
import time

PROVIDERS = frozenset('brave exa tavily searxng duckduckgo openalex semanticscholar arxiv crossref hackernews stackexchange sec_edgar github_code courtlistener crtsh muckrock telegram commoncrawl wayback ahmia'.split())
STATUSES = ('ok_nonempty', 'ok_empty', 'error', 'skipped')
MAX_BYTES = 1024 * 1024


def integer(value):
    return type(value) is int and 0 <= value <= 10**12


def load_run(path):
    if path.is_symlink() or not path.is_file() or path.stat().st_size > MAX_BYTES:
        raise ValueError('run must be a regular file no larger than 1 MiB')
    data = path.read_bytes()
    report = json.loads(data)
    if not isinstance(report, dict) or type(report.get('schema')) is not int or report.get('schema') != 1 or report.get('configuration') not in ('public_defaults', 'environment') or not integer(report.get('elapsed_ms')):
        raise ValueError('invalid diagnostic schema or configuration')
    rows = report.get('rows')
    if not isinstance(rows, list) or not rows or len(rows) > len(PROVIDERS):
        raise ValueError('invalid diagnostic rows')
    seen = set()
    for row in rows:
        if not isinstance(row, dict) or not isinstance(row.get('provider'), str) or row.get('provider') not in PROVIDERS or row['provider'] in seen:
            raise ValueError('invalid or duplicate provider')
        seen.add(row['provider'])
        if row.get('status') not in STATUSES or not isinstance(row.get('query'), str):
            raise ValueError('invalid status or missing query')
        for key in ('elapsed_ms', 'result_count', 'requests', 'retries', 'recovered'):
            if not integer(row.get(key)):
                raise ValueError('invalid diagnostic measurement')
        if (row['status'] == 'ok_nonempty') != (row['result_count'] > 0):
            raise ValueError('result count disagrees with diagnostic status')
        if row['status'] == 'skipped' and any(row[k] for k in ('requests', 'retries', 'recovered', 'elapsed_ms')):
            raise ValueError('skipped provider contains activity')
    # Hash only canonical recognized input: differently formatted copies are still duplicates.
    canonical = json.dumps(report, sort_keys=True, separators=(',', ':')).encode()
    return report, hashlib.sha256(canonical).hexdigest()


def summarize(paths, providers=(), minimum_runs=3, response_rate=0.8, nonempty_rate=0.8):
    runs, hashes = [], set()
    for path in paths:
        report, digest = load_run(Path(path))
        if digest in hashes:
            raise ValueError('duplicate diagnostic run')
        hashes.add(digest)
        runs.append(report)
    if not runs or len({r['configuration'] for r in runs}) != 1:
        raise ValueError('require runs using the same configuration mode')
    names = set(providers) or {row['provider'] for run in runs for row in run['rows']}
    result = []
    for name in sorted(names):
        rows = [row for run in runs for row in run['rows'] if row['provider'] == name]
        if len({row['query'] for row in rows}) > 1:
            raise ValueError('provider queries differ; summarize separately')
        counts = Counter(row['status'] for row in rows)
        attempted = len(rows) - counts['skipped']
        responses = counts['ok_nonempty'] + counts['ok_empty']
        response_fraction = responses / attempted if attempted else None
        nonempty_fraction = counts['ok_nonempty'] / attempted if attempted else None
        latencies = sorted(row['elapsed_ms'] for row in rows if row['status'] != 'skipped')
        if attempted == 0:
            gate = 'skipped'
        elif attempted < minimum_runs or len(rows) < len(runs):
            gate = 'insufficient_runs'
        elif response_fraction < response_rate or nonempty_fraction < nonempty_rate:
            gate = 'failed'
        else:
            gate = 'passed'
        result.append({'provider': name, 'counts': {s: counts[s] for s in STATUSES}, 'missing_runs': len(runs) - len(rows),
                       'attempted_runs': attempted, 'response_rate': response_fraction, 'nonempty_rate': nonempty_fraction,
                       'median_ms': statistics.median(latencies) if latencies else None,
                       'p95_ms': latencies[math.ceil(0.95 * len(latencies)) - 1] if latencies else None,
                       'requests': sum(r['requests'] for r in rows), 'retries': sum(r['retries'] for r in rows),
                       'recovered': sum(r['recovered'] for r in rows), 'gate': gate})
    # Never copy query, reason, endpoint, source path, or arbitrary fields into public summaries.
    return {'schema': 1, 'configuration': runs[0]['configuration'], 'run_count': len(runs),
            'thresholds': {'minimum_runs': minimum_runs, 'response_rate': response_rate, 'nonempty_rate': nonempty_rate},
            'passed': bool(result) and all(row['gate'] == 'passed' for row in result), 'providers': result}


def write_new(path, value):
    with Path(path).open('x') as output:
        json.dump(value, output, indent=2, allow_nan=False)
        output.write('\n')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    for command in ('run', 'summarize'):
        p = sub.add_parser(command)
        p.add_argument('--provider', choices=sorted(PROVIDERS), action='append', default=[])
        p.add_argument('--minimum-runs', type=int, default=3)
        p.add_argument('--response-rate', type=float, default=0.8)
        p.add_argument('--nonempty-rate', type=float, default=0.8)
        if command == 'summarize':
            p.add_argument('files', nargs='+')
            p.add_argument('--out', required=True, help='new summary file')
        else:
            p.add_argument('--binary', default='target/debug/examples/provider_health')
            mode = p.add_mutually_exclusive_group(required=True)
            mode.add_argument('--public', action='store_true')
            mode.add_argument('--configured', action='store_true', help='explicitly permit environment keys and service credits')
            p.add_argument('--runs', type=int, default=3)
            p.add_argument('--interval', type=float, default=30, help='seconds between runs, minimum 1')
            p.add_argument('--out', required=True, help='new output directory')
    args = parser.parse_args()
    if not 1 <= args.minimum_runs <= 100 or not all(math.isfinite(v) and 0 <= v <= 1 for v in (args.response_rate, args.nonempty_rate)):
        parser.error('invalid gate thresholds')
    try:
        if args.command == 'run':
            if not 1 <= args.runs <= 20 or not math.isfinite(args.interval) or not 1 <= args.interval <= 3600:
                parser.error('runs must be 1..20 and interval 1..3600 seconds')
            binary = Path(args.binary).resolve(strict=True)
            out = Path(args.out)
            out.mkdir(parents=True, exist_ok=False)
            files = []
            for index in range(args.runs):
                if index:
                    time.sleep(args.interval)
                # Individual invocations retain their own bounded provider rows and sanitized failures.
                for name in sorted(set(args.provider)) or [None]:
                    path = out / f'run-{index + 1:02d}-{name or "all"}.json'
                    command = [str(binary), '--configured' if args.configured else '--public', '--out', str(path)]
                    if name:
                        command += ['--provider', name]
                    run = subprocess.run(command, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=300, check=False)
                    if run.returncode not in (0, 1) or not path.is_file():
                        raise ValueError('diagnostic process failed without a usable report')
                    files.append(path)
            # Multiple selected providers are separate invocation groups. Merge their rows per repeat.
            if len(set(args.provider)) > 1:
                merged = []
                for index in range(args.runs):
                    group = [load_run(p)[0] for p in files if p.name.startswith(f'run-{index + 1:02d}-')]
                    path = out / f'combined-{index + 1:02d}.json'
                    write_new(path, {'schema': 1, 'configuration': group[0]['configuration'],
                                     'elapsed_ms': sum(r['elapsed_ms'] for r in group), 'rows': [row for r in group for row in r['rows']]})
                    merged.append(path)
                files = merged
            destination = out / 'summary.json'
        else:
            files, destination = args.files, args.out
        report = summarize(files, args.provider, args.minimum_runs, args.response_rate, args.nonempty_rate)
        write_new(destination, report)
        print(f"Provider matrix: {'passed' if report['passed'] else 'not passed'}; availability only, not research accuracy.")
        return 0 if report['passed'] else 1
    except (OSError, ValueError, subprocess.SubprocessError):
        # Do not echo arbitrary imported JSON, provider output, paths or environment values.
        print('Provider matrix failed: invalid input, existing output, or diagnostic process failure.')
        return 2


if __name__ == '__main__':
    raise SystemExit(main())
