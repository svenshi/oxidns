// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Replayable, line-oriented text sources.
//!
//! This module owns physical text I/O only. It preserves input order and raw
//! line contents, attaches a structured location, and computes non-destructive
//! lexical annotations. Consumers remain responsible for deciding whether a
//! line is a comment, a rule, an error, or something to skip.

use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io::{self, BufRead, BufReader, Seek};
use std::path::PathBuf;

use super::FingerprintReader;

const TEXT_READER_CAPACITY: usize = 256 * 1024;

/// A replayable sequence of inline text followed by zero or more files.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TextSource<'a> {
    inline_field: &'a str,
    inline: &'a [String],
    files: &'a [String],
}

impl<'a> TextSource<'a> {
    pub(crate) const fn new(
        inline_field: &'a str,
        inline: &'a [String],
        files: &'a [String],
    ) -> Self {
        Self {
            inline_field,
            inline,
            files,
        }
    }

    /// Open a replay session whose scans are guaranteed to use identical input.
    ///
    /// A single source file is kept open and rewound between scans. Multi-file
    /// sources are reopened one at a time and verified against fingerprints
    /// captured by the first scan, bounding descriptor use without retaining
    /// file contents. A changed file aborts the candidate build.
    #[allow(dead_code)]
    pub(crate) fn open_replay(
        &self,
    ) -> Result<TextSourceSession<'a>, TextScanError<std::convert::Infallible>> {
        let paths = self
            .files
            .iter()
            .map(String::as_str)
            .filter(|path| !path.trim().is_empty())
            .collect::<Vec<_>>();
        let files = if paths.len() <= 1 {
            let mut opened = Vec::with_capacity(paths.len());
            for path in paths {
                opened.push(OpenTextFile {
                    path,
                    file: open_file(path)?,
                    fingerprint: None,
                });
            }
            ReplayFiles::Opened(opened)
        } else {
            // Preserve eager open-error reporting while never retaining more
            // than one descriptor for a multi-file source.
            for path in &paths {
                drop(open_file(path)?);
            }
            ReplayFiles::Verified {
                paths,
                fingerprints: None,
            }
        };
        Ok(TextSourceSession {
            inline_field: self.inline_field,
            inline: self.inline,
            files,
        })
    }

    /// Scan inline values and then every physical file line.
    ///
    /// Each call reopens the configured files. The borrowed line text is valid
    /// only for the duration of the visitor call, allowing one buffer to be
    /// reused for the complete scan.
    pub(crate) fn scan<E, F>(
        &self,
        classifier: &LineClassifier<'_>,
        mut visitor: F,
    ) -> Result<(), TextScanError<E>>
    where
        F: FnMut(TextLine<'_>) -> Result<(), E>,
    {
        visit_inline(self.inline_field, self.inline, classifier, &mut visitor)?;

        let mut buffer = String::with_capacity(256);
        for path in self.files {
            if path.trim().is_empty() {
                continue;
            }
            let file = File::open(path).map_err(|source| TextScanError::Open {
                path: PathBuf::from(path),
                source,
            })?;
            let mut reader = BufReader::with_capacity(TEXT_READER_CAPACITY, file);
            visit_reader(&mut reader, path, classifier, &mut buffer, &mut visitor)?;
        }
        Ok(())
    }

    /// Scan inline values and then every physical file line, skipping files
    /// that fail to open.
    ///
    /// Missing or unreadable files are reported in the returned list instead
    /// of aborting the scan; callers decide how to surface them. Content and
    /// read errors, and visitor errors, still abort the scan so per-line
    /// diagnostics stay precise. Because each call re-opens files, callers
    /// that scan multiple times must treat the result as an availability hint
    /// rather than a guarantee across scans.
    pub(crate) fn scan_tolerant<E, F>(
        &self,
        classifier: &LineClassifier<'_>,
        mut visitor: F,
    ) -> Result<Vec<SkippedTextFile>, TextScanError<E>>
    where
        F: FnMut(TextLine<'_>) -> Result<(), E>,
    {
        visit_inline(self.inline_field, self.inline, classifier, &mut visitor)?;

        let mut buffer = String::with_capacity(256);
        let mut skipped = Vec::new();
        for path in self.files {
            if path.trim().is_empty() {
                continue;
            }
            let file = match File::open(path) {
                Ok(file) => file,
                Err(source) => {
                    skipped.push(SkippedTextFile {
                        path: PathBuf::from(path),
                        reason: source.to_string(),
                    });
                    continue;
                }
            };
            let mut reader = BufReader::with_capacity(TEXT_READER_CAPACITY, file);
            visit_reader(&mut reader, path, classifier, &mut buffer, &mut visitor)?;
        }
        Ok(skipped)
    }

    /// Open a replay session that excludes files which fail to open.
    ///
    /// The set of available files is decided once here; every replay scan
    /// then covers exactly the same input, keeping multi-pass consumers
    /// consistent. A file that disappears after this call still aborts later
    /// scans, matching the fingerprint-change protection. Returns the session
    /// plus the files that were skipped at open time.
    ///
    /// Only feature-enabled consumers use this today; keep it available to the
    /// always-compiled test suite and to future tolerant replay callers.
    #[allow(dead_code)]
    pub(crate) fn open_replay_tolerant(
        &self,
    ) -> Result<
        (TextSourceSession<'a>, Vec<SkippedTextFile>),
        TextScanError<std::convert::Infallible>,
    > {
        let paths = self
            .files
            .iter()
            .map(String::as_str)
            .filter(|path| !path.trim().is_empty())
            .collect::<Vec<_>>();
        let mut skipped = Vec::new();
        let files = if paths.len() <= 1 {
            let mut opened = Vec::with_capacity(paths.len());
            for path in paths {
                match open_file(path) {
                    Ok(file) => opened.push(OpenTextFile {
                        path,
                        file,
                        fingerprint: None,
                    }),
                    Err(TextScanError::Open { path, source }) => {
                        skipped.push(SkippedTextFile {
                            path,
                            reason: source.to_string(),
                        });
                    }
                    Err(error) => return Err(error),
                }
            }
            ReplayFiles::Opened(opened)
        } else {
            // Decide availability up front so no replay scan retains more
            // than one descriptor for a multi-file source.
            let mut available = Vec::with_capacity(paths.len());
            for path in &paths {
                match open_file(path) {
                    Ok(file) => drop(file),
                    Err(TextScanError::Open { path, source }) => {
                        skipped.push(SkippedTextFile {
                            path,
                            reason: source.to_string(),
                        });
                        continue;
                    }
                    Err(error) => return Err(error),
                }
                available.push(*path);
            }
            ReplayFiles::Verified {
                paths: available,
                fingerprints: None,
            }
        };
        Ok((
            TextSourceSession {
                inline_field: self.inline_field,
                inline: self.inline,
                files,
            },
            skipped,
        ))
    }
}

/// A file that could not be opened and was skipped by a tolerant scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkippedTextFile {
    pub(crate) path: PathBuf,
    pub(crate) reason: String,
}

/// A text source session that rejects inconsistent input across replay scans.
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct TextSourceSession<'a> {
    inline_field: &'a str,
    inline: &'a [String],
    files: ReplayFiles<'a>,
}

