#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Sven Shi
# SPDX-License-Identifier: GPL-3.0-or-later
"""Publish data-only benchmark artifacts using trusted default-branch code."""

import io
import json
import math
import os
from pathlib import Path
import re
import urllib.parse
import urllib.request
import zipfile

import hotpath

MARKER = '<!-- oxidns-hotpath -->'
MAX_ARCHIVE = 64 * 1024 * 1024


class SafeRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, msg, headers, url):
        if urllib.parse.urlsplit(url).scheme != 'https':
            raise ValueError('refusing insecure artifact redirect')
        redirected = super().redirect_request(request, fp, code, msg, headers, url)
        if urllib.parse.urlsplit(request.full_url).netloc != urllib.parse.urlsplit(url).netloc:
            redirected.remove_header('Authorization')
        return redirected


class GitHub:
    def __init__(self):
        self.base = os.environ.get('GITHUB_API_URL', 'https://api.github.com')
        self.token = os.environ['GH_TOKEN']

    def request(self, path, method='GET', data=None, binary=False):
        request = urllib.request.Request(self.base + path, method=method,
            data=json.dumps(data).encode() if data is not None else None,
            headers={'Authorization': f'Bearer {self.token}', 'Accept': 'application/vnd.github+json',
                     'Content-Type': 'application/json', 'X-GitHub-Api-Version': '2022-11-28'})
        with urllib.request.build_opener(SafeRedirect()).open(request, timeout=60) as response:
            payload = response.read(MAX_ARCHIVE + 1)
        if len(payload) > MAX_ARCHIVE:
            raise ValueError('GitHub response exceeds size limit')
        return payload if binary else json.loads(payload)

    def pages(self, path, key=None):
        page = 1
        while True:
            response = self.request(path + ('&' if '?' in path else '?') + f'per_page=100&page={page}')
            items = response[key] if key else response
            yield from items
            if len(items) < 100:
                break
            page += 1


def artifact_rows(archive):
    # Never extract archives, execute report contents, or reuse artifact Markdown.
    with zipfile.ZipFile(io.BytesIO(archive)) as bundle:
        members = [item for item in bundle.infolist() if item.filename == 'gate/result.json']
        if len(members) != 1 or members[0].file_size > 2 * 1024 * 1024:
            raise ValueError('missing or oversized gate result')
        with bundle.open(members[0]) as source:
            payload = source.read(2 * 1024 * 1024 + 1)
        if len(payload) > 2 * 1024 * 1024:
            raise ValueError('oversized gate result')
        result = json.loads(payload)
    if not isinstance(result, dict) or result.get('schema_version') != 1 or not isinstance(result.get('rows'), list) or len(result['rows']) > 200:
        raise ValueError('invalid gate result')
    rules = hotpath.validate_policy(result['policy'])
    rows = []
    for row in result['rows']:
        if not isinstance(row, dict) or not isinstance(row.get('metrics'), dict) or not isinstance(row.get('changes'), dict):
            raise ValueError('invalid metric row')
        if not re.fullmatch(r'[a-z][a-z0-9_./-]{0,200}', row['id']):
            raise ValueError('invalid benchmark ID')
        metrics = row['metrics']
        for key in ('ns_per_op', 'instructions_per_op', 'allocated_bytes_per_op'):
            value = metrics[key]
            if value is not None and (type(value) not in (int, float) or not math.isfinite(value) or not 0 <= value <= 1e18):
                raise ValueError('invalid metric')
        if metrics['ns_per_op'] is None or metrics['allocated_bytes_per_op'] is None:
            raise ValueError('missing native metric')
        changes = {}
        # Recompute statuses using validated policy; never print arbitrary text
        # or mentions supplied by code running in an untrusted PR job.
        for key, rule in rules.items():
            if key in row['changes']:
                previous = row['changes'][key]['baseline']
                if type(previous) not in (int, float) or not math.isfinite(previous) or not 0 <= previous <= 1e18 or metrics[key] is None:
                    raise ValueError('invalid baseline metric')
                changes[key] = hotpath.change(metrics[key], previous, rule)
        comparable = len(changes) == len(rules)
        statuses = {item['status'] for item in changes.values()}
        status = ('skipped' if not comparable else 'regression' if 'regression' in statuses
                  else 'improvement' if 'improvement' in statuses else 'stable')
        revision = row.get('baseline_revision')
        revision = revision if isinstance(revision, str) and re.fullmatch(r'[0-9a-f]{40}', revision) else None
        rows.append({'id': row['id'], 'metrics': metrics, 'changes': changes if comparable else {},
                     'status': status, 'reason': None if comparable else 'Baseline unavailable or incompatible',
                     'baseline_revision': revision})
    return rows, rules


