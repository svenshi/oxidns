# SPDX-FileCopyrightText: 2026 Sven Shi
# SPDX-License-Identifier: GPL-3.0-or-later

from __future__ import annotations

import importlib.util
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).with_name("telegram_release_message.py")
SPEC = importlib.util.spec_from_file_location("telegram_release_message", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
telegram = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(telegram)


class TelegramReleaseMessageTest(unittest.TestCase):
    def test_renders_release_markdown_as_supported_html(self) -> None:
        notes = """# OxiDNS v1.5.1

## 🚀 发布概览

- 支持 `normal` 和 <配置文件>。
- **重要：** 查看 [升级说明](https://example.com/docs)。
"""

        message = telegram.build_message(notes, "https://example.com/releases/v1.5.1")

        self.assertIn("<b>OxiDNS v1.5.1</b>", message)
        self.assertIn("<b>🚀 发布概览</b>", message)
        self.assertIn("• 支持 <code>normal</code> 和 &lt;配置文件&gt;。", message)
        self.assertIn("• <b>重要：</b>", message)
        self.assertIn('<a href="https://example.com/docs">升级说明</a>', message)
        self.assertNotIn("# OxiDNS", telegram.visible_text(message))

    def test_truncates_by_telegram_visible_length_without_breaking_html(self) -> None:
        notes = "# OxiDNS v1.5.1\n\n- " + ("🚀发布内容" * 1_000)

        message = telegram.build_message(
            notes,
            "https://example.com/releases/v1.5.1",
            limit=256,
        )

        self.assertLessEqual(telegram._utf16_length(telegram.visible_text(message)), 256)
        self.assertIn("发布说明过长，已裁剪", message)
        self.assertTrue(message.endswith("</a>"))
        parser = telegram._VisibleTextParser()
        parser.feed(message)
        parser.close()

    def test_rejects_non_http_release_url(self) -> None:
        with self.assertRaisesRegex(ValueError, "absolute HTTP\\(S\\) URL"):
            telegram.build_message("# OxiDNS v1.5.1", "javascript:alert(1)")


if __name__ == "__main__":
    unittest.main()