#[derive(Debug)]
#[allow(dead_code)]
enum ReplayFiles<'a> {
    Opened(Vec<OpenTextFile<'a>>),
    Verified {
        paths: Vec<&'a str>,
        fingerprints: Option<Vec<[u8; 32]>>,
    },
}

#[derive(Debug)]
#[allow(dead_code)]
struct OpenTextFile<'a> {
    path: &'a str,
    file: File,
    fingerprint: Option<[u8; 32]>,
}

impl TextSourceSession<'_> {
    /// Replay inline values and files from their beginnings.
    ///
    /// Content changes are reported after the affected file has been visited.
    /// Consumers must therefore write only into an unpublished candidate that
    /// can be discarded when this method returns an error.
    #[allow(dead_code)]
    pub(crate) fn scan<E, F>(
        &mut self,
        classifier: &LineClassifier<'_>,
        mut visitor: F,
    ) -> Result<(), TextScanError<E>>
    where
        F: FnMut(TextLine<'_>) -> Result<(), E>,
    {
        visit_inline(self.inline_field, self.inline, classifier, &mut visitor)?;

        match &mut self.files {
            ReplayFiles::Opened(files) => {
                let mut buffer = String::with_capacity(256);
                for opened in files {
                    opened.file.rewind().map_err(|source| TextScanError::Read {
                        path: PathBuf::from(opened.path),
                        line: 1,
                        source,
                    })?;
                    let hashing_reader = FingerprintReader::new(&mut opened.file);
                    let mut reader = BufReader::with_capacity(TEXT_READER_CAPACITY, hashing_reader);
                    visit_reader(
                        &mut reader,
                        opened.path,
                        classifier,
                        &mut buffer,
                        &mut visitor,
                    )?;
                    let fingerprint = reader.into_inner().finish();
                    if let Some(expected) = opened.fingerprint {
                        if expected != fingerprint {
                            return Err(TextScanError::Changed {
                                path: PathBuf::from(opened.path),
                            });
                        }
                    } else {
                        opened.fingerprint = Some(fingerprint);
                    }
                }
            }
            ReplayFiles::Verified {
                paths,
                fingerprints,
            } => {
                let capture_fingerprints = fingerprints.is_none();
                let mut captured = capture_fingerprints.then(|| Vec::with_capacity(paths.len()));
                let mut buffer = String::with_capacity(256);
                for (index, path) in paths.iter().copied().enumerate() {
                    let file = open_file(path)?;
                    let hashing_reader = FingerprintReader::new(file);
                    let mut reader = BufReader::with_capacity(TEXT_READER_CAPACITY, hashing_reader);
                    visit_reader(&mut reader, path, classifier, &mut buffer, &mut visitor)?;
                    let fingerprint = reader.into_inner().finish();
                    if let Some(expected) = fingerprints.as_ref() {
                        if expected[index] != fingerprint {
                            return Err(TextScanError::Changed {
                                path: PathBuf::from(path),
                            });
                        }
                    } else if let Some(captured) = &mut captured {
                        captured.push(fingerprint);
                    }
                }
                if let Some(captured) = captured {
                    *fingerprints = Some(captured);
                }
            }
        }
        Ok(())
    }
}

