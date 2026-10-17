"""Offline matrix gates and privacy checks; synthetic data is never a live result."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import provider_matrix as matrix


class MatrixTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def run_file(self, number, status='ok_nonempty', provider='crossref', **changes):
        row = dict(provider=provider, query='fixed', status=status, elapsed_ms=number * 10,
                   result_count=int(status == 'ok_nonempty'), requests=1, retries=0, recovered=0)
        if status == 'skipped':
            row.update(elapsed_ms=0, requests=0)
        row.update(changes)
        report = dict(schema=1, configuration='public_defaults', elapsed_ms=number * 10, rows=[row])
        path = self.root / f'{number}.json'
        path.write_text(json.dumps(report))
        return path

    def test_measured_rates_latency_and_gate(self):
        files = [self.run_file(i) for i in range(1, 4)]
        report = matrix.summarize(files)
        self.assertTrue(report['passed'])
        row = report['providers'][0]
        self.assertEqual((row['median_ms'], row['p95_ms']), (20, 30))
        self.assertEqual(row['response_rate'], 1)
        self.assertEqual(row['requests'], 3)

    def test_skipped_error_and_empty_are_not_nonempty_success(self):
        for status, expected in [('skipped', 'skipped'), ('error', 'failed'), ('ok_empty', 'failed')]:
            files = [self.run_file(i, status) for i in range(1, 4)]
            report = matrix.summarize(files)
            self.assertFalse(report['passed'])
            row = report['providers'][0]
            self.assertEqual(row['gate'], expected)
            self.assertEqual(row['response_rate'], None if status == 'skipped' else int(status == 'ok_empty'))
        # Operators can intentionally assess response-only availability; empty is still explicit.
        self.assertTrue(matrix.summarize(files, nonempty_rate=0)['passed'])

    def test_insufficient_missing_and_mixed_query(self):
        first, second = self.run_file(1), self.run_file(2, provider='arxiv')
        report = matrix.summarize([first, second], minimum_runs=1)
        self.assertFalse(report['passed'])
        self.assertEqual(report['providers'][0]['missing_runs'], 1)
        self.assertFalse(matrix.summarize([first])['passed'])
        second = self.run_file(2, query='different')
        with self.assertRaises(ValueError):
            matrix.summarize([first, second])

    def test_no_arbitrary_input_fields_enter_summary(self):
        files = [self.run_file(i, reason='secret-key', query='secret-query', endpoint='https://secret-host') for i in range(1, 4)]
        output = json.dumps(matrix.summarize(files))
        self.assertNotIn('secret', output)
        self.assertNotIn(str(self.root), output)

    def test_duplicates_bad_counts_boolean_and_mixed_config_refused(self):
        first = self.run_file(1)
        duplicate = self.root / 'duplicate.json'
        duplicate.write_text(json.dumps(json.loads(first.read_text()), indent=2))
        with self.assertRaises(ValueError):
            matrix.summarize([first, duplicate])
        for changes in [dict(result_count=0), dict(requests=True), dict(elapsed_ms=-1), dict(status='secret')]:
            with self.assertRaises(ValueError):
                matrix.summarize([self.run_file(2, **changes)])
        second = self.run_file(2)
        value = json.loads(second.read_text())
        value['configuration'] = 'environment'
        second.write_text(json.dumps(value))
        with self.assertRaises(ValueError):
            matrix.summarize([first, second])

    def test_symlink_oversize_and_overwrite_refused(self):
        first = self.run_file(1)
        link = self.root / 'link'
        link.symlink_to(first)
        with self.assertRaises(ValueError):
            matrix.load_run(link)
        first.write_bytes(b' ' * (matrix.MAX_BYTES + 1))
        with self.assertRaises(ValueError):
            matrix.load_run(first)
        with self.assertRaises(FileExistsError):
            matrix.write_new(first, {})

    def test_cli_invalid_threshold_and_all_skipped_exit(self):
        script = str(Path(matrix.__file__))
        files = [str(self.run_file(i, 'skipped')) for i in range(1, 4)]
        out = self.root / 'summary.json'
        run = subprocess.run([sys.executable, script, 'summarize', *files, '--out', str(out)], capture_output=True, text=True)
        self.assertEqual(run.returncode, 1, run.stderr)
        self.assertEqual(json.loads(out.read_text())['providers'][0]['gate'], 'skipped')
        run = subprocess.run([sys.executable, script, 'summarize', *files, '--out', str(out), '--response-rate', 'nan'], capture_output=True)
        self.assertEqual(run.returncode, 2)

    def test_run_invokes_only_explicit_scope_and_writes_summary(self):
        # Stand-in process tests orchestration only; no network/environment credentials read.
        binary = self.root / 'fake provider'
        binary.write_text('''#!/usr/bin/env python3
import json, sys, time
args = sys.argv
assert '--public' in args and '--configured' not in args
name = args[args.index('--provider') + 1]
n = time.monotonic_ns() % 1000000
row = dict(provider=name, query='fixed', status='ok_nonempty', elapsed_ms=n, result_count=1, requests=1, retries=0, recovered=0)
with open(args[args.index('--out') + 1], 'x') as f:
    json.dump(dict(schema=1, configuration='public_defaults', elapsed_ms=n, rows=[row]), f)
''')
        binary.chmod(0o700)
        out = self.root / 'matrix'
        run = subprocess.run([sys.executable, str(Path(matrix.__file__)), 'run', '--public', '--binary', str(binary),
                              '--provider', 'crossref', '--provider', 'arxiv', '--runs', '3', '--interval', '1', '--out', str(out)],
                             capture_output=True, text=True)
        self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
        result = json.loads((out / 'summary.json').read_text())
        self.assertTrue(result['passed'])
        self.assertEqual(result['run_count'], 3)
        self.assertEqual(len(result['providers']), 2)


if __name__ == '__main__':
    unittest.main()
