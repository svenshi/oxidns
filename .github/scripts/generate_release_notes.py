#!/usr/bin/env python3
"""Generate or update release notes based on actual git commits or curated notes."""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
from pathlib import Path


def get_git_commits(version: str) -> list[tuple[str, str, str]]:
    """Get list of (short_hash, subject, author) since previous tag."""
    prev_tag = ""
    # Try finding previous tag relative to the current version tag
    try:
        res = subprocess.run(
            ["git", "describe", "--tags", "--abbrev=0", f"{version}^"],
            capture_output=True,
            text=True,
            check=False,
        )
        if res.returncode == 0 and res.stdout.strip():
            prev_tag = res.stdout.strip()
    except Exception:
        pass

    # If not found, try getting latest tag that is not the current version
    if not prev_tag:
        try:
            res = subprocess.run(
                ["git", "tag", "--sort=-creatordate"],
                capture_output=True,
                text=True,
                check=False,
            )
            if res.returncode == 0:
                tags = [t.strip() for t in res.stdout.splitlines() if t.strip()]
                for t in tags:
                    if t != version:
                        prev_tag = t
                        break
        except Exception:
            pass

    cmd = ["git", "log", "--pretty=format:%h%x09%s%x09%an"]
    if prev_tag:
        cmd.append(f"{prev_tag}..HEAD")
    else:
        cmd.extend(["-n", "30"])

    try:
        res = subprocess.run(cmd, capture_output=True, text=True, check=False)
        if res.returncode != 0:
            return []

        commits: list[tuple[str, str, str]] = []
        for line in res.stdout.strip().splitlines():
            line = line.strip()
            if not line:
                continue
            parts = line.split("\t")
            if len(parts) >= 3:
                commits.append((parts[0].strip(), parts[1].strip(), parts[2].strip()))
            elif len(parts) == 2:
                commits.append((parts[0].strip(), parts[1].strip(), ""))
        return commits
    except Exception:
        return []


def categorize_commits(
    commits: list[tuple[str, str, str]],
) -> dict[str, list[tuple[str, str, str]]]:
    """Categorize commits by type."""
    categories: dict[str, list[tuple[str, str, str]]] = {
        "features": [],
        "ui": [],
        "fixes": [],
        "perf": [],
        "chores": [],
        "other": [],
    }

    for chash, subject, author in commits:
        subj_lower = subject.lower()

        # Skip pure version bump tags or repetitive release merge commits
        if re.match(r"^release\s+v\d", subj_lower) or re.match(r"^bump\s+version", subj_lower):
            continue

        if subj_lower.startswith(("feat", "feature")):
            if any(k in subj_lower for k in ("ui", "webui", "glass", "theme", "dashboard")):
                categories["ui"].append((chash, subject, author))
            else:
                categories["features"].append((chash, subject, author))
        elif subj_lower.startswith(("fix", "bug", "patch")):
            categories["fixes"].append((chash, subject, author))
        elif subj_lower.startswith(("ui", "style", "css")):
            categories["ui"].append((chash, subject, author))
        elif subj_lower.startswith(("perf", "refactor", "optimize")):
            categories["perf"].append((chash, subject, author))
        elif subj_lower.startswith(("docs", "chore", "ci", "deps", "test", "build")):
            categories["chores"].append((chash, subject, author))
        else:
            categories["other"].append((chash, subject, author))

    return categories


def build_changelog_section(commits: list[tuple[str, str, str]]) -> str:
    """Format categorized commits into clean markdown sections."""
    if not commits:
        return "- 包含常规稳定性优化与底层依赖更新。"

    cats = categorize_commits(commits)
    sections: list[str] = []

    def format_list(items: list[tuple[str, str, str]]) -> list[str]:
        lines = []
        for chash, subj, author in items:
            author_str = f" (@{author})" if author and author != "oxidns-builder" else ""
            lines.append(f"- `{chash}` {subj}{author_str}")
        return lines

    if cats["features"]:
        sections.append("### 🚀 新特性 / Features\n" + "\n".join(format_list(cats["features"])))
    if cats["ui"]:
        sections.append("### 🎨 界面与交互 / WebUI & Aesthetics\n" + "\n".join(format_list(cats["ui"])))
    if cats["fixes"]:
        sections.append("### 🐛 问题修复 / Bug Fixes\n" + "\n".join(format_list(cats["fixes"])))
    if cats["perf"]:
        sections.append("### ⚡ 性能与重构 / Performance & Refactoring\n" + "\n".join(format_list(cats["perf"])))
    if cats["chores"]:
        sections.append("### 🔧 工程维护 / Maintenance\n" + "\n".join(format_list(cats["chores"])))
    if cats["other"]:
        sections.append("### 📦 其它变更 / Other Changes\n" + "\n".join(format_list(cats["other"])))

    return "\n\n".join(sections) if sections else "- 包含常规稳定性优化与底层依赖更新。"


