#!/usr/bin/env python3
"""Create synthetic instrument receipts, not human or competitor results."""
import argparse
import json
from pathlib import Path
import shutil
import subprocess
import sys
from test_compare import ComparisonTests

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--out', required=True)
args = parser.parse_args()
out = Path(args.out).resolve()
if out.exists():
    parser.error('output already exists')
fixture = ComparisonTests()
fixture.setUp()
try:
    packet = fixture.prepare()
    paths = [fixture.grades(packet, name) for name in ('synthetic-grader-one', 'synthetic-grader-two')]
    # Explicit disagreement proves summaries retain independent judgments.
    grade = json.loads(paths[1].read_text())
    grade['grades'][0]['citation_checks'][0]['judgment'] = 'partial'
    paths[1].write_text(json.dumps(grade, indent=2))
    shutil.copytree(fixture.root, out)
finally:
    fixture.doCleanups()
command = [sys.executable, str(Path(__file__).with_name('compare.py')), 'summarize', str(out/'packet/packet.json'), '--key', str(out/'key.json'), '--grades', *(str(out/p.name) for p in paths), '--out', str(out/'summary.json')]
result = subprocess.run(command, capture_output=True, text=True)
(out/'receipt.json').write_text(json.dumps({'synthetic':True, 'command':command, 'exit_code':result.returncode, 'stdout':result.stdout, 'stderr':result.stderr}, indent=2))
print(f'Synthetic receipt: {out}/receipt.json')
sys.exit(result.returncode)
