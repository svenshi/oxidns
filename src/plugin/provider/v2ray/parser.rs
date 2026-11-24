// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs::File;
use std::io::{BufReader, Error as IoError, Read, Seek};
use std::path::{Path, PathBuf};

use prost::Message;

use super::model::{Cidr, Domain, DomainType, GeoIp, GeoIpList, GeoSite, GeoSiteList, attribute};
use crate::infra::io::FingerprintReader;

const MAX_PROTOBUF_GROUP_DEPTH: usize = 100;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ParsedDat {
    GeoSite(GeoSiteList),
    GeoIp(GeoIpList),
}

pub(crate) fn geoip_code(entry: &GeoIp) -> &str {
    if entry.code.is_empty() {
        entry.country_code.as_str()
    } else {
        entry.code.as_str()
    }
}

pub(crate) fn geosite_code(entry: &GeoSite) -> &str {
    if entry.code.is_empty() {
        entry.country_code.as_str()
    } else {
        entry.code.as_str()
    }
}

pub(crate) fn cidr_to_rule(cidr: &Cidr) -> Option<String> {
    match cidr.ip.len() {
        4 => Some(format!(
            "{}.{}.{}.{}/{}",
            cidr.ip[0], cidr.ip[1], cidr.ip[2], cidr.ip[3], cidr.prefix
        )),
        16 => {
            let mut octets = [0u8; 16];
            octets.copy_from_slice(&cidr.ip);
            Some(format!(
                "{}/{}",
                std::net::Ipv6Addr::from(octets),
                cidr.prefix
            ))
        }
        _ => None,
    }
}

pub(crate) fn geosite_domain_expression(domain: &Domain) -> Result<String, String> {
    let prefix = match domain_type(domain)? {
        DomainType::Plain => "keyword:",
        DomainType::Regex => "regexp:",
        DomainType::RootDomain => "domain:",
        DomainType::Full => "full:",
    };
    Ok(format!("{}{}", prefix, domain.value))
}

fn geosite_domain_expression_original(domain: &Domain) -> Result<String, String> {
    let prefix = match domain_type(domain)? {
        DomainType::Plain => "plain:",
        DomainType::Regex => "regex:",
        DomainType::RootDomain => "root_domain:",
        DomainType::Full => "full:",
    };
    Ok(format!("{}{}", prefix, domain.value))
}

pub(crate) fn geosite_domain_expression_original_with_attrs(
    domain: &Domain,
) -> Result<String, String> {
    let mut line = geosite_domain_expression_original(domain)?;
    for attribute in &domain.attribute {
        line.push(' ');
        line.push('@');
        line.push_str(attribute.key.as_str());
        match &attribute.typed_value {
            None | Some(attribute::TypedValue::BoolValue(true)) => {}
            Some(attribute::TypedValue::BoolValue(false)) => line.push_str("=false"),
            Some(attribute::TypedValue::IntValue(value)) => {
                line.push('=');
                line.push_str(value.to_string().as_str());
            }
        }
    }
    Ok(line)
}

pub(crate) fn parse_geosite_dat(data: &[u8]) -> Result<GeoSiteList, String> {
    let list = GeoSiteList::decode(data).map_err(|error| error.to_string())?;
    is_valid_geosite_list(&list)
        .then_some(list)
        .ok_or_else(|| "decoded geosite payload failed structural validation".to_string())
}

pub(crate) fn parse_geoip_dat(data: &[u8]) -> Result<GeoIpList, String> {
    let list = GeoIpList::decode(data).map_err(|error| error.to_string())?;
    is_valid_geoip_list(&list)
        .then_some(list)
        .ok_or_else(|| "decoded geoip payload failed structural validation".to_string())
}

/// One opened protobuf file that can be replayed without following a replaced
/// path between planning and compilation passes.
#[derive(Debug)]
pub(crate) struct DatFileSession {
    path: PathBuf,
    file: File,
    fingerprint: Option<[u8; 32]>,
}

impl DatFileSession {
    /// Open failures stay typed as I/O errors so callers can distinguish an
    /// unavailable file from invalid dat content.
    pub(crate) fn open(path: &Path) -> Result<Self, IoError> {
        let file = File::open(path)?;
        file.metadata()?;
        Ok(Self {
            path: path.to_path_buf(),
            file,
            fingerprint: None,
        })
    }