def extract_existing_version_notes(content: str, version: str) -> str | None:
    """Extract release notes section if the file already defines this version."""
    v_clean = version.lstrip("v")
    patterns = [
        rf"^#+\s*OxiDNS\s+(?:v)?{re.escape(v_clean)}\b.*$",
        rf"^#+\s*(?:v)?{re.escape(v_clean)}\b.*$",
    ]

    lines = content.splitlines()
    start_idx = None
    for idx, line in enumerate(lines):
        for pat in patterns:
            if re.match(pat, line.strip(), re.IGNORECASE):
                start_idx = idx
                break
        if start_idx is not None:
            break

    if start_idx is None:
        return None

    # Find next top-level heading (# ...)
    end_idx = len(lines)
    for idx in range(start_idx + 1, len(lines)):
        if re.match(r"^#\s+[^\s#]", lines[idx]):
            end_idx = idx
            break

    section_lines = lines[start_idx:end_idx]
    return "\n".join(section_lines).strip()


def generate_notes(
    version: str,
    existing_file: Path | None = None,
    custom_notes: str = "",
    custom_title: str = "",
) -> str:
    """Generate final release notes string."""
    v_clean = version if version.startswith("v") else f"v{version}"

    # 1. User supplied custom notes directly via workflow dispatch
    if custom_notes and custom_notes.strip():
        header = f"# OxiDNS {v_clean}"
        body = custom_notes.strip()
        if not body.startswith("#"):
            return f"{header}\n\n{body}\n"
        return f"{body}\n"

    # 2. Check if existing release notes file already has this version
    if existing_file and existing_file.is_file():
        existing_text = existing_file.read_text(encoding="utf-8", errors="ignore")
        existing_section = extract_existing_version_notes(existing_text, v_clean)
        if existing_section:
            return f"{existing_section}\n"

    # 3. Generate dynamically from git commits
    commits = get_git_commits(v_clean)
    changelog = build_changelog_section(commits)

    title_text = custom_title.strip() if custom_title else "更新发布说明"

    notes = [
        f"# OxiDNS {v_clean}",
        "",
        f"## 🚀 {title_text}",
        f"- 本版本包含针对 **OxiDNS** 的最新功能开发、界面增强与稳定性改进。",
        "",
        "## ✨ 实际更新内容 (Changelog)",
        changelog,
        "",
        "## 📦 安装与升级说明",
        "- 保持原有 `config.yaml` 配置文件即可直接平滑启动或升级。",
        "- 根据操作系统平台下载对应的二进制压缩包或 `.deb` 安装包进行替换。",
        "",
    ]

    return "\n".join(notes)


def main() -> None:
    parser = argparse.ArgumentParser(description="Generate release notes for OxiDNS")
    parser.add_argument("--version", required=True, help="Release tag version (e.g. v1.6.8)")
    parser.add_argument("--output", type=Path, default=Path(".github/release-notes.md"), help="Output path")
    parser.add_argument("--custom-notes", default="", help="Custom release notes string")
    parser.add_argument("--title", default="", help="Optional release title")
    args = parser.parse_args()

    output_path = args.output
    notes = generate_notes(
        version=args.version,
        existing_file=output_path if output_path.exists() else None,
        custom_notes=args.custom_notes,
        custom_title=args.title,
    )

    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(notes, encoding="utf-8")
    print(f"Successfully generated release notes for {args.version} -> {output_path}")


if __name__ == "__main__":
    main()
