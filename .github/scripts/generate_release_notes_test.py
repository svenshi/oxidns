from __future__ import annotations

import importlib.util
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).with_name("generate_release_notes.py")
SPEC = importlib.util.spec_from_file_location("generate_release_notes", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
generator = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(generator)


class GenerateReleaseNotesTest(unittest.TestCase):
    def test_categorize_commits(self) -> None:
        commits = [
            ("abc1234", "feat(ui): add liquid glass neon progress bars", "author1"),
            ("def5678", "fix(server): resolve connection reset bug", "author2"),
            ("789abcd", "perf(cache): optimize cache lookup latency", "author3"),
            ("1112223", "chore(ci): update release workflow", "author4"),
            ("4445556", "update readme documentation", "author5"),
        ]

        cats = generator.categorize_commits(commits)
        self.assertEqual(len(cats["ui"]), 1)
        self.assertEqual(cats["ui"][0][0], "abc1234")
        self.assertEqual(len(cats["fixes"]), 1)
        self.assertEqual(cats["fixes"][0][0], "def5678")
        self.assertEqual(len(cats["perf"]), 1)
        self.assertEqual(len(cats["chores"]), 1)
        self.assertEqual(len(cats["other"]), 1)

    def test_build_changelog_section(self) -> None:
        commits = [
            ("a1b2c3d", "feat(ui): support mobile view", "author1"),
            ("e4f5a6b", "fix: correct dns timestamp", "author2"),
        ]
        markdown = generator.build_changelog_section(commits)
        self.assertIn("### 🎨 界面与交互 / WebUI & Aesthetics", markdown)
        self.assertIn("`a1b2c3d` feat(ui): support mobile view", markdown)
        self.assertIn("### 🐛 问题修复 / Bug Fixes", markdown)
        self.assertIn("`e4f5a6b` fix: correct dns timestamp", markdown)

    def test_custom_notes_priority(self) -> None:
        custom = "## 🌟 Custom Release\n- Unique feature here"
        notes = generator.generate_notes(
            version="v1.6.8",
            custom_notes=custom,
        )
        self.assertIn("# OxiDNS v1.6.8", notes)
        self.assertIn("- Unique feature here", notes)

    def test_extract_existing_version_notes(self) -> None:
        content = """# OxiDNS v1.6.8

## 🚀 发布概览
- 特性A
- 特性B

# OxiDNS v1.6.1

## 历史版本
- 旧说明
"""
        extracted = generator.extract_existing_version_notes(content, "v1.6.8")
        self.assertIsNotNone(extracted)
        assert extracted is not None
        self.assertIn("特性A", extracted)
        self.assertNotIn("旧说明", extracted)


if __name__ == "__main__":
    unittest.main()
