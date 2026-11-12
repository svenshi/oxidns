# SPDX-FileCopyrightText: 2026 Sven Shi
# SPDX-License-Identifier: GPL-3.0-or-later

import copy
import io
import json
import unittest
import urllib.request
import zipfile

import hotpath
import hotpath_comment as comment


def archive(identifier='group/test', baseline=100, current=110):
    result = {'schema_version': 1, 'policy': hotpath.policy(), 'rows': [{
        'id': identifier, 'metrics': {'ns_per_op': 123, 'instructions_per_op': current,
                                    'allocated_bytes_per_op': 100},
        'changes': {'instructions_per_op': {'baseline': baseline},
                    'allocated_bytes_per_op': {'baseline': 100}},
        'status': 'stable', 'reason': '@everyone untrusted Markdown',
        'baseline_revision': '@everyone'}]}
    output = io.BytesIO()
    with zipfile.ZipFile(output, 'w') as bundle:
        bundle.writestr('gate/result.json', json.dumps(result))
        bundle.writestr('../../danger.py', 'raise Exception("must never run")')
    return output.getvalue()


class FakeGitHub:
    def __init__(self):
        self.run = {'id': 20, 'event': 'pull_request', 'conclusion': 'success', 'run_attempt': 1,
                    'head_sha': 'a' * 40, 'head_branch': 'feature',
                    'head_repository': {'full_name': 'fork/repo', 'owner': {'login': 'fork'}},
                    'pull_requests': []}
        self.pr = {'number': 7, 'state': 'open', 'head': {'sha': 'a' * 40, 'repo': {'full_name': 'fork/repo'}},
                   'base': {'repo': {'full_name': 'owner/repo'}}}
        self.comments = []
        self.runs = [self.run]
        self.writes = []
        self.payload = archive()
        self.head_reads = 0
        self.change_head = False

    def request(self, path, method='GET', data=None, binary=False):
        if method != 'GET':
            self.writes.append((path, method, data))
            return {}
        if '/pulls/7' in path:
            self.head_reads += 1
            pr = copy.deepcopy(self.pr)
            if self.change_head and self.head_reads > 1:
                pr['head']['sha'] = 'b' * 40
            return pr
        if path.endswith('/actions/runs/20'):
            return self.run
        if path.endswith('/zip'):
            return self.payload
        raise AssertionError(path)

    def pages(self, path, key=None):
        if '/pulls?' in path:
            return iter([{'number': 7}])
        if '/workflows/' in path:
            return iter(self.runs)
        if path.endswith('/artifacts'):
            return iter([{'name': 'hotpath-suite', 'id': 1, 'size_in_bytes': 100, 'expired': False}])
        if path.endswith('/comments'):
            return iter(self.comments)
        raise AssertionError(path)


class CommentTest(unittest.TestCase):
    def test_data_only_archive_recomputes_verdict_and_does_not_extract_paths(self):
        rows, _ = comment.artifact_rows(archive())
        self.assertEqual(rows[0]['status'], 'regression')
        self.assertIsNone(rows[0]['reason'])
        self.assertIsNone(rows[0]['baseline_revision'])
        for identifier in ('@everyone', 'x|fake', '<script>', 'x\nhello'):
            with self.assertRaises(ValueError):
                comment.artifact_rows(archive(identifier))

    def test_fork_pr_is_resolved_through_api_and_comment_created(self):
        api = FakeGitHub()
        self.assertTrue(comment.publish(api, 'owner/repo', api.run))
        path, method, data = api.writes[0]
        self.assertEqual((path, method), ('/repos/owner/repo/issues/7/comments', 'POST'))
        self.assertIn('🔴 Performance gate failed', data['body'])
        self.assertNotIn('@everyone', data['body'])
        self.assertIn('Full reports and logs', data['body'])

    def test_update_existing_comment_and_clear_previous_failure(self):
        api = FakeGitHub()
        api.payload = archive(current=80)
        api.comments = [{'id': 123, 'user': {'login': 'github-actions[bot]'},
                         'body': comment.MARKER + '\n<!-- run:19 attempt:1 -->'}]
        comment.publish(api, 'owner/repo', api.run)
        self.assertEqual(api.writes[0][1], 'PATCH')
        self.assertIn('✅ Performance gate passed', api.writes[0][2]['body'])
        self.assertIn('🟢 ↓ -20.00%', api.writes[0][2]['body'])

    def test_stale_sha_newer_run_and_newer_attempt_do_not_post(self):
        api = FakeGitHub()
        api.pr['head']['sha'] = 'b' * 40
        self.assertFalse(comment.publish(api, 'owner/repo', api.run))
        api = FakeGitHub()
        api.runs.append({**api.run, 'id': 21})
        self.assertFalse(comment.publish(api, 'owner/repo', api.run))
        api = FakeGitHub()
        event = {**api.run, 'run_attempt': 1}
        api.run['run_attempt'] = 2
        self.assertFalse(comment.publish(api, 'owner/repo', event))
        self.assertEqual(api.writes, [])

    def test_head_changes_before_write_do_not_post(self):
        api = FakeGitHub()
        api.change_head = True
        self.assertFalse(comment.publish(api, 'owner/repo', api.run))
        self.assertEqual(api.writes, [])

    def test_bad_artifact_still_posts_workflow_failure(self):
        api = FakeGitHub()
        api.payload = b'not a zip'
        comment.publish(api, 'owner/repo', api.run)
        self.assertIn('🔴 Performance gate failed', api.writes[0][2]['body'])
        self.assertIn('reports are incomplete', api.writes[0][2]['body'])

    def test_cross_host_redirect_drops_token(self):
        request = urllib.request.Request('https://api.github.com/artifact', headers={'Authorization': 'Bearer secret'})
        redirected = comment.SafeRedirect().redirect_request(request, None, 302, 'Found', {}, 'https://storage.example/report')
        self.assertFalse(redirected.has_header('Authorization'))
        with self.assertRaises(ValueError):
            comment.SafeRedirect().redirect_request(request, None, 302, 'Found', {}, 'http://storage.example/report')


if __name__ == '__main__':
    unittest.main()
