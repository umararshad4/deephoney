#!/usr/bin/env python3
"""Offline, coordinator-run blind comparison. Never calls models or providers."""
import argparse
import hashlib
import json
import math
from pathlib import Path
from urllib.parse import urlsplit
import random
import secrets
import shutil
import statistics
import sys


def require(condition, message):
    if not condition:
        raise ValueError(message)


def read(path):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, f'duplicate JSON key: {key}')
            result[key] = value
        return result
    return json.loads(Path(path).read_text(), object_pairs_hook=unique,
                      parse_constant=lambda value: (_ for _ in ()).throw(ValueError(value)))


def fields(obj, keys):
    require(isinstance(obj, dict) and set(obj) == set(keys.split()), f'expected fields: {keys}')


def string(value):
    require(isinstance(value, str) and bool(value.strip()), 'expected nonempty string')


def source_url(value):
    if not isinstance(value, str):
        return False
    try:
        parsed = urlsplit(value)
        return parsed.scheme in ('http', 'https') and bool(parsed.hostname) and parsed.username is None and parsed.password is None
    except ValueError:
        return False


def number(value):
    require(value is None or (type(value) in (int, float) and math.isfinite(value) and value >= 0),
            'measurement must be null or a finite nonnegative number')


def write(path, obj):
    with Path(path).open('x') as out:
        json.dump(obj, out, indent=2, allow_nan=False)
        out.write('\n')


def artifact(base, name):
    string(name)
    candidate = base / name
    require(not Path(name).is_absolute() and '..' not in Path(name).parts, 'unsafe artifact path')
    require(not any(p.is_symlink() for p in [candidate, *candidate.parents]), 'symlinks refused')
    candidate = candidate.resolve()
    require(candidate.is_relative_to(base.resolve()) and candidate.is_file(), 'artifact missing/outside manifest directory')
    require(candidate.suffix.lower() in ('.md', '.pdf'), 'only reviewed Markdown/PDF artifacts accepted')
    require(candidate.stat().st_size <= 20 * 1024 * 1024, 'artifact exceeds 20 MiB')
    return candidate


def sample_registry(path, runs):
    """Freeze coordinator-selected claim locations before independent grading."""
    data = read(path)
    require(isinstance(data, list), 'sample registry must be an array')
    expected = {(r['platform'], r['case_id'], r['replicate']) for r, _ in runs if r['status'] == 'completed'}
    registry = {}
    sizes = set()
    for row in data:
        fields(row, 'platform case_id replicate citation_checks fact_checks')
        string(row['platform']); string(row['case_id'])
        require(type(row['replicate']) is int, 'invalid sample replicate')
        identity = (row['platform'], row['case_id'], row['replicate'])
        require(identity in expected and identity not in registry, 'unknown, failed or duplicate sample report')
        for kind in ('citation_checks', 'fact_checks'):
            checks = row[kind]
            require(isinstance(checks, list) and checks, 'nonempty frozen samples required')
            ids = set()
            for check in checks:
                fields(check, 'id location statement')
                for value in check.values():
                    string(value)
                    require('REPLACE' not in value, 'unfinished sample description')
                    require(row['platform'].casefold() not in value.casefold(), 'sample contains platform identity')
                require(check['id'] not in ids, 'duplicate frozen sample ID')
                ids.add(check['id'])
        sizes.add((len(row['citation_checks']), len(row['fact_checks'])))
        registry[identity] = {k: row[k] for k in ('citation_checks', 'fact_checks')}
    require(set(registry) == expected, 'sample registry must cover every completed report')
    require(len(sizes) <= 1, 'all completed reports must use the same sample counts')
    return registry


