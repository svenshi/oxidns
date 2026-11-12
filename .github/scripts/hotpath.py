#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Sven Shi
# SPDX-License-Identifier: GPL-3.0-or-later
"""Run and summarize repository-owned hotpath reports; no cloud API or token."""

import argparse
import hashlib
import json
import math
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


def policy():
    # Keep regression thresholds in code, shared by CI and the PR publisher.
    return {
        'instructions_per_op': {'percent': 3, 'absolute': 0},
        'allocated_bytes_per_op': {'percent': 20, 'absolute': 16},
    }


def validate_policy(rules):
    if not isinstance(rules, dict) or set(rules) != {'instructions_per_op', 'allocated_bytes_per_op'}:
        raise ValueError('gate policy must define instruction and allocation thresholds')
    for rule in rules.values():
        if not isinstance(rule, dict) or set(rule) != {'percent', 'absolute'}:
            raise ValueError('each gate threshold needs percent and absolute')
        for value in rule.values():
            if type(value) not in (int, float) or not math.isfinite(value) or value < 0:
                raise ValueError('gate thresholds must be finite nonnegative numbers')
    return rules


def change(current, previous, rule):
    difference = current - previous
    percent = difference / previous * 100 if previous else None
    # Both relative and absolute thresholds must be exceeded. A newly nonzero
    # metric has no finite relative delta and is judged by the absolute floor.
    significant = abs(difference) > rule['absolute'] and (percent is None or abs(percent) > rule['percent'] + 1e-9)
    status = ('regression' if difference > 0 else 'improvement') if significant else 'stable'
    return {'current': current, 'baseline': previous, 'percent': percent, 'status': status}


def assess(directory, baseline=None, rules=None):
    rules = policy() if rules is None else rules
    rows = []
    for path in sorted(directory.glob('*.json')):
        report = json.loads(path.read_text())
        validate(report['native'], report['id'], report['iterations'], 'native')
        if report.get('callgrind'):
            validate(report['callgrind'], report['id'], report['iterations'], 'callgrind')
        result = metrics(report)
        reason = 'No baseline'
        previous_metrics = None
        baseline_revision = None
        if baseline and (baseline / path.name).is_file():
            try:
                previous = json.loads((baseline / path.name).read_text())
                reason = compatibility(report, previous)
                if reason is None:
                    validate(previous['native'], report['id'], report['iterations'], 'native')
                    validate(previous['callgrind'], report['id'], report['iterations'], 'callgrind')
                    previous_metrics = metrics(previous)
                    baseline_revision = previous['provenance'].get('revision')
            except (ValueError, KeyError, TypeError, AttributeError):
                reason = 'invalid baseline report'
        changes = {key: change(result[key], previous_metrics[key], rule)
                   for key, rule in rules.items()} if previous_metrics else {}
        statuses = {item['status'] for item in changes.values()}
        status = ('skipped' if reason else 'regression' if 'regression' in statuses
                  else 'improvement' if 'improvement' in statuses else 'stable')
        rows.append({'id': report['id'], 'status': status, 'reason': reason,
                     'metrics': result, 'changes': changes,
                     'revision': report['provenance'].get('revision'), 'baseline_revision': baseline_revision})
    counts = {status: sum(row['status'] == status for row in rows)
              for status in ('regression', 'improvement', 'stable', 'skipped')}
    return {'schema_version': 1, 'status': 'failed' if counts['regression'] or not rows
            else 'skipped' if counts['skipped'] == len(rows) else 'passed',
            'policy': rules, 'counts': counts, 'rows': rows}


STATUS = {'regression': '🔴 Regression', 'improvement': '🟢 Improvement',
          'stable': '⚪ Within limits', 'skipped': '⚠️ Not compared'}


