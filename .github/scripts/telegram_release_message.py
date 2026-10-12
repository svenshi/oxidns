#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Sven Shi
# SPDX-License-Identifier: GPL-3.0-or-later

"""Render curated GitHub release notes as Telegram-compatible HTML."""

from __future__ import annotations

import argparse
import html
import re
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import urlsplit


TELEGRAM_MESSAGE_LIMIT = 4096
TRUNCATION_NOTICE = "\n\n…（发布说明过长，已裁剪；完整内容请查看 GitHub Release。）"

_HEADING_RE = re.compile(r"^#{1,6}[ \t]+(.+)$")
_LIST_ITEM_RE = re.compile(r"^([ \t]*)[-+*][ \t]+(.+)$")
_LINK_RE = re.compile(r"\[([^]\n]+)]\((https?://[^\s)]+)\)")


def _render_inline(markdown: str) -> str:
    """Render the small inline Markdown subset used by release notes."""
    rendered: list[str] = []
    plain: list[str] = []

    def flush_plain() -> None:
        if plain:
            rendered.append(html.escape("".join(plain), quote=False))
            plain.clear()

    cursor = 0
    while cursor < len(markdown):
        if markdown[cursor] == "`":
            delimiter_end = cursor + 1
            while delimiter_end < len(markdown) and markdown[delimiter_end] == "`":
                delimiter_end += 1
            delimiter = markdown[cursor:delimiter_end]
            closing = markdown.find(delimiter, delimiter_end)
            if closing != -1:
                flush_plain()
                code = markdown[delimiter_end:closing]
                rendered.append(f"<code>{html.escape(code, quote=False)}</code>")
                cursor = closing + len(delimiter)
                continue

        if markdown.startswith("**", cursor):
            closing = markdown.find("**", cursor + 2)
            if closing != -1:
                flush_plain()
                content = markdown[cursor + 2 : closing]
                rendered.append(f"<b>{_render_inline(content)}</b>")
                cursor = closing + 2
                continue

        if markdown[cursor] == "[":
            match = _LINK_RE.match(markdown, cursor)
            if match is not None:
                flush_plain()
                label, url = match.groups()
                rendered.append(
                    f'<a href="{html.escape(url, quote=True)}">'
                    f"{_render_inline(label)}</a>"
                )
                cursor = match.end()
                continue

        plain.append(markdown[cursor])
        cursor += 1

    flush_plain()
    return "".join(rendered)


def render_markdown(markdown: str) -> str:
    """Convert release-note Markdown into Telegram's supported HTML subset."""
    rendered_lines: list[str] = []
    for line in markdown.rstrip("\n").splitlines():
        heading = _HEADING_RE.match(line)
        if heading is not None:
            rendered_lines.append(f"<b>{_render_inline(heading.group(1))}</b>")
            continue

        list_item = _LIST_ITEM_RE.match(line)
        if list_item is not None:
            indentation, content = list_item.groups()
            rendered_lines.append(f"{indentation}• {_render_inline(content)}")
            continue

        rendered_lines.append(_render_inline(line))

    return "\n".join(rendered_lines)


class _VisibleTextParser(HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.parts: list[str] = []

    def handle_data(self, data: str) -> None:
        self.parts.append(data)


def visible_text(rendered_html: str) -> str:
    parser = _VisibleTextParser()
    parser.feed(rendered_html)
    parser.close()
    return "".join(parser.parts)


def _utf16_length(text: str) -> int:
    # Telegram measures message/entity offsets in UTF-16 code units.
    return len(text.encode("utf-16-le")) // 2


def _truncate_utf16(text: str, limit: int) -> str:
    length = 0
    end = 0
    for end, character in enumerate(text, start=1):
        next_length = length + _utf16_length(character)
        if next_length > limit:
            return text[: end - 1]
        length = next_length
    return text[:end]


def _release_link(release_url: str) -> str:
    parsed = urlsplit(release_url)
    if parsed.scheme not in {"http", "https"} or not parsed.netloc:
        raise ValueError("release URL must be an absolute HTTP(S) URL")

    escaped_url = html.escape(release_url, quote=True)
    label = html.escape(release_url, quote=False)
    return f'\n\n👉 GitHub Release：<a href="{escaped_url}">{label}</a>'


def build_message(
    release_notes: str,
    release_url: str,
    *,
    limit: int = TELEGRAM_MESSAGE_LIMIT,
) -> str:
    """Build a valid Telegram HTML message within the configured limit."""
    release_notes = release_notes.rstrip("\n")
    release_link = _release_link(release_url)
    message = f"{render_markdown(release_notes)}{release_link}"
    if _utf16_length(visible_text(message)) <= limit:
        return message

    reserved_length = _utf16_length(visible_text(TRUNCATION_NOTICE + release_link))
    available_length = limit - reserved_length
    if available_length <= 0:
        raise ValueError("message limit is too small for the release link")

    # Markdown delimiters are visible or removed during rendering, so limiting
    # the source prefix is conservative and cannot split an emitted HTML tag.
    truncated_notes = _truncate_utf16(release_notes, available_length).rstrip()
    message = f"{render_markdown(truncated_notes)}{TRUNCATION_NOTICE}{release_link}"
    while _utf16_length(visible_text(message)) > limit:
        truncated_notes = truncated_notes[:-1].rstrip()
        message = f"{render_markdown(truncated_notes)}{TRUNCATION_NOTICE}{release_link}"
    return message


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("release_notes", type=Path)
    parser.add_argument("release_url")
    args = parser.parse_args()

    release_notes = args.release_notes.read_text(encoding="utf-8")
    print(build_message(release_notes, args.release_url))


if __name__ == "__main__":
    main()
