# SPDX-FileCopyrightText: 2026 Sven Shi
# SPDX-License-Identifier: GPL-3.0-or-later

import copy
import json
from pathlib import Path
import tempfile
import unittest

import hotpath


def raw(kind='native', iterations=10):
    routine = {'name': hotpath.ROUTINE, 'calls': iterations, 'sampled_calls': iterations,
               'total': '1.25 µs'}
    return {
        'version': 'test-version', 'meta': {'run': kind, 'rustc': 'test-rustc', 'os': 'linux-x86_64'},
        'user_metadata': {'criterion_benchmark': 'group/test', 'criterion_iterations': str(iterations)},
        'functions_timing': {'data': [
            {'name': 'hotpath::criterion::bench', 'calls': 1, 'total': '90 s'}, routine]},
        'functions_alloc': {'description': 'Cumulative allocation bytes during each function call (including nested calls).',
                            'data': [{**routine, 'total': '2 KB'}]},
        'functions_icount': {'tool': 'callgrind-3.26.0', 'data': [{**routine, 'total': '12,345'}]},
    }


def combined():
    return hotpath.combine(raw(), raw('callgrind'), case={'benchmark': 'case', 'id': 'group/test'},
                           target='suite', iterations=10,
                           provenance={'fixture_sha256': 'fixture', 'profile': 'hotpath', 'features': 'test'})


class ReportTest(unittest.TestCase):
    def test_normalize_routine_not_outer_bench_or_rounded_average(self):
        result = hotpath.metrics(combined())
        self.assertEqual(result['ns_per_op'], 125)
        self.assertEqual(result['allocated_bytes_per_op'], 204.8)
        self.assertEqual(result['instructions_per_op'], 1234.5)

    def test_reject_stale_wrong_and_sampled_reports(self):
        for metadata in [
            {'criterion_benchmark': 'wrong', 'criterion_iterations': '10'},
            {'criterion_benchmark': 'group/test', 'criterion_iterations': '9'},
        ]:
            report = raw()
            report['user_metadata'] = metadata
            with self.assertRaises(ValueError):
                hotpath.validate(report, 'group/test', 10, 'native')
        report = raw()
        report['functions_timing']['data'][1]['sampled_calls'] = 1
        with self.assertRaises(ValueError):
            hotpath.validate(report, 'group/test', 10, 'native')

    def test_missing_or_duplicate_routine_is_an_error(self):
        for rows in ([], [raw()['functions_alloc']['data'][0]] * 2):
            report = raw()
            report['functions_alloc']['data'] = rows
            with self.assertRaises(ValueError):
                hotpath.validate(report, 'group/test', 10, 'native')

    def test_exclusive_allocation_is_not_misreported_as_total(self):
        report = raw()
        report['functions_alloc']['description'] = 'Exclusive allocation bytes by each function'
        with self.assertRaises(ValueError):
            hotpath.validate(report, 'group/test', 10, 'native')

    def test_old_cloud_artifact_is_not_a_comparison_baseline(self):
        self.assertEqual(hotpath.compatibility(combined(), raw()), 'baseline report format changed')

    def test_broken_baseline_does_not_hide_current_results(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            current, baseline = directory / 'head', directory / 'base'
            current.mkdir()
            baseline.mkdir()
            (current / 'case.json').write_text(json.dumps(combined()))
            (baseline / 'case.json').write_text('{truncated')
            summary = hotpath.summarize(current, baseline)
            self.assertIn('invalid baseline report', summary)
            self.assertIn('125.00', summary)
            broken = combined()
            broken['native']['functions_timing']['data'][1]['total'] = 'invalid unit'
            (baseline / 'case.json').write_text(json.dumps(broken))
            self.assertIn('invalid baseline report', hotpath.summarize(current, baseline))

    def test_reject_mismatched_native_and_callgrind_builds(self):
        for field in ['rustc', 'os']:
            counted = raw('callgrind')
            counted['meta'][field] = 'other'
            with self.assertRaises(ValueError):
                hotpath.combine(raw(), counted, case={'id': 'group/test', 'benchmark': 'case'},
                                target='suite', iterations=10, provenance={})

    def test_native_only_has_no_fabricated_instruction_count(self):
        report = combined()
        report['callgrind'] = None
        self.assertIsNone(hotpath.metrics(report)['instructions_per_op'])
        self.assertIsNotNone(hotpath.compatibility(report, combined()))

    def test_changed_fixture_tool_or_iterations_is_not_comparable(self):
        for group, field in [('provenance', 'fixture_sha256'), ('provenance', 'profile'),
                             ('provenance', 'features'), ('native', 'version')]:
            current = combined()
            current[group][field] = 'changed'
            self.assertIsNotNone(hotpath.compatibility(current, combined()))
        current = combined()
        current['iterations'] = 20
        self.assertIsNotNone(hotpath.compatibility(current, combined()))
        current = combined()
        current['callgrind']['functions_icount']['tool'] = 'callgrind-other'
        self.assertIsNotNone(hotpath.compatibility(current, combined()))

    def test_summary_compares_instructions_and_escapes_table_cells(self):
        with tempfile.TemporaryDirectory() as temporary:
            current, baseline = Path(temporary) / 'head', Path(temporary) / 'base'
            current.mkdir()
            baseline.mkdir()
            previous = combined()
            changed = copy.deepcopy(previous)
            changed['callgrind']['functions_icount']['data'][0]['total'] = '24,690'
            (baseline / 'case.json').write_text(json.dumps(previous))
            (current / 'case.json').write_text(json.dumps(changed))
            summary = hotpath.summarize(current, baseline)
            self.assertIn('+100.00%', summary)
            self.assertIn('125.00', summary)
            changed['provenance']['fixture_sha256'] = 'changed'
            (current / 'case.json').write_text(json.dumps(changed))
            self.assertIn('Not comparable: fixture_sha256 changed', hotpath.summarize(current, baseline))
        self.assertEqual(hotpath.cell('x|y\nz'), 'x&#124;y z')

    def test_invalid_units_are_rejected(self):
        for value in ('nan', '-1 B', '2 fortnight', 'inf ns'):
            with self.assertRaises(ValueError):
                hotpath.quantity(value, {'B': 1, 'ns': 1})


class CatalogTest(unittest.TestCase):
    def test_core_is_nonempty_subset_and_all_cases_are_unique(self):
        core = hotpath.catalog()
        all_cases = hotpath.catalog('all')
        core_ids = {case['id'] for suite in core for case in suite['cases']}
        all_ids = {case['id'] for suite in all_cases for case in suite['cases']}
        self.assertTrue(core_ids < all_ids)

    def test_reject_unsafe_names_duplicates_and_invalid_iteration_budgets(self):
        catalog = [{'target': 'suite', 'iterations': 10, 'cases': [
            {'benchmark': 'case', 'id': 'group/test', 'tier': 'core'}]}]
        broken = []
        for field, value in [('benchmark', '../case'), ('id', 'group/"unsafe"'),
                             ('iterations', 0), ('tier', 'unknown'), ('measurement', 'unknown')]:
            item = copy.deepcopy(catalog)
            item[0]['cases'][0][field] = value
            broken.append(item)
        item = copy.deepcopy(catalog)
        item[0]['cases'].append(copy.deepcopy(item[0]['cases'][0]))
        broken.append(item)
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'catalog.json'
            for item in broken:
                path.write_text(json.dumps(item))
                with self.assertRaises(ValueError):
                    hotpath.catalog(path=path)


if __name__ == '__main__':
    unittest.main()