def prepare(manifest_path, output, key_path, sample_path=None):
    manifest = read(manifest_path)
    fields(manifest, 'study_id synthetic reference cases runs')
    string(manifest['study_id'])
    require(type(manifest['synthetic']) is bool, 'synthetic must be boolean')
    reference = manifest['reference']
    fields(reference, 'title provenance evidence')
    string(reference['title']); string(reference['provenance'])
    require(isinstance(reference['evidence'], list) and reference['evidence'], 'reference evidence required')
    evidence = set()
    for item in reference['evidence']:
        fields(item, 'id case_id statement source attribution')
        for value in item.values(): string(value)
        require(source_url(item['source']), 'reference source must be an HTTP(S) URL without credentials')
        require(item['id'] not in evidence, 'duplicate evidence ID')
        evidence.add(item['id'])
    cases = {}
    for case in manifest['cases']:
        fields(case, 'id question')
        string(case['id']); string(case['question'])
        require(case['id'] not in cases, 'duplicate case')
        cases[case['id']] = case
    require(cases, 'cases required')
    require(all(item['case_id'] in cases for item in reference['evidence']), 'unknown reference case')
    require(all(any(e['case_id'] == c for e in reference['evidence']) for c in cases), 'every case needs reference evidence')
    require(isinstance(manifest['runs'], list) and manifest['runs'], 'runs required')
    runs = []; identities = set(); platforms = set()
    base = Path(manifest_path).resolve().parent
    for raw in manifest['runs']:
        fields(raw, 'platform case_id replicate status artifact sanitized_reviewed elapsed_seconds observed_cost currency measurement_notes failure_reason')
        string(raw['platform']); string(raw['measurement_notes'])
        require(raw['case_id'] in cases, 'unknown case')
        require(type(raw['replicate']) is int and raw['replicate'] > 0, 'invalid replicate')
        identity = (raw['platform'], raw['case_id'], raw['replicate'])
        require(identity not in identities, 'duplicate run'); identities.add(identity)
        platforms.add(raw['platform'])
        require(raw['status'] in ('completed', 'failed'), 'invalid status')
        number(raw['elapsed_seconds']); number(raw['observed_cost'])
        require(raw['currency'] is None or (isinstance(raw['currency'], str) and len(raw['currency']) == 3 and raw['currency'].isascii() and raw['currency'].isalpha() and raw['currency'].isupper()), 'currency must be three uppercase ASCII letters or null')
        require(raw['observed_cost'] is None or raw['currency'] is not None, 'known cost needs currency')
        if raw['status'] == 'completed':
            require(raw['sanitized_reviewed'] is True and raw['failure_reason'] is None, 'completed artifact needs coordinator sanitization review')
            source = artifact(base, raw['artifact'])
            if source.suffix.lower() == '.md':
                require(raw['platform'].casefold() not in source.read_text().casefold(), 'report contains platform identity; sanitize before preparation')
        else:
            require(raw['artifact'] is None and raw['sanitized_reviewed'] is False, 'failed runs cannot have report artifacts')
            string(raw['failure_reason']); source = None
        runs.append((dict(raw), source))
    # A missing platform/case is not silently removed from the denominator.
    require(all(any(r['platform'] == p and r['case_id'] == c for r, _ in runs) for p in platforms for c in cases), 'record every platform/case, including failed runs')
    require(manifest['synthetic'] or sample_path is not None, 'real study requires a frozen sample registry (--samples)')
    samples = sample_registry(sample_path, runs) if sample_path is not None else None
    out = Path(output).absolute(); key = Path(key_path).absolute()
    require(not out.is_symlink() and not key.is_symlink(), 'output/key symlinks refused')
    out = out.resolve(); key = key.resolve()
    require(not out.exists() and not out.is_symlink(), 'output already exists')
    require(not key.exists() and not key.is_symlink(), 'key already exists')
    require(not key.is_relative_to(out), 'coordinator key must be outside blind packet')
    require(key.parent.exists(), 'key parent missing')
    require(len({frozenset((r['case_id'], r['replicate']) for r, _ in runs if r['platform'] == p) for p in platforms}) == 1, 'platforms must share planned case/replicate attempts; include failures')
    random.SystemRandom().shuffle(runs)
    public = []; private = []
    for raw, source in runs:
        blind_id = secrets.token_hex(12)
        entry = {'blind_id': blind_id, 'case_id': raw['case_id'], 'status': raw['status'], 'artifact': None, 'sha256': None}
        if source:
            entry['artifact'] = blind_id + source.suffix.lower()
            entry['sha256'] = hashlib.sha256(source.read_bytes()).hexdigest()
        public.append(entry); private.append({'blind_id': blind_id, **raw, 'sha256': entry['sha256']})
    out.mkdir()
    for entry, (_, source) in zip(public, runs):
        if source: shutil.copyfile(source, out / entry['artifact'])
    packet = {'study_id': manifest['study_id'], 'synthetic': manifest['synthetic'], 'reference': reference, 'cases': list(cases.values()), 'runs': public}
    if samples is not None:
        packet['samples'] = [{'blind_id': entry['blind_id'], **samples[(raw['platform'], raw['case_id'], raw['replicate'])]}
                             for entry, (raw, _) in zip(public, runs) if raw['status'] == 'completed']
    write(out / 'packet.json', packet)
    template = {'study_id': manifest['study_id'], 'synthetic': manifest['synthetic'], 'grader': 'REPLACE-WITH-INDEPENDENT-GRADER-ID', 'independent_human': False, 'grades': []}
    for entry in public:
        if entry['status'] == 'completed':
            template['grades'].append({'blind_id': entry['blind_id'], 'citation_checks': [], 'fact_checks': [], 'reference_checks': [{'id': e['id'], 'judgment': 'unresolved', 'reason': 'REPLACE after reviewing report and reference', 'source_urls': [e['source']]} for e in reference['evidence'] if e['case_id'] == entry['case_id']]})
    if samples is not None:
        frozen = {row['blind_id']: row for row in packet['samples']}
        for row in template['grades']:
            for kind in ('citation_checks', 'fact_checks'):
                row[kind] = [{'id': check['id'], 'judgment': 'partial' if kind == 'citation_checks' else 'unresolved',
                              'reason': 'REPLACE after independently checking the frozen claim and sources', 'source_urls': []}
                             for check in frozen[row['blind_id']][kind]]
    write(out / 'grade-template.json', template)
    # Exclusive creation; coordinator-only filesystem permissions before writing.
    import os
    fd = os.open(key, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, 'w') as handle:
        json.dump({'study_id': manifest['study_id'], 'synthetic': manifest['synthetic'], 'packet_sha256': hashlib.sha256((out / 'packet.json').read_bytes()).hexdigest(), 'runs': private}, handle, indent=2)
    return {'packet': str(out), 'key': str(key), 'runs': len(runs)}