def select_pr(api, repo, run):
    candidates = run.get('pull_requests', [])
    if not candidates:
        head = f"{run['head_repository']['owner']['login']}:{run['head_branch']}"
        candidates = api.pages(f'/repos/{repo}/pulls?state=open&head=' + urllib.parse.quote(head, safe=''))
    for candidate in candidates:
        pr = api.request(f"/repos/{repo}/pulls/{candidate['number']}")
        if (pr['state'] == 'open' and pr['head']['sha'] == run['head_sha']
                and pr['head']['repo'] and pr['head']['repo']['full_name'] == run['head_repository']['full_name']
                and pr['base']['repo']['full_name'] == repo):
            return pr
    return None


def publish(api, repo, run):
    if run['event'] != 'pull_request' or run['conclusion'] == 'cancelled' or not run.get('head_repository'):
        return False
    pr = select_pr(api, repo, run)
    if not pr:
        return False
    latest = api.request(f"/repos/{repo}/actions/runs/{run['id']}")
    if latest['run_attempt'] != run['run_attempt']:
        return False
    runs = api.pages(f'/repos/{repo}/actions/workflows/hotpath.yml/runs?event=pull_request&head_sha=' + run['head_sha'], 'workflow_runs')
    if any(item['id'] > run['id'] and item['head_sha'] == run['head_sha']
           and item['head_repository']['full_name'] == run['head_repository']['full_name'] for item in runs):
        return False
    rules = None
    rows, problems = [], 0
    for artifact in api.pages(f"/repos/{repo}/actions/runs/{run['id']}/artifacts", 'artifacts'):
        if not re.fullmatch(r'hotpath-[a-z][a-z0-9_]*', artifact['name']):
            continue
        if artifact.get('expired') or artifact['size_in_bytes'] > MAX_ARCHIVE:
            problems += 1
            continue
        try:
            payload = api.request(f"/repos/{repo}/actions/artifacts/{artifact['id']}/zip", binary=True)
            artifact_result, artifact_policy = artifact_rows(payload)
            if rules is not None and rules != artifact_policy:
                raise ValueError('benchmark artifacts have conflicting policies')
            rules = artifact_policy
            rows.extend(artifact_result)
        except (ValueError, KeyError, TypeError, OSError, RecursionError, zipfile.BadZipFile):
            problems += 1
    if len({row['id'] for row in rows}) != len(rows):
        raise ValueError('duplicate benchmark reports')
    counts = {status: sum(row['status'] == status for row in rows)
              for status in ('regression', 'improvement', 'stable', 'skipped')}
    failed = run['conclusion'] != 'success' or problems or not rows or counts['regression']
    result = {'status': 'failed' if failed else 'skipped' if counts['skipped'] == len(rows) else 'passed',
              'counts': counts, 'rows': rows, 'policy': rules or hotpath.policy()}
    link = f"{os.environ.get('GITHUB_SERVER_URL', 'https://github.com')}/{repo}/actions/runs/{run['id']}"
    body = f"{MARKER}\n<!-- run:{run['id']} attempt:{run['run_attempt']} -->\n" + hotpath.render(result)
    body += f"\nCommit: `{run['head_sha']}` · [Full reports and logs]({link})\n"
    if failed:
        body += '\n🔴 Workflow failed or reports are incomplete. Inspect the linked logs before merging.\n'
    if len(body) > 60000:
        body = body[:59000] + f'\n\nDetails truncated; [view the complete workflow report]({link}).\n'
    # Re-check the head immediately before writing; a newer commit invalidates this report.
    if api.request(f"/repos/{repo}/pulls/{pr['number']}")['head']['sha'] != run['head_sha']:
        return False
    comments = api.pages(f"/repos/{repo}/issues/{pr['number']}/comments")
    existing = next((item for item in comments if item['user']['login'] == 'github-actions[bot]'
                     and item['body'].startswith(MARKER)), None)
    if existing:
        stamp = re.search(r'<!-- run:(\d+) attempt:(\d+) -->', existing['body'])
        if stamp and (int(stamp[1]), int(stamp[2])) > (run['id'], run['run_attempt']):
            return False
        api.request(f"/repos/{repo}/issues/comments/{existing['id']}", 'PATCH', {'body': body})
    else:
        api.request(f"/repos/{repo}/issues/{pr['number']}/comments", 'POST', {'body': body})
    return True


if __name__ == '__main__':
    event = json.loads(Path(os.environ['GITHUB_EVENT_PATH']).read_text())
    publish(GitHub(), os.environ['GITHUB_REPOSITORY'], event['workflow_run'])
