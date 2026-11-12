#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Sven Shi
# SPDX-License-Identifier: GPL-3.0-or-later
"""Run and summarize repository-owned hotpath reports; no cloud API or token."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
CATALOG = ROOT / '.github/hotpath-benchmarks.json'
ROUTINE = 'hotpath::criterion::routine'


def catalog(tier='core', path=CATALOG):
    suites = json.loads(path.read_text())
    targets, names, ids = set(), set(), set()
    selected = []
    for suite in suites:
        target = suite['target']
        if not re.fullmatch(r'[a-z][a-z0-9_]*', target) or target in targets:
            raise ValueError(f'invalid or duplicate target: {target}')
        targets.add(target)
        if type(suite['iterations']) is not int or suite['iterations'] <= 0:
            raise ValueError(f'invalid iterations: {target}')
        cases = []
        for case in suite['cases']:
            name, identifier = case['benchmark'], case['id']
            if not re.fullmatch(r'[a-z0-9][a-z0-9-]*', name) or name in names:
                raise ValueError(f'invalid or duplicate report name: {name}')
            # The just recipe interpolates this exact ID into a quoted shell
            # argument. Keep catalog IDs within a shell-safe alphabet.
            if not isinstance(identifier, str) or not re.fullmatch(r'[a-z][a-z0-9_./-]*', identifier) or identifier in ids:
                raise ValueError(f'invalid or duplicate Criterion id: {identifier}')
            if case['tier'] not in ('core', 'extended'):
                raise ValueError(f'invalid tier: {name}')
            if 'iterations' in case and (type(case['iterations']) is not int or case['iterations'] <= 0):
                raise ValueError(f'invalid case iterations: {name}')
            if case.get('measurement', 'both') not in ('native', 'both'):
                raise ValueError(f'invalid measurement: {name}')
            names.add(name)
            ids.add(identifier)
            if tier == 'all' or case['tier'] == 'core':
                cases.append(case)
        if cases:
            selected.append({**suite, 'cases': cases})
    if not selected:
        raise ValueError('empty benchmark catalog')
    return selected


def entry(report, section):
    rows = report.get(section, {}).get('data', [])
    matches = [row for row in rows if row['name'] == ROUTINE]
    if len(matches) != 1:
        raise ValueError(f'{section}: expected one per-iteration routine entry')
    return matches[0]


def validate(report, identifier, iterations, kind):
    metadata = report['user_metadata']
    if (metadata['criterion_benchmark'], metadata['criterion_iterations']) != (identifier, str(iterations)):
        raise ValueError('report benchmark/iteration identity mismatch')
    if report['meta']['run'] != kind:
        raise ValueError(f'expected {kind} report')
    section = 'functions_timing' if kind == 'native' else 'functions_icount'
    row = entry(report, section)
    if row['calls'] != iterations or row['sampled_calls'] != iterations:
        raise ValueError(f'{section}: incomplete or sampled routine measurements')
    if kind == 'native':
        if not report['functions_alloc'].get('description', '').startswith('Cumulative allocation bytes'):
            raise ValueError('allocation report must include nested calls in bytes mode')
        allocation = entry(report, 'functions_alloc')
        if allocation['calls'] != iterations or allocation['sampled_calls'] != iterations:
            raise ValueError('incomplete allocation measurements')
    elif not report[section].get('tool', '').startswith('callgrind-'):
        raise ValueError('instruction report has no Callgrind version')
    elif quantity(row['total'], {'': 1}) <= 0:
        raise ValueError('instruction report contains no measured work')


def quantity(value, units):
    match = re.fullmatch(r'([0-9]+(?:\.[0-9]+)?)\s*([^\d\s]*)', value.replace(',', '').strip())
    if match is None or match[2] not in units:
        raise ValueError(f'unsupported metric: {value!r}')
    return float(match[1]) * units[match[2]]


def metrics(report):
    iterations = report['iterations']
    native = report['native']
    # Use routine, not the outer bench frame: batched setup/teardown is excluded.
    timing = quantity(entry(native, 'functions_timing')['total'],
                      {'ns': 1, 'µs': 1000, 'us': 1000, 'ms': 1e6, 's': 1e9}) / iterations
    # Cumulative allocation mode includes nested calls. As for instructions,
    # read the routine frame once; summing nested rows would double count.
    allocation = quantity(entry(native, 'functions_alloc')['total'],
                          {'B': 1, 'KB': 1024, 'MB': 1024**2, 'GB': 1024**3, 'TB': 1024**4}) / iterations
    counted = report.get('callgrind')
    instructions = (quantity(entry(counted, 'functions_icount')['total'], {'': 1}) / iterations
                    if counted else None)
    return {'ns_per_op': timing, 'allocated_bytes_per_op': allocation, 'instructions_per_op': instructions}


def combine(native, counted, *, case, target, iterations, provenance):
    validate(native, case['id'], iterations, 'native')
    if counted:
        validate(counted, case['id'], iterations, 'callgrind')
        for field in ('rustc', 'os'):
            if native['meta'][field] != counted['meta'][field]:
                raise ValueError(f'native/Callgrind {field} mismatch')
        if native['version'] != counted['version']:
            raise ValueError('native/Callgrind hotpath version mismatch')
    report = {'schema_version': 1, 'benchmark': case['benchmark'], 'id': case['id'],
              'target': target, 'iterations': iterations, 'provenance': provenance,
              'native': native, 'callgrind': counted}
    report['metrics'] = metrics(report)
    return report


def compatibility(current, baseline):
    if baseline.get('schema_version') != 1:
        return 'baseline report format changed'
    for field in ('schema_version', 'benchmark', 'id', 'target', 'iterations'):
        if current[field] != baseline[field]:
            return f'{field} changed'
    for field in ('fixture_sha256', 'profile', 'features', 'runner_image'):
        if current['provenance'].get(field) != baseline['provenance'].get(field):
            return f'{field} changed'
    for field in ('rustc', 'os'):
        if current['native']['meta'][field] != baseline['native']['meta'][field]:
            return f'{field} changed'
    if current['native']['version'] != baseline['native']['version']:
        return 'hotpath version changed'
    current_tool = (current.get('callgrind') or {}).get('functions_icount', {}).get('tool')
    baseline_tool = (baseline.get('callgrind') or {}).get('functions_icount', {}).get('tool')
    if not current_tool or current_tool != baseline_tool:
        return 'Callgrind unavailable or version changed'
    return None


def cell(value):
    return str(value).replace('|', '&#124;').replace('\n', ' ').replace('`', '')


def summarize(directory, baseline=None):
    lines = ['## Request-path benchmarks', '',
             '| Benchmark | ns/op (diagnostic) | Instructions/op | Allocated B/op | Instruction change | Allocation change |',
             '|---|---:|---:|---:|---|---|']
    reports = sorted(directory.glob('*.json'))
    baseline_revisions = set()
    for path in reports:
        report = json.loads(path.read_text())
        result = metrics(report)
        delta = 'No baseline'
        allocation_delta = 'No baseline'
        if baseline and (baseline / path.name).is_file():
            try:
                previous = json.loads((baseline / path.name).read_text())
                reason = compatibility(report, previous)
                if reason is None:
                    validate(previous['native'], report['id'], report['iterations'], 'native')
                    validate(previous['callgrind'], report['id'], report['iterations'], 'callgrind')
                    previous_metrics = metrics(previous)
                    baseline_revisions.add(previous['provenance'].get('revision', 'unknown'))
            except (ValueError, KeyError, TypeError, AttributeError):
                reason = 'invalid baseline report'
            if reason:
                delta = f'Not comparable: {reason}'
                allocation_delta = delta
            else:
                old = previous_metrics['instructions_per_op']
                delta = f'{(result["instructions_per_op"] / old - 1) * 100:+.2f}%' if old else 'Baseline is zero'
                old_bytes = previous_metrics['allocated_bytes_per_op']
                allocation_delta = f'{(result["allocated_bytes_per_op"] / old_bytes - 1) * 100:+.2f}%' if old_bytes else 'Baseline is zero'
        instructions = result['instructions_per_op']
        instruction_text = f'{instructions:,.2f}' if instructions is not None else 'Native only'
        lines.append(f'| {cell(report["id"])} | {result["ns_per_op"]:.2f} | {instruction_text} | '
                     f'{result["allocated_bytes_per_op"]:.2f} | {cell(delta)} | {cell(allocation_delta)} |')
    if not reports:
        lines.extend(['', 'No completed benchmark reports. Inspect the failed benchmark step.'])
    else:
        first = json.loads(reports[0].read_text())
        lines[2:2] = [f'Current revision: `{cell(first["provenance"].get("revision", "unknown"))}`; '
                      f'profile: `{cell(first["provenance"]["profile"])}`; '
                      f'compiler: `{cell(first["native"]["meta"]["rustc"])}`.', '']
    if baseline_revisions:
        lines.extend(['', 'Compared baseline revision(s): ' + ', '.join(f'`{cell(revision)}`' for revision in sorted(baseline_revisions)) + '.'])
    lines.extend(['', 'Timing includes profiler overhead and runner noise; it is not a regression gate.',
                  'Allocated bytes include nested calls in the measured routine, excluding batched setup/teardown. '
                  'Full per-function data is in the JSON artifacts. Display units may be rounded.',
                  'Instruction changes are informational; changed fixtures/tools are not compared.', ''])
    return '\n'.join(lines)


def run(args):
    suites = [suite for suite in catalog(args.tier) if suite['target'] == args.target]
    if len(suites) != 1:
        raise ValueError(f'target has no selected cases: {args.target}')
    suite = suites[0]
    needs_callgrind = not args.native_only and any(case.get('measurement', 'both') == 'both' for case in suite['cases'])
    if needs_callgrind and (platform.system() != 'Linux' or platform.machine() != 'x86_64'):
        raise ValueError('the Callgrind runner requires Linux x86_64; use --native-only elsewhere')
    if args.iterations is not None and args.iterations <= 0:
        raise ValueError('iterations must be positive')
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    profile = os.environ.get('HOTPATH_PROFILE', 'hotpath')
    recipe = subprocess.check_output(
        ['just', '--dry-run', 'bench-hotpath', suite['cases'][0]['id'], args.target],
        cwd=ROOT, stderr=subprocess.STDOUT, text=True)
    features = re.search(r'--features ([a-z0-9,_-]+)', recipe)
    if features is None or 'hotpath-cloud' in features[1]:
        raise ValueError('bench-hotpath must declare explicit features without hotpath-cloud')
    provenance = {
        'revision': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
        'worktree_dirty': bool(subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT, text=True).strip()),
        'fixture_sha256': hashlib.sha256((ROOT / f'benches/{args.target}.rs').read_bytes()).hexdigest(),
        'lockfile_sha256': hashlib.sha256((ROOT / 'Cargo.lock').read_bytes()).hexdigest(),
        'profile': profile, 'features': features[1],
        'runner_image': {key: os.environ.get(key) for key in ('ImageOS', 'ImageVersion')},
    }
    # Override caller configuration which could sample/truncate or merge a
    # foreign report. No credential or cloud feature is used by the command.
    env = {key: value for key, value in os.environ.items()
           if not key.startswith('HOTPATH_') and key != 'CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER'}
    env.update(HOTPATH_PROFILE=profile,
               HOTPATH_OUTPUT_FORMAT='json', HOTPATH_REPORT='functions-timing,functions-alloc,functions-icount',
               HOTPATH_FUNCTIONS_LIMIT='0', HOTPATH_TIME_SAMPLING_RATE='1', HOTPATH_UPLOAD='0',
               HOTPATH_ALLOC_CUMULATIVE='1', HOTPATH_ALLOC_METRIC='bytes',
               HOTPATH_METRICS_SERVER_OFF='true')
    for case in suite['cases']:
        iterations = args.iterations if args.iterations is not None else case.get('iterations', suite['iterations'])
        env['HOTPATH_CRITERION_ITERATIONS'] = str(iterations)
        print(f'Running {case["id"]} ({iterations} iterations)', flush=True)
        with tempfile.TemporaryDirectory(prefix='oxidns-hotpath-') as temporary:
            work = Path(temporary)
            command = ['just', 'bench-hotpath', case['id'], args.target]
            native_path = work / 'native.json'
            subprocess.run(command, cwd=ROOT, env={**env, 'HOTPATH_OUTPUT_PATH': str(native_path)}, check=True)
            native = json.loads(native_path.read_text())
            validate(native, case['id'], iterations, 'native')
            counted = None
            if not args.native_only and case.get('measurement', 'both') == 'both':
                counted_path = work / 'callgrind.json'
                dump = str(work / 'callgrind.%p')
                subprocess.run(command, cwd=ROOT, check=True, env={**env,
                    'HOTPATH_OUTPUT_PATH': str(counted_path), 'HOTPATH_CALLGRIND_OUT_FILE': dump,
                    'CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER':
                        f'valgrind -q --tool=callgrind --instr-atstart=no --callgrind-out-file={dump}'})
                counted = json.loads(counted_path.read_text())
            report = combine(native, counted, case=case, target=args.target,
                             iterations=iterations, provenance=provenance)
            (output / f'{case["benchmark"]}.json').write_text(json.dumps(report, indent=2) + '\n')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    matrix = sub.add_parser('catalog')
    matrix.add_argument('--tier', choices=['core', 'all'], default='core')
    runner = sub.add_parser('run')
    runner.add_argument('--target', required=True)
    runner.add_argument('--tier', choices=['core', 'all'], default='core')
    runner.add_argument('--output', required=True, type=Path)
    runner.add_argument('--iterations', type=int)
    runner.add_argument('--native-only', action='store_true')
    summary = sub.add_parser('summary')
    summary.add_argument('directory', type=Path)
    summary.add_argument('--baseline', type=Path)
    args = parser.parse_args()
    if args.command == 'catalog':
        print(json.dumps({'include': catalog(args.tier)}, separators=(',', ':')))
    elif args.command == 'run':
        run(args)
    else:
        print(summarize(args.directory, args.baseline))


if __name__ == '__main__':
    main()