    pub(crate) fn visit_geosite<F>(&mut self, on_entry: F) -> Result<(), String>
    where
        F: FnMut(GeoSite) -> Result<(), String>,
    {
        self.visit::<GeoSite, _>(on_entry)
    }

    pub(crate) fn visit_geoip<F>(&mut self, on_entry: F) -> Result<(), String>
    where
        F: FnMut(GeoIp) -> Result<(), String>,
    {
        self.visit::<GeoIp, _>(on_entry)
    }

    fn visit<M, F>(&mut self, on_entry: F) -> Result<(), String>
    where
        M: Message + Default,
        F: FnMut(M) -> Result<(), String>,
    {
        let file_len = self
            .file
            .metadata()
            .map_err(|error| format!("failed to inspect '{}': {error}", self.path.display()))?
            .len();
        self.file
            .rewind()
            .map_err(|error| format!("failed to rewind '{}': {error}", self.path.display()))?;
        let hashing_reader = FingerprintReader::new(&mut self.file);
        let buffered_reader = BufReader::with_capacity(256 * 1024, hashing_reader);
        let mut reader = ReadCursor::new(buffered_reader);
        visit_top_level_entries::<M, _>(&mut reader, file_len, on_entry)?;
        let fingerprint = reader.into_inner().into_inner().finish();
        if let Some(expected) = self.fingerprint {
            if expected != fingerprint {
                return Err(format!(
                    "protobuf source '{}' changed between replay scans",
                    self.path.display()
                ));
            }
        } else {
            self.fingerprint = Some(fingerprint);
        }
        Ok(())
    }
}

struct ReadCursor<R> {
    inner: R,
    position: u64,
}

impl<R> ReadCursor<R> {
    fn new(inner: R) -> Self {
        Self { inner, position: 0 }
    }

    fn position(&self) -> u64 {
        self.position
    }

    fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: Read> Read for ReadCursor<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buffer)?;
        self.position += read as u64;
        Ok(read)
    }
}

fn visit_top_level_entries<M, F>(
    reader: &mut ReadCursor<impl Read>,
    file_len: u64,
    mut on_entry: F,
) -> Result<(), String>
where
    M: Message + Default,
    F: FnMut(M) -> Result<(), String>,
{
    let mut payload = Vec::new();
    while let Some(key) = read_varint(reader, true)? {
        if key == 0 {
            return Err("protobuf field key must not be zero".to_string());
        }
        let field = key >> 3;
        let wire = (key & 7) as u8;
        if field == 1 {
            if wire != 2 {
                return Err(format!(
                    "protobuf entry field has unexpected wire type {wire}"
                ));
            }
            let encoded_len = read_required_varint(reader)?;
            let position = reader.position();
            if encoded_len > file_len.saturating_sub(position) {
                return Err(format!(
                    "truncated protobuf entry payload: declared {encoded_len} bytes, only {} remain",
                    file_len.saturating_sub(position)
                ));
            }
            let len = usize::try_from(encoded_len)
                .map_err(|_| "protobuf entry length exceeds platform limits".to_string())?;
            payload.resize(len, 0);
            reader
                .read_exact(&mut payload)
                .map_err(|error| format!("truncated protobuf entry payload: {error}"))?;
            let entry = M::decode(payload.as_slice())
                .map_err(|error| format!("failed to decode protobuf entry: {error}"))?;
            on_entry(entry)?;
        } else {
            skip_field(reader, wire, field, 0)?;
        }
    }
    Ok(())
}

fn read_required_varint(reader: &mut impl Read) -> Result<u64, String> {
    read_varint(reader, false)?.ok_or_else(|| "truncated protobuf varint".to_string())
}

fn read_varint(reader: &mut impl Read, allow_clean_eof: bool) -> Result<Option<u64>, String> {
    let mut value = 0u64;
    for shift in (0..70).step_by(7) {
        let mut byte = [0u8; 1];
        match reader.read_exact(&mut byte) {
            Ok(()) => {}
            Err(error)
                if allow_clean_eof
                    && shift == 0
                    && error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                return Ok(None);
            }
            Err(error) => return Err(format!("truncated protobuf varint: {error}")),
        }
        if shift == 63 && byte[0] > 1 {
            return Err("protobuf varint overflows u64".to_string());
        }
        value |= u64::from(byte[0] & 0x7F) << shift;
        if byte[0] & 0x80 == 0 {
            return Ok(Some(value));
        }
    }
    Err("protobuf varint is too long".to_string())
}