def grade(packet, path):
    data = read(path)
    fields(data, 'study_id grader independent_human synthetic grades')
    require(data['study_id'] == packet['study_id'] and data['synthetic'] is packet['synthetic'], 'study mismatch')
    string(data['grader'])
    require('REPLACE' not in data['grader'], 'unfinished grader identity')
    require(data['independent_human'] is True or (packet['synthetic'] and data['independent_human'] is False), 'real studies require independent human attestation')
    completed = {r['blind_id']: r for r in packet['runs'] if r['status'] == 'completed'}
    checked = {}
    for item in data['grades']:
        fields(item, 'blind_id citation_checks fact_checks reference_checks')
        bid = item['blind_id']
        require(bid in completed and bid not in checked, 'unknown, failed or duplicate report grade')
        for kind in ('citation_checks', 'fact_checks', 'reference_checks'):
            require(isinstance(item[kind], list) and item[kind], f'{kind} must contain checks')
            ids = set()
            for check in item[kind]:
                fields(check, 'id judgment reason source_urls')
                string(check['id']); string(check['reason'])
                require('REPLACE' not in check['reason'], 'unfinished worksheet reason')
                require(check['id'] not in ids, 'duplicate check ID'); ids.add(check['id'])
                choices = {'citation_checks': ('supported', 'partial', 'unsupported'), 'fact_checks': ('correct', 'error', 'unresolved'), 'reference_checks': ('covered', 'missed', 'unresolved')}
                require(check['judgment'] in choices[kind], 'invalid judgment')
                require(isinstance(check['source_urls'], list) and check['source_urls'] and all(source_url(s) for s in check['source_urls']), 'check attribution required')
            if kind != 'reference_checks' and 'samples' in packet:
                frozen = next(row for row in packet['samples'] if row['blind_id'] == bid)
                require(ids == {c['id'] for c in frozen[kind]}, 'grades must match frozen sample IDs')
            if kind == 'reference_checks':
                expected = {e['id'] for e in packet['reference']['evidence'] if e['case_id'] == completed[bid]['case_id']}
                require(ids == expected, 'reference coverage must grade every case reference exactly once')
        checked[bid] = item
    require(set(checked) == set(completed), 'missing completed report grades')
    return data, checked


