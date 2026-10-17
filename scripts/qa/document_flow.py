#!/usr/bin/env python3
"""Offline subprocess acceptance: real OCR, synthetic image model, transient PDF failure."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

ROOT = Path(__file__).resolve().parents[2]
QUOTE = 'Meridian audited sales'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', default=str(ROOT / 'target/debug/examples/cli'))
    parser.add_argument('--out', required=True, help='new local output directory')
    args = parser.parse_args()
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=False)
    counts = {'pdf': 0, 'images': 0}

    class Handler(BaseHTTPRequestHandler):
        def respond(self, value, content_type='application/json', status=200):
            body = value if isinstance(value, bytes) else json.dumps(value).encode()
            self.send_response(status)
            self.send_header('Content-Type', content_type)
            self.send_header('Content-Length', str(len(body)))
            self.send_header('Retry-After', '0')
            self.end_headers()
            self.wfile.write(body)

        def do_GET(self):
            if self.path.startswith('/search?'):
                self.respond({'results': [{'url': base + '/scan.pdf', 'title': QUOTE, 'content': QUOTE}]})
            else:
                counts['pdf'] += 1
                self.respond(b'temporary failure', 'text/plain', 503) if counts['pdf'] == 1 else self.respond(
                    (ROOT / 'crates/engine/tests/fixtures/scanned-table-chart.pdf').read_bytes(), 'application/pdf')

        def do_POST(self):
            request = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            system = request['messages'][0]['content']
            content = request['messages'][1]['content']
            if isinstance(content, list):
                counts['images'] += 1
                prompt = next(block['text'] for block in content if block['type'] == 'text')
                image = next(block['image_url']['url'] for block in content if block['type'] == 'image_url')
                assert image.startswith('data:image/png;base64,')
                lines = json.loads(prompt.split('OCR: ', 1)[1])
                def cell(text):
                    return {'text': text, 'line': next(line['line'] for line in lines if line['text'] == text)}
                result = {'tables': [{'rows': [[cell('Year'), cell('Revenue')], [cell('2024'), cell('125')], [cell('2025'), cell('175')]]}],
                          'charts': [{'points': [{'label': cell('2024'), 'value': cell('125')}, {'label': cell('2025'), 'value': cell('175')}]}]}
            elif 'RESEARCH PLAN' in system:
                result = {'done': False, 'lenses': [{'name': 'Document', 'question': 'What is the document heading?', 'sources': ['searxng'], 'queries': ['Meridian audited sales']}]}
            elif 'RESEARCH GAPS' in system:
                result = {'done': True, 'lenses': []}
            elif 'EXTRACT CLAIMS' in system:
                assert 'Visual table/chart candidates' in content, 'optional visual candidates did not reach extraction'
                result = {'claims': [{'text': 'The document is headed Meridian audited sales.', 'quote': QUOTE}]} if QUOTE in content else {'claims': []}
            elif 'VERIFY REPORT STATEMENTS' in system:
                result = {'supported': [0, 1]}
            elif 'REPORT DRAFT' in system:
                result = {'summary': [{'text': 'The document is headed Meridian audited sales.', 'claims': [0]}],
                          'findings': [{'text': 'The document is headed Meridian audited sales.', 'claims': [0]}], 'contested': []}
            else:
                result = {'contradictions': [], 'verdicts': []}
            self.respond({'choices': [{'message': {'content': json.dumps(result)}}], 'usage': {'prompt_tokens': 7, 'completion_tokens': 3}})

        def log_message(self, *_):
            pass

    class Server(ThreadingHTTPServer):
        def server_bind(self):
            from socketserver import TCPServer
            TCPServer.server_bind(self)
            self.server_name, self.server_port = 'localhost', self.server_address[1]

    server = Server(('127.0.0.1', 0), Handler)
    base = f'http://127.0.0.1:{server.server_port}'
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    env = dict(os.environ)
    for name in ['ANTHROPIC_API_KEY', 'OPENAI_API_KEY', 'BRAVE_API_KEY', 'EXA_API_KEY', 'TAVILY_API_KEY', 'GITHUB_TOKEN',
                 'COURTLISTENER_TOKEN', 'DEEPHONEY_CONTACT', 'VERIFIER_MODEL', 'VERIFIER_API_KEY', 'VERIFIER_BASE_URL',
                 'DEEPHONEY_TOR', 'DEEPHONEY_BROWSER']:
        env[name] = ''
    env.update(OPENAI_BASE_URL=base, MODEL='synthetic-vision', SEARXNG_URL=base, DEEPHONEY_VISUAL_DOCUMENTS='1')
    try:
        run = subprocess.run([args.binary, 'What is the document heading?', '--depth', '1', '--breadth', '1', '--pages', '1', '--json'],
                             cwd=ROOT, env=env, capture_output=True, text=True, timeout=150)
        (out / 'events.jsonl').write_text(run.stderr)
        assert run.returncode == 0, run.stderr
        report = json.loads(run.stdout)
        (out / 'report.json').write_text(run.stdout)
        events = [json.loads(line) for line in run.stderr.splitlines()]
        assert report['ledger']['claims'][0]['evidence'][0]['quote'] == QUOTE
        assert report['ledger']['claims'][0]['evidence'][0]['locator'].startswith('PDF page 1;')
        assert 'machine transcription' in report['ledger']['claims'][0]['evidence'][0]['locator']
        assert 'Machine-transcribed evidence: check the original page' in report['markdown']
        assert report['metrics']['retrieval_retries'] == 1 and report['metrics']['recovered_requests'] == 1
        assert counts == {'pdf': 2, 'images': 1}, counts
        assert report['metrics']['model_calls'] >= 5
        assert any('OCR' in json.dumps(event) for event in events), events
        receipt = {'result': 'pass', 'synthetic_model': True, 'real_native_ocr': True, 'requests': counts, 'metrics': report['metrics'],
                   'limits': 'Tests integration and provenance; not model accuracy or chart association correctness.'}
        (out / 'receipt.json').write_text(json.dumps(receipt, indent=2))
        print(json.dumps(receipt, indent=2))
    finally:
        server.shutdown()
        server.server_close()
        thread.join()


if __name__ == '__main__':
    main()
