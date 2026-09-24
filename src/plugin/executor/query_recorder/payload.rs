// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Versioned, lossless response snapshots, independent of SQLite and API rows.

use std::io::Write;

use flate2::write::ZlibEncoder;
use flate2::{Compression, Decompress, FlushDecompress, Status};
use serde::{Deserialize, Serialize};

use super::model::{EdnsJson, RecordJson, RecordRow};
use crate::infra::error::{DnsError, Result};

const VERSION: i64 = 1;
const RAW: i64 = 0;
const ZLIB: i64 = 1;
const MIN_COMPRESS_BYTES: usize = 2 * 1024;
const MAX_COMPRESS_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone)]
pub(super) struct EncodedPayload {
    pub(super) version: i64,
    pub(super) codec: i64,
    pub(super) raw_len: i64,
    pub(super) data: Vec<u8>,
}

#[derive(Debug, Deserialize, PartialEq)]
pub(super) struct ResponseSnapshot {
    pub(super) req_edns_json: Option<EdnsJson>,
    pub(super) answers_json: Vec<RecordJson>,
    pub(super) authorities_json: Vec<RecordJson>,
    pub(super) additionals_json: Vec<RecordJson>,
    pub(super) signature_json: Vec<RecordJson>,
    pub(super) resp_edns_json: Option<EdnsJson>,
}

#[derive(Serialize)]
struct SnapshotRef<'a> {
    req_edns_json: &'a Option<EdnsJson>,
    answers_json: &'a [RecordJson],
    authorities_json: &'a [RecordJson],
    additionals_json: &'a [RecordJson],
    signature_json: &'a [RecordJson],
    resp_edns_json: &'a Option<EdnsJson>,
}

impl EncodedPayload {
    #[cfg(test)]
    pub(super) fn encode(record: &RecordRow) -> Result<Self> {
        Self::encode_with_compression(record, true)
    }

    pub(super) fn encode_with_compression(record: &RecordRow, compress: bool) -> Result<Self> {
        let raw = serde_json::to_vec(&SnapshotRef {
            req_edns_json: &record.req_edns_json,
            answers_json: &record.answers_json,
            authorities_json: &record.authorities_json,
            additionals_json: &record.additionals_json,
            signature_json: &record.signature_json,
            resp_edns_json: &record.resp_edns_json,
        })?;
        Self::encode_json(raw, compress)
    }

    fn encode_json(raw: Vec<u8>, compress: bool) -> Result<Self> {
        let raw_len = i64::try_from(raw.len()).map_err(|_| invalid("snapshot is too large"))?;
        if compress && (MIN_COMPRESS_BYTES..=MAX_COMPRESS_BYTES).contains(&raw.len()) {
            let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
            encoder.write_all(&raw)?;
            let data = encoder.finish()?;
            if data.len() <= raw.len() * 4 / 5 {
                return Ok(Self {
                    version: VERSION,
                    codec: ZLIB,
                    raw_len,
                    data,
                });
            }
        }
        Ok(Self {
            version: VERSION,
            codec: RAW,
            raw_len,
            data: raw,
        })
    }