def summarize(packet_path, key_path, grade_paths, output):
    packet = read(packet_path); key = read(key_path)
    require(hashlib.sha256(Path(packet_path).read_bytes()).hexdigest() == key['packet_sha256'], 'packet changed after preparation')
    require(packet['study_id'] == key['study_id'] and packet['synthetic'] is key['synthetic'], 'key mismatch')
    private = {r['blind_id']: r for r in key['runs']}
    require(len(private) == len(key['runs']) and set(private) == {r['blind_id'] for r in packet['runs']}, 'run key mismatch')
    for r in packet['runs']:
        original = private[r['blind_id']]
        require(all(r[k] == original[k] for k in ('status', 'case_id', 'sha256')), 'packet/key altered')
        if r['artifact']:
            source = artifact(Path(packet_path).resolve().parent, r['artifact'])
            require(hashlib.sha256(source.read_bytes()).hexdigest() == r['sha256'], 'artifact changed after preparation')
    require(len(grade_paths) >= 2, 'at least two independent grade files required')
    grades = [grade(packet, p) for p in grade_paths]
    require(len({g['grader'].strip().casefold() for g, _ in grades}) == len(grades), 'duplicate grader')
    per_run = []
    for run in packet['runs']:
        raw = private[run['blind_id']]
        row = {k: raw[k] for k in ('platform', 'case_id', 'replicate', 'status', 'elapsed_seconds', 'observed_cost', 'currency', 'measurement_notes', 'failure_reason')}
        row['blind_id'] = run['blind_id']; row['human_judgments'] = []; agreement = {}
        if run['status'] == 'completed':
            for g, checks in grades:
                item = checks[run['blind_id']]
                counts = {}
                for kind in ('citation_checks', 'fact_checks', 'reference_checks'):
                    counts[kind] = {judgment: sum(c['judgment'] == judgment for c in item[kind]) for judgment in sorted({c['judgment'] for c in item[kind]})}
                    counts[kind]['checked'] = len(item[kind])
                row['human_judgments'].append({'grader': g['grader'], **counts})
            # Item-level agreement only on the same declared sample IDs.
            for kind in ('citation_checks', 'fact_checks', 'reference_checks'):
                samples = [{c['id']: c['judgment'] for c in checks[run['blind_id']][kind]} for _, checks in grades]
                require(all(set(s) == set(samples[0]) for s in samples), 'graders used different samples; reconcile sample IDs without changing independent judgments')
                agreement[kind] = {'unanimous': sum(len({s[i] for s in samples}) == 1 for i in samples[0]), 'items': len(samples[0])}
        row['agreement'] = agreement; per_run.append(row)
    totals = []
    for platform in sorted({r['platform'] for r in per_run}):
        rows = [r for r in per_run if r['platform'] == platform]
        elapsed = [r['elapsed_seconds'] for r in rows if r['elapsed_seconds'] is not None]
        money = {}
        for r in rows:
            if r['observed_cost'] is not None: money[r['currency']] = money.get(r['currency'], 0) + r['observed_cost']
        human_totals = []
        for grader, _ in grades:
            aggregate = {}
            for kind, numerator in (('citation_checks', 'supported'), ('fact_checks', 'error'), ('reference_checks', 'missed')):
                counts = {}
                for row in rows:
                    for judgment in row['human_judgments']:
                        if judgment['grader'] == grader['grader']:
                            for label, count in judgment[kind].items():
                                counts[label] = counts.get(label, 0) + count
                denominator = counts.get('checked', 0)
                aggregate[kind] = {**counts, 'rate_of_' + numerator: counts.get(numerator, 0) / denominator if denominator else None}
            human_totals.append({'grader': grader['grader'], **aggregate})
        totals.append({'human_sample_totals': human_totals, 'platform': platform, 'attempted': len(rows), 'completed': sum(r['status'] == 'completed' for r in rows), 'completion_rate': sum(r['status'] == 'completed' for r in rows) / len(rows), 'latency_known_runs': len(elapsed), 'latency_unknown_runs': len(rows)-len(elapsed), 'mean_observed_seconds': statistics.mean(elapsed) if elapsed else None, 'observed_cost_by_currency': money, 'cost_known_runs': sum(r['observed_cost'] is not None for r in rows), 'cost_unknown_runs': sum(r['observed_cost'] is None for r in rows)})
    result = {'study_id': packet['study_id'], 'synthetic': packet['synthetic'], 'interpretation': 'Synthetic instrumentation only; no research quality result' if packet['synthetic'] else 'Independent human judgments on checked samples only; no automatic truth or superiority claim', 'sampling_frozen': 'samples' in packet, 'platforms': totals, 'runs': per_run, 'grades_sha256': [hashlib.sha256(Path(p).read_bytes()).hexdigest() for p in grade_paths]}
    if output is not None: write(output, result)
    return result