fn open_file<E>(path: &str) -> Result<File, TextScanError<E>> {
    File::open(path).map_err(|source| TextScanError::Open {
        path: PathBuf::from(path),
        source,
    })
}

fn visit_inline<E, F>(
    field: &str,
    inline: &[String],
    classifier: &LineClassifier<'_>,
    visitor: &mut F,
) -> Result<(), TextScanError<E>>
where
    F: FnMut(TextLine<'_>) -> Result<(), E>,
{
    for (index, raw) in inline.iter().enumerate() {
        visit_line(
            raw,
            TextLocation::Inline { field, index },
            classifier,
            visitor,
        )?;
    }
    Ok(())
}

fn visit_reader<E, F>(
    reader: &mut impl BufRead,
    path: &str,
    classifier: &LineClassifier<'_>,
    buffer: &mut String,
    visitor: &mut F,
) -> Result<(), TextScanError<E>>
where
    F: FnMut(TextLine<'_>) -> Result<(), E>,
{
    let mut line_no = 0usize;
    loop {
        buffer.clear();
        let bytes = reader
            .read_line(buffer)
            .map_err(|source| TextScanError::Read {
                path: PathBuf::from(path),
                line: line_no + 1,
                source,
            })?;
        if bytes == 0 {
            return Ok(());
        }
        line_no += 1;
        remove_line_ending(buffer);
        visit_line(
            buffer,
            TextLocation::File {
                path,
                line: line_no,
            },
            classifier,
            visitor,
        )?;
    }
}

fn visit_line<E, F>(
    raw: &str,
    location: TextLocation<'_>,
    classifier: &LineClassifier<'_>,
    visitor: &mut F,
) -> Result<(), TextScanError<E>>
where
    F: FnMut(TextLine<'_>) -> Result<(), E>,
{
    let trimmed = raw.trim();
    let annotations = classifier.classify(raw, trimmed);
    visitor(TextLine {
        raw,
        trimmed,
        location,
        annotations,
    })
    .map_err(|source| TextScanError::Consumer {
        location: location.into(),
        source,
    })
}

fn remove_line_ending(line: &mut String) {
    if line.ends_with('\n') {
        line.pop();
        if line.ends_with('\r') {
            line.pop();
        }
    }
}

/// Borrowed physical location of one source line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextLocation<'a> {
    Inline { field: &'a str, index: usize },
    File { path: &'a str, line: usize },
}

impl Display for TextLocation<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Inline { field, index } => write!(f, "{field}[{index}]"),
            Self::File { path, line } => write!(f, "file '{path}', line {line}"),
        }
    }
}

/// Owned location retained only by an error path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OwnedTextLocation {
    Inline { field: String, index: usize },
    File { path: PathBuf, line: usize },
}

impl From<TextLocation<'_>> for OwnedTextLocation {
    fn from(value: TextLocation<'_>) -> Self {
        match value {
            TextLocation::Inline { field, index } => Self::Inline {
                field: field.to_owned(),
                index,
            },
            TextLocation::File { path, line } => Self::File {
                path: PathBuf::from(path),
                line,
            },
        }
    }
}