fn skip_field(
    reader: &mut impl Read,
    wire: u8,
    field: u64,
    group_depth: usize,
) -> Result<(), String> {
    match wire {
        0 => {
            read_required_varint(reader)?;
            Ok(())
        }
        1 => skip_bytes(reader, 8),
        2 => {
            let len = usize::try_from(read_required_varint(reader)?)
                .map_err(|_| "protobuf field length exceeds platform limits".to_string())?;
            skip_bytes(reader, len)
        }
        3 => {
            if group_depth >= MAX_PROTOBUF_GROUP_DEPTH {
                return Err(format!(
                    "protobuf group nesting exceeds limit of {MAX_PROTOBUF_GROUP_DEPTH}"
                ));
            }
            loop {
                let key = read_required_varint(reader)?;
                let nested_field = key >> 3;
                let nested_wire = (key & 7) as u8;
                if nested_wire == 4 {
                    if nested_field != field {
                        return Err("mismatched protobuf end-group field".to_string());
                    }
                    return Ok(());
                }
                skip_field(reader, nested_wire, nested_field, group_depth + 1)?;
            }
        }
        4 => Err("unexpected protobuf end-group field".to_string()),
        5 => skip_bytes(reader, 4),
        _ => Err(format!("invalid protobuf wire type {wire}")),
    }
}

fn skip_bytes(reader: &mut impl Read, mut len: usize) -> Result<(), String> {
    let mut scratch = [0u8; 8192];
    while len > 0 {
        let chunk = len.min(scratch.len());
        reader
            .read_exact(&mut scratch[..chunk])
            .map_err(|error| format!("truncated protobuf field: {error}"))?;
        len -= chunk;
    }
    Ok(())
}

pub(crate) fn detect_dat_kind(data: &[u8]) -> Result<ParsedDat, String> {
    let geosite = parse_geosite_dat(data).ok().map(ParsedDat::GeoSite);
    let geoip = parse_geoip_dat(data).ok().map(ParsedDat::GeoIp);
    match (geosite, geoip) {
        (Some(_), Some(_)) => {
            Err("dat kind is ambiguous; please pass --kind geosite or --kind geoip".to_string())
        }
        (Some(parsed), None) | (None, Some(parsed)) => Ok(parsed),
        (None, None) => Err("failed to identify dat kind from file contents".to_string()),
    }
}

fn domain_type(domain: &Domain) -> Result<DomainType, String> {
    DomainType::try_from(domain.r#type).map_err(|_| {
        format!(
            "unsupported domain type '{}' for '{}'",
            domain.r#type, domain.value
        )
    })
}

fn is_valid_geosite_list(list: &GeoSiteList) -> bool {
    !list.entry.is_empty()
        && list.entry.iter().all(|entry| {
            !geosite_code(entry).trim().is_empty()
                && !entry.domain.is_empty()
                && entry
                    .domain
                    .iter()
                    .all(|domain| !domain.value.trim().is_empty())
        })
}

fn is_valid_geoip_list(list: &GeoIpList) -> bool {
    !list.entry.is_empty()
        && list.entry.iter().all(|entry| {
            !geoip_code(entry).trim().is_empty()
                && !entry.cidr.is_empty()
                && entry
                    .cidr
                    .iter()
                    .all(|cidr| matches!(cidr.ip.len(), 4 | 16))
        })
}

#[cfg(test)]
mod streaming_tests {
    use std::io::Write;

    use tempfile::{NamedTempFile, TempDir};

    use super::*;

    fn visit_geosite_file<F>(path: &Path, on_entry: F) -> Result<(), String>
    where
        F: FnMut(GeoSite) -> Result<(), String>,
    {
        DatFileSession::open(path)
            .map_err(|error| format!("failed to open '{}': {error}", path.display()))?
            .visit_geosite(on_entry)
    }