def delta_text(item):
    if not item:
        return '—'
    difference = item['current'] - item['baseline']
    symbol = {'regression': '🔴 ↑', 'improvement': '🟢 ↓',
              'stable': '⚪ ↑' if difference > 0 else '⚪ ↓' if difference < 0 else '⚪ ='}[item['status']]
    delta = f"{item['percent']:+.2f}%" if item['percent'] is not None else ('new cost' if difference else '0.00%')
    return f'{symbol} {delta}'


def render(assessment):
    counts = assessment['counts']
    title = {'failed': '🔴 Performance gate failed', 'passed': '✅ Performance gate passed',
             'skipped': '⚠️ Performance gate not evaluated'}[assessment['status']]
    lines = ['## ' + title, '',
             f"🔴 **{counts['regression']} regressed** · 🟢 **{counts['improvement']} improved** · "
             f"⚪ **{counts['stable']} stable** · ⚠️ **{counts['skipped']} not compared**", '',
             'Lower instructions and allocated bytes are better. ↑ = more work/memory; ↓ = less.', '',
             '| Result | Benchmark | Instructions/op (base → head) | Change | B/op (base → head) | Change |',
             '|---|---|---:|---|---:|---|']
    order = {'regression': 0, 'improvement': 1, 'skipped': 2, 'stable': 3}
    for row in sorted(assessment['rows'], key=lambda row: (order[row['status']], row['id'])):
        def value(key):
            current = row['metrics'][key]
            text = f'{current:,.2f}' if current is not None else 'Native only'
            previous = row['changes'].get(key)
            return f"{previous['baseline']:,.2f} → {text}" if previous else text
        lines.append(f"| {STATUS[row['status']]} | {cell(row['id'])} | {value('instructions_per_op')} | "
                     f"{delta_text(row['changes'].get('instructions_per_op'))} | {value('allocated_bytes_per_op')} | "
                     f"{delta_text(row['changes'].get('allocated_bytes_per_op'))} |")
    skipped = [row for row in assessment['rows'] if row['reason']]
    if skipped:
        lines.extend(['', '<details><summary>Comparison skipped</summary>', ''])
        lines.extend(f"- {cell(row['id'])}: {cell(row['reason'])}" for row in skipped)
        lines.extend(['', '</details>'])
    lines.extend(['', '<details><summary>Native timing (diagnostic only)</summary>', '',
                  '| Benchmark | ns/op |', '|---|---:|'])
    lines.extend(f"| {cell(row['id'])} | {row['metrics']['ns_per_op']:.2f} |" for row in assessment['rows'])
    lines.extend(['', '</details>', '', 'Gate thresholds (both percent and absolute increases must be exceeded):'])
    for key, rule in assessment['policy'].items():
        lines.append(f"- {key}: >{rule['percent']:g}% and >{rule['absolute']:g}/op.")
    revisions = sorted({row['baseline_revision'] for row in assessment['rows'] if row['baseline_revision']})
    if revisions:
        lines.extend(['', 'Compared baseline revision(s): ' + ', '.join(f'`{cell(value)}`' for value in revisions)])
    lines.extend(['', 'Timing includes profiler/runner noise and is never gated. Allocations include nested calls; '
                  'display units may be rounded. Missing or incompatible baselines are explicitly skipped, not passed.', ''])
    return '\n'.join(lines)


def summarize(directory, baseline=None):
    return render(assess(directory, baseline))


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
    gate = sub.add_parser('gate')
    gate.add_argument('directory', type=Path)
    gate.add_argument('--baseline', type=Path)
    gate.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if args.command == 'catalog':
        print(json.dumps({'include': catalog(args.tier)}, separators=(',', ':')))
    elif args.command == 'run':
        run(args)
    elif args.command == 'gate':
        result = assess(args.directory, args.baseline)
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(result, indent=2) + '\n')
        print(render(result))
        if result['status'] == 'failed':
            raise SystemExit(1)
    else:
        print(summarize(args.directory, args.baseline))


if __name__ == '__main__':
    main()