impl Display for OwnedTextLocation {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Inline { field, index } => write!(f, "{field}[{index}]"),
            Self::File { path, line } => write!(f, "file '{}', line {line}", path.display()),
        }
    }
}

/// A borrowed line and its non-destructive lexical metadata.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TextLine<'a> {
    raw: &'a str,
    trimmed: &'a str,
    location: TextLocation<'a>,
    annotations: LineAnnotations<'a>,
}

impl<'a> TextLine<'a> {
    pub(crate) const fn raw(&self) -> &'a str {
        self.raw
    }

    pub(crate) const fn trimmed(&self) -> &'a str {
        self.trimmed
    }

    pub(crate) const fn location(&self) -> TextLocation<'a> {
        self.location
    }

    pub(crate) const fn annotations(&self) -> LineAnnotations<'a> {
        self.annotations
    }
}

/// Lexical facts about a physical line. They never imply filtering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LineAnnotations<'a> {
    pub(crate) blank: bool,
    pub(crate) leading_comment: Option<&'a str>,
}

/// Configures non-destructive leading-comment recognition.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct LineClassifier<'a> {
    comment_markers: &'a [&'a str],
}

impl<'a> LineClassifier<'a> {
    pub(crate) const fn new(comment_markers: &'a [&'a str]) -> Self {
        Self { comment_markers }
    }

    fn classify<'line>(&self, raw: &'line str, trimmed: &'line str) -> LineAnnotations<'a> {
        let start_trimmed = raw.trim_start();
        let leading_comment = self
            .comment_markers
            .iter()
            .copied()
            .filter(|marker| !marker.is_empty() && start_trimmed.starts_with(marker))
            .max_by_key(|marker| marker.len());
        LineAnnotations {
            blank: trimmed.is_empty(),
            leading_comment,
        }
    }
}

/// Failure while opening, reading, replaying, or consuming a text source.
#[derive(Debug)]
pub(crate) enum TextScanError<E> {
    Open {
        path: PathBuf,
        source: io::Error,
    },
    Read {
        path: PathBuf,
        line: usize,
        source: io::Error,
    },
    Changed {
        path: PathBuf,
    },
    Consumer {
        location: OwnedTextLocation,
        source: E,
    },
}

impl<E: Display> Display for TextScanError<E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Open { path, source } => {
                write!(
                    f,
                    "failed to open text source '{}': {source}",
                    path.display()
                )
            }
            Self::Read { path, line, source } => write!(
                f,
                "failed to read text source '{}' at line {line}: {source}",
                path.display()
            ),
            Self::Changed { path } => write!(
                f,
                "text source '{}' changed between replay scans",
                path.display()
            ),
            Self::Consumer { location, source } => write!(f, "{location}: {source}"),
        }
    }
}

