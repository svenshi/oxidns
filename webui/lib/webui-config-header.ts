export type WebUiMode = "expert" | "standard";

export interface WebUiConfigHeader {
  mode: WebUiMode;
  modeHeaderPresent: boolean;
}

const PREFIX = "oxidns-webui.";
const HEADER_LINE_RE = /^#\s*oxidns-webui\.([^:]+):\s*(.*)$/;
const MANAGED_LINE_RE = /^#\s*oxidns-webui\.[^:]+:\s*.*$/;

function splitLines(text: string): string[] {
  return text.length > 0 ? text.replace(/\r\n/g, "\n").split("\n") : [];
}

function leadingCommentBlockEnd(lines: string[]): number {
  let index = 0;
  while (index < lines.length) {
    const trimmed = lines[index]?.trim() ?? "";
    if (trimmed === "" || trimmed.startsWith("#")) {
      index += 1;
      continue;
    }
    break;
  }
  return index;
}

function normalizeMode(value: string | undefined): WebUiMode {
  return value === "standard" ? "standard" : "expert";
}

export function parseWebUiConfigHeader(text: string): WebUiConfigHeader {
  const lines = splitLines(text);
  const headerEnd = leadingCommentBlockEnd(lines);
  let modeValue: string | undefined;

  for (let index = 0; index < headerEnd; index += 1) {
    const line = lines[index] ?? "";
    const match = line.match(HEADER_LINE_RE);
    if (match?.[1] === "mode") {
      modeValue = match[2]?.trim();
    }
  }

  return {
    mode: normalizeMode(modeValue),
    modeHeaderPresent: modeValue === "expert" || modeValue === "standard",
  };
}

export function stripWebUiConfigHeader(text: string): string {
  const lines = splitLines(text);
  const cleaned = lines.filter((line) => !MANAGED_LINE_RE.test(line));
  return cleaned.join("\n");
}

function renderHeaderLines(header: WebUiConfigHeader): string[] {
  return [`# ${PREFIX}mode: ${header.mode}`];
}

export function writeWebUiConfigHeader(
  text: string,
  header: WebUiConfigHeader,
): string {
  const withoutManagedHeader = stripWebUiConfigHeader(text);
  const lines = splitLines(withoutManagedHeader);
  const rendered = renderHeaderLines({
    ...header,
    modeHeaderPresent: true,
  });

  if (lines.length === 0) {
    return rendered.join("\n");
  }

  const hasExistingTopComment =
    lines[0]?.trim().startsWith("#") || lines[0]?.trim() === "";
  const separator =
    hasExistingTopComment && lines[0]?.trim() !== "" ? [""] : [];
  return [...rendered, ...separator, ...lines].join("\n");
}

export function hasWebUiManagedHeader(text: string): boolean {
  return parseWebUiConfigHeader(text).modeHeaderPresent;
}