    fn encoded_site(code: &str) -> Vec<u8> {
        GeoSiteList {
            entry: vec![GeoSite {
                country_code: code.to_string(),
                domain: vec![Domain {
                    r#type: DomainType::RootDomain as i32,
                    value: format!("{code}.example"),
                    attribute: Vec::new(),
                }],
                resource_hash: Vec::new(),
                code: String::new(),
                file_path: String::new(),
            }],
        }
        .encode_to_vec()
    }

    #[test]
    fn streams_entries_and_skips_unknown_top_level_fields() {
        let list = GeoSiteList {
            entry: vec![
                GeoSite {
                    country_code: "CN".to_string(),
                    domain: vec![Domain {
                        r#type: DomainType::RootDomain as i32,
                        value: "example.cn".to_string(),
                        attribute: Vec::new(),
                    }],
                    resource_hash: Vec::new(),
                    code: String::new(),
                    file_path: String::new(),
                },
                GeoSite {
                    country_code: "US".to_string(),
                    domain: vec![Domain {
                        r#type: DomainType::Full as i32,
                        value: "example.com".to_string(),
                        attribute: Vec::new(),
                    }],
                    resource_hash: Vec::new(),
                    code: String::new(),
                    file_path: String::new(),
                },
            ],
        };
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(&[0x10, 0x01]).unwrap();
        file.write_all(&list.encode_to_vec()).unwrap();
        let mut codes = Vec::new();
        visit_geosite_file(file.path(), |entry| {
            codes.push(geosite_code(&entry).to_string());
            Ok(())
        })
        .unwrap();
        assert_eq!(codes, vec!["CN", "US"]);
    }

    #[test]
    fn dat_session_replays_opened_snapshot_after_path_replacement() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("geosite.dat");
        let replacement = dir.path().join("replacement.dat");
        std::fs::write(&path, encoded_site("OLD")).unwrap();
        std::fs::write(&replacement, encoded_site("NEW")).unwrap();
        let mut session = DatFileSession::open(&path).unwrap();

        let mut first = Vec::new();
        session
            .visit_geosite(|entry| {
                first.push(geosite_code(&entry).to_string());
                Ok(())
            })
            .unwrap();

        std::fs::remove_file(&path).unwrap();
        std::fs::rename(&replacement, &path).unwrap();
        let mut replayed = Vec::new();
        session
            .visit_geosite(|entry| {
                replayed.push(geosite_code(&entry).to_string());
                Ok(())
            })
            .unwrap();

        assert_eq!(first, vec!["OLD"]);
        assert_eq!(replayed, first);
    }

    #[test]
    fn dat_session_rejects_in_place_changes_between_visits() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("geosite.dat");
        std::fs::write(&path, encoded_site("OLD")).unwrap();
        let mut session = DatFileSession::open(&path).unwrap();

        session.visit_geosite(|_| Ok(())).unwrap();
        std::fs::write(&path, encoded_site("NEW")).unwrap();

        let error = session.visit_geosite(|_| Ok(())).unwrap_err();
        assert!(error.contains("changed between replay scans"));
    }

    #[test]
    fn rejects_truncated_entry_payload() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(&[0x0A, 0x05, 0x01]).unwrap();
        let error = visit_geosite_file(file.path(), |_| Ok(())).unwrap_err();
        assert!(error.contains("truncated protobuf entry payload"));
    }

    #[test]
    fn rejects_overflowing_length_varint_without_allocating_payload() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(&[0x0A]).unwrap();
        file.write_all(&[0xFF; 9]).unwrap();
        file.write_all(&[0x02]).unwrap();
        let error = visit_geosite_file(file.path(), |_| Ok(())).unwrap_err();
        assert!(error.contains("overflows u64"), "{error}");
    }

    #[test]
    fn accepts_unknown_groups_at_recursion_limit() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(&[0x13; MAX_PROTOBUF_GROUP_DEPTH]).unwrap();
        file.write_all(&[0x14; MAX_PROTOBUF_GROUP_DEPTH]).unwrap();
        visit_geosite_file(file.path(), |_| Ok(())).unwrap();
    }

    #[test]
    fn rejects_unknown_groups_beyond_recursion_limit() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(&[0x13; MAX_PROTOBUF_GROUP_DEPTH + 1])
            .unwrap();
        let error = visit_geosite_file(file.path(), |_| Ok(())).unwrap_err();
        assert!(error.contains("group nesting exceeds limit"), "{error}");
    }
}