    pub(super) fn decode(&self) -> Result<ResponseSnapshot> {
        if self.version != VERSION {
            return Err(invalid("unsupported payload version"));
        }
        let raw_len =
            usize::try_from(self.raw_len).map_err(|_| invalid("invalid payload length"))?;
        match self.codec {
            RAW => {
                if raw_len != self.data.len() {
                    return Err(invalid("raw payload length mismatch"));
                }
                Ok(serde_json::from_slice(&self.data)?)
            }
            ZLIB => {
                if raw_len > MAX_COMPRESS_BYTES {
                    return Err(invalid("compressed payload exceeds decode limit"));
                }
                let mut decoder = Decompress::new(true);
                let mut raw = Vec::new();
                loop {
                    let mut chunk = [0u8; 8192];
                    let before_in = decoder.total_in();
                    let before_out = decoder.total_out();
                    let status = decoder
                        .decompress(
                            &self.data[before_in as usize..],
                            &mut chunk,
                            FlushDecompress::None,
                        )
                        .map_err(|_| invalid("invalid zlib stream"))?;
                    let produced = (decoder.total_out() - before_out) as usize;
                    if raw.len() + produced > raw_len {
                        return Err(invalid("compressed payload length mismatch"));
                    }
                    raw.extend_from_slice(&chunk[..produced]);
                    if status == Status::StreamEnd {
                        if raw.len() != raw_len || decoder.total_in() != self.data.len() as u64 {
                            return Err(invalid(
                                "compressed payload length mismatch or trailing data",
                            ));
                        }
                        break;
                    }
                    if before_in == decoder.total_in() && produced == 0 {
                        return Err(invalid("incomplete zlib stream"));
                    }
                }
                Ok(serde_json::from_slice(&raw)?)
            }
            _ => Err(invalid("unsupported payload codec")),
        }
    }
}

fn invalid(message: &str) -> DnsError {
    DnsError::plugin(format!("query_recorder {message}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(padding: usize) -> Vec<u8> {
        let mut raw = br#"{"req_edns_json":null,"answers_json":[],"authorities_json":[],"additionals_json":[],"signature_json":[],"resp_edns_json":null}"#.to_vec();
        raw.resize(padding.max(raw.len()), b' ');
        raw
    }

    #[test]
    fn thresholds_and_lossless_fallback() {
        for (size, codec) in [
            (2047, RAW),
            (2048, ZLIB),
            (MAX_COMPRESS_BYTES, ZLIB),
            (MAX_COMPRESS_BYTES + 1, RAW),
        ] {
            let encoded = EncodedPayload::encode_json(snapshot(size), true).unwrap();
            assert_eq!(encoded.codec, codec);
            assert_eq!(
                encoded.decode().unwrap(),
                EncodedPayload::encode_json(snapshot(0), false)
                    .unwrap()
                    .decode()
                    .unwrap()
            );
        }
        // High entropy bytes exercise the compression policy independently of
        // JSON parsing.
        let mut state = 1u64;
        let raw: Vec<_> = (0..4096)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state as u8
            })
            .collect();
        let encoded = EncodedPayload::encode_json(raw.clone(), true).unwrap();
        assert_eq!(encoded.codec, RAW);
        assert_eq!(encoded.data, raw);
    }

    #[test]
    fn rejects_corruption_versions_lengths_and_trailing_data() {
        let valid = EncodedPayload::encode_json(snapshot(4096), true).unwrap();
        assert_eq!(valid.codec, ZLIB);
        for length in 0..valid.data.len() {
            let mut corrupt = valid.clone();
            corrupt.data.truncate(length);
            assert!(
                corrupt.decode().is_err(),
                "accepted truncated stream at {length}"
            );
        }
        for index in 0..valid.data.len() {
            let mut corrupt = valid.clone();
            corrupt.data[index] ^= 0xFF;
            assert!(corrupt.decode().is_err(), "accepted corruption at {index}");
        }
        let mut corrupt = valid.clone();
        corrupt.data.push(0);
        assert!(corrupt.decode().is_err());
        let mut corrupt = valid.clone();
        corrupt.data.extend_from_slice(&valid.data);
        assert!(corrupt.decode().is_err());
        for len in [-1, 0, 4095, 4097, MAX_COMPRESS_BYTES as i64 + 1] {
            let mut corrupt = valid.clone();
            corrupt.raw_len = len;
            assert!(corrupt.decode().is_err());
        }
        let mut corrupt = valid.clone();
        corrupt.version = 2;
        assert!(corrupt.decode().is_err());
        let mut corrupt = valid;
        corrupt.codec = 2;
        assert!(corrupt.decode().is_err());
        let mut raw = EncodedPayload::encode_json(snapshot(0), false).unwrap();
        raw.raw_len += 1;
        assert!(raw.decode().is_err());
    }
}