def readiness(summary):
    """A descriptive-study completeness gate, never a superiority test."""
    missing = []
    if summary['synthetic']: missing.append('synthetic study cannot establish research quality')
    if not summary['sampling_frozen']: missing.append('claim sample descriptions were not frozen before grading')
    rows = summary['runs']
    cases = {r['case_id'] for r in rows}
    platforms = {r['platform'] for r in rows}
    if len(cases) < 10: missing.append('need at least 10 distinct cases')
    if len(platforms) < 2: missing.append('need at least two platforms')
    if any(len({r['replicate'] for r in rows if r['platform'] == p and r['case_id'] == c}) < 2 for p in platforms for c in cases):
        missing.append('need at least two planned replicates per platform and case, including failures')
    completed = [r for r in rows if r['status'] == 'completed']
    if not completed: missing.append('no completed reports to assess')
    if any(j[k]['checked'] < 5 for r in completed for j in r['human_judgments'] for k in ('citation_checks', 'fact_checks')):
        missing.append('need at least five frozen citation and fact samples per completed report')
    if any(not any(r['platform'] == p for r in completed) for p in platforms):
        missing.append('at least one platform has no completed report; compare completion only')
    return {'study_id': summary['study_id'], 'ready_for_descriptive_review': not missing,
            'missing': missing, 'case_count': len(cases), 'platform_count': len(platforms),
            'timing_comparison_ready': not missing and all(r['elapsed_seconds'] is not None for r in rows),
            'cost_comparison_ready': not missing and all(r['observed_cost'] is not None for r in rows)
                                     and len({r['currency'] for r in rows}) == 1,
            'superiority_established': False,
            'interpretation': 'Completeness thresholds only; human attestations and reference quality require independent review. No statistical or causal superiority claim.'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='action', required=True)
    p = sub.add_parser('prepare'); p.add_argument('manifest'); p.add_argument('--out', required=True); p.add_argument('--key', required=True); p.add_argument('--samples')
    p = sub.add_parser('summarize'); p.add_argument('packet'); p.add_argument('--key', required=True); p.add_argument('--grades', nargs='+', required=True); p.add_argument('--out', required=True)
    p = sub.add_parser('readiness'); p.add_argument('packet'); p.add_argument('--key', required=True); p.add_argument('--grades', nargs='+', required=True); p.add_argument('--out', required=True)
    args = parser.parse_args()
    try:
        if args.action == 'prepare': result = prepare(args.manifest, args.out, args.key, args.samples)
        elif args.action == 'summarize': result = summarize(args.packet, args.key, args.grades, args.out)
        else:
            result = readiness(summarize(args.packet, args.key, args.grades, None))
            write(args.out, result)
        print(json.dumps(result, indent=2))
        if args.action == 'readiness' and not result['ready_for_descriptive_review']: return 1
    except (ValueError, OSError, KeyError, TypeError) as error:
        print(f'comparison refused: {error}', file=sys.stderr); return 1
    return 0

if __name__ == '__main__':
    sys.exit(main())