impl<E> std::error::Error for TextScanError<E>
where
    E: std::error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Open { source, .. } | Self::Read { source, .. } => Some(source),
            Self::Changed { .. } => None,
            Self::Consumer { source, .. } => Some(source),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::{NamedTempFile, TempDir};

    use super::*;

    #[test]
    fn scan_preserves_order_raw_text_locations_and_crlf() {
        let mut first = NamedTempFile::new().unwrap();
        write!(first, "  file-a  \r\n\r\n# file-comment\n").unwrap();
        let mut second = NamedTempFile::new().unwrap();
        write!(second, "file-b").unwrap();
        let inline = vec![" inline ".to_string(), "".to_string()];
        let files = vec![
            first.path().display().to_string(),
            second.path().display().to_string(),
        ];
        let source = TextSource::new("args.rules", &inline, &files);
        let classifier = LineClassifier::new(&["#"]);
        let mut actual = Vec::new();
        source
            .scan(&classifier, |line| {
                actual.push((
                    line.raw().to_owned(),
                    line.trimmed().to_owned(),
                    line.location().to_string(),
                    line.annotations().blank,
                    line.annotations().leading_comment.map(str::to_owned),
                ));
                Ok::<_, String>(())
            })
            .unwrap();

        assert_eq!(actual[0].0, " inline ");
        assert_eq!(actual[0].2, "args.rules[0]");
        assert!(actual[1].3);
        assert_eq!(actual[2].0, "  file-a  ");
        assert_eq!(actual[2].1, "file-a");
        assert!(actual[3].3);
        assert_eq!(actual[4].4.as_deref(), Some("#"));
        assert!(actual[4].2.ends_with("line 3"));
        assert_eq!(actual[5].0, "file-b");
        assert!(actual[5].2.ends_with("line 1"));
    }

    #[test]
    fn annotations_do_not_filter_and_choose_longest_marker() {
        let inline = vec![
            "  ## cosmetic".to_string(),
            "# comment".to_string(),
            "example.test##banner".to_string(),
        ];
        let source = TextSource::new("rules", &inline, &[]);
        let classifier = LineClassifier::new(&["#", "##"]);
        let mut markers = Vec::new();
        source
            .scan(&classifier, |line| {
                markers.push(line.annotations().leading_comment.map(str::to_owned));
                Ok::<_, String>(())
            })
            .unwrap();
        assert_eq!(
            markers,
            vec![Some("##".to_string()), Some("#".to_string()), None]
        );
    }

    #[test]
    fn scans_are_repeatable_and_blank_paths_are_ignored() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "one\ntwo").unwrap();
        let files = vec!["  ".to_string(), file.path().display().to_string()];
        let source = TextSource::new("rules", &[], &files);
        for _ in 0..2 {
            let mut lines = 0;
            source
                .scan(&LineClassifier::default(), |_| {
                    lines += 1;
                    Ok::<_, String>(())
                })
                .unwrap();
            assert_eq!(lines, 2);
        }
    }

    #[test]
    fn replay_session_keeps_opened_file_snapshot_after_path_replacement() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("rules.txt");
        let replacement = dir.path().join("replacement.txt");
        std::fs::write(&path, "old\n").unwrap();
        std::fs::write(&replacement, "new\n").unwrap();
        let files = vec![path.display().to_string()];
        let source = TextSource::new("rules", &[], &files);
        let mut session = source.open_replay().unwrap();

        let mut first = Vec::new();
        session
            .scan(&LineClassifier::default(), |line| {
                first.push(line.raw().to_owned());
                Ok::<_, String>(())
            })
            .unwrap();

        std::fs::remove_file(&path).unwrap();
        std::fs::rename(&replacement, &path).unwrap();

        let mut replayed = Vec::new();
        session
            .scan(&LineClassifier::default(), |line| {
                replayed.push(line.raw().to_owned());
                Ok::<_, String>(())
            })
            .unwrap();
        assert_eq!(first, vec!["old"]);
        assert_eq!(replayed, first);

        let mut reopened = Vec::new();
        source
            .scan(&LineClassifier::default(), |line| {
                reopened.push(line.raw().to_owned());
                Ok::<_, String>(())
            })
            .unwrap();
        assert_eq!(reopened, vec!["new"]);
    }

    #[test]
    fn replay_session_rejects_in_place_single_file_changes() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("rules.txt");
        std::fs::write(&path, "old\n").unwrap();
        let files = vec![path.display().to_string()];
        let source = TextSource::new("rules", &[], &files);
        let mut session = source.open_replay().unwrap();

        session
            .scan(&LineClassifier::default(), |_| Ok::<_, String>(()))
            .unwrap();
        std::fs::write(&path, "new\n").unwrap();

        let error = session
            .scan(&LineClassifier::default(), |_| Ok::<_, String>(()))
            .unwrap_err();
        assert!(matches!(error, TextScanError::Changed { path: changed } if changed == path));
    }

    #[test]
    fn replay_session_bounds_descriptors_and_rejects_changed_multi_file_input() {
        let dir = TempDir::new().unwrap();
        let first_path = dir.path().join("first.txt");
        let second_path = dir.path().join("second.txt");
        let replacement = dir.path().join("replacement.txt");
        std::fs::write(&first_path, "first\n").unwrap();
        std::fs::write(&second_path, "old\n").unwrap();
        std::fs::write(&replacement, "new\n").unwrap();
        let files = vec![
            first_path.display().to_string(),
            second_path.display().to_string(),
        ];
        let source = TextSource::new("rules", &[], &files);
        let mut session = source.open_replay().unwrap();
        assert!(matches!(
            &session.files,
            ReplayFiles::Verified {
                fingerprints: None,
                ..
            }
        ));

        session
            .scan(&LineClassifier::default(), |_| Ok::<_, String>(()))
            .unwrap();
        assert!(matches!(
            &session.files,
            ReplayFiles::Verified {
                fingerprints: Some(fingerprints),
                ..
            } if fingerprints.len() == 2
        ));

        std::fs::remove_file(&second_path).unwrap();
        std::fs::rename(&replacement, &second_path).unwrap();
        let error = session
            .scan(&LineClassifier::default(), |_| Ok::<_, String>(()))
            .unwrap_err();
        assert!(matches!(error, TextScanError::Changed { path } if path == second_path));
    }

    #[test]
    fn reports_open_and_consumer_errors_with_locations() {
        let dir = TempDir::new().unwrap();
        let files = vec![dir.path().join("missing").display().to_string()];
        let source = TextSource::new("rules", &[], &files);
        let error = source
            .scan::<String, _>(&LineClassifier::default(), |_| Ok(()))
            .unwrap_err();
        assert!(matches!(error, TextScanError::Open { .. }));

        let inline = vec!["bad".to_string()];
        let source = TextSource::new("rules", &inline, &[]);
        let error = source
            .scan(&LineClassifier::default(), |_| Err("invalid".to_string()))
            .unwrap_err();
        assert_eq!(error.to_string(), "rules[0]: invalid");
    }

    #[test]
    fn tolerant_scan_skips_missing_files_but_keeps_others() {
        let dir = TempDir::new().unwrap();
        let present = dir.path().join("present.txt");
        let mut present_file = std::fs::File::create(&present).unwrap();
        writeln!(present_file, "kept").unwrap();
        let files = vec![
            "  ".to_string(),
            dir.path().join("missing.txt").display().to_string(),
            present.display().to_string(),
        ];
        let inline = vec!["inline-rule".to_string()];
        let source = TextSource::new("rules", &inline, &files);

        let mut lines = Vec::new();
        let skipped = source
            .scan_tolerant(&LineClassifier::default(), |line| {
                lines.push(line.trimmed().to_owned());
                Ok::<_, String>(())
            })
            .unwrap();

        assert_eq!(lines, vec!["inline-rule", "kept"]);
        assert_eq!(skipped.len(), 1);
        assert!(skipped[0].path.ends_with("missing.txt"));
        assert!(!skipped[0].reason.is_empty());
    }

    #[test]
    fn tolerant_scan_still_aborts_on_consumer_errors() {
        let inline = vec!["bad".to_string()];
        let source = TextSource::new("rules", &inline, &[]);
        let error = source
            .scan_tolerant(&LineClassifier::default(), |_| Err("invalid".to_string()))
            .unwrap_err();
        assert_eq!(error.to_string(), "rules[0]: invalid");
    }

    #[test]
    fn tolerant_replay_excludes_missing_files_from_every_scan() {
        let dir = TempDir::new().unwrap();
        let first_path = dir.path().join("first.txt");
        let second_path = dir.path().join("missing.txt");
        std::fs::write(&first_path, "first\n").unwrap();
        let files = vec![
            first_path.display().to_string(),
            second_path.display().to_string(),
        ];
        let source = TextSource::new("rules", &[], &files);

        let (mut session, skipped) = source.open_replay_tolerant().unwrap();
        assert_eq!(skipped.len(), 1);
        assert!(skipped[0].path.ends_with("missing.txt"));

        for _ in 0..2 {
            let mut lines = Vec::new();
            session
                .scan(&LineClassifier::default(), |line| {
                    lines.push(line.trimmed().to_owned());
                    Ok::<_, String>(())
                })
                .unwrap();
            assert_eq!(lines, vec!["first"]);
        }
    }

    #[test]
    fn tolerant_replay_single_missing_file_yields_inline_only_session() {
        let files = vec!["/definitely/missing/rules.txt".to_string()];
        let inline = vec!["inline".to_string()];
        let source = TextSource::new("rules", &inline, &files);

        let (mut session, skipped) = source.open_replay_tolerant().unwrap();
        assert_eq!(skipped.len(), 1);
        let mut lines = Vec::new();
        session
            .scan(&LineClassifier::default(), |line| {
                lines.push(line.trimmed().to_owned());
                Ok::<_, String>(())
            })
            .unwrap();
        assert_eq!(lines, vec!["inline"]);
    }

    #[cfg(unix)]
    #[test]
    fn reports_read_errors_with_physical_line() {
        let dir = TempDir::new().unwrap();
        let files = vec![dir.path().display().to_string()];
        let source = TextSource::new("rules", &[], &files);
        let error = source
            .scan::<String, _>(&LineClassifier::default(), |_| Ok(()))
            .unwrap_err();
        match error {
            TextScanError::Read { line, .. } => assert_eq!(line, 1),
            other => panic!("expected read error, got {other}"),
        }
    }
}
