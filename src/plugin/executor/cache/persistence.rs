// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Cache persistence helpers.

use std::sync::Arc;

use tokio::fs;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tracing::{info, warn};
use wincode::{SchemaRead, SchemaWrite};

use super::key::{CacheKey, EcsScopeDigest, normalize_domain_key};
use super::{
    CacheItem, CacheMap, clamp_persisted_cache_ttl, is_cache_disposition_valid,
    response_disposition_for_cache,
};
use crate::infra::cache::ttl::TtlCacheEntry;
use crate::infra::clock::AppClock;
use crate::infra::error::Result;
use crate::infra::system::file_len_if_exists;
use crate::proto::{DNSClass, Message, RecordType};

#[derive(Debug, SchemaRead, SchemaWrite)]
struct PersistedCacheEntry {
    domain: String,
    record_type: u16,
    dns_class: u16,
    do_bit: bool,
    cd_bit: bool,
    ecs_family: Option<u16>,
    ecs_source_prefix: Option<u8>,
    ecs_scope_prefix: Option<u8>,
    ecs_network: Option<Vec<u8>>,
    resp_bytes: Vec<u8>,
    cache_age_ms: u64,
    last_access_age_ms: u64,
    ttl: u32,
    remaining_ttl_ms: u64,
}

pub(super) async fn dump_cache_to_file(cache_map: &CacheMap, dump_path: &str) -> Result<()> {
    let encoded = dump_cache_to_bytes(cache_map)?;

    // Write-then-rename to avoid partially written cache dump on crash.
    let tmp_path = format!("{}.tmp", dump_path);
    let mut file = File::create(&tmp_path).await?;
    file.write_all(&encoded).await?;
    file.sync_all().await?;
    fs::rename(&tmp_path, dump_path).await?;
    Ok(())
}

pub(super) fn dump_cache_to_bytes(cache_map: &CacheMap) -> Result<Vec<u8>> {
    let now = AppClock::elapsed_millis();
    let mut entries: Vec<PersistedCacheEntry> = Vec::with_capacity(cache_map.len());

    for (key, item) in cache_map.iter_entries_cloned() {
        let TtlCacheEntry {
            value,
            cache_time_ms,
            expire_at_ms,
            last_access_ms,
        } = item;

        if expire_at_ms <= now {
            continue;
        }

        let remaining_ttl_ms = expire_at_ms.saturating_sub(now);
        if remaining_ttl_ms == 0 {
            continue;
        }

        if !value.is_validated() {
            let disposition = response_disposition_for_cache(&value.resp, &key);
            if !is_cache_disposition_valid(disposition) {
                continue;
            }
        }

        let resp_bytes = match value.resp.to_bytes() {
            Ok(bytes) => bytes,
            Err(e) => {
                warn!(
                    "Failed to encode cached DNS message for {}: {}",
                    key.domain, e
                );
                continue;
            }
        };

        let (ecs_family, ecs_source_prefix, ecs_scope_prefix, ecs_network) = match &key.ecs_scope {
            Some(ecs) => (
                Some(ecs.family),
                Some(ecs.source_prefix),
                Some(ecs.scope_prefix),
                Some(ecs.network[..usize::from(ecs.network_len.min(16))].to_vec()),
            ),
            None => (None, None, None, None),
        };

        entries.push(PersistedCacheEntry {
            domain: key.domain.clone(),
            record_type: u16::from(key.record_type),
            dns_class: u16::from(key.dns_class),
            do_bit: key.do_bit,
            cd_bit: key.cd_bit,
            ecs_family,
            ecs_source_prefix,
            ecs_scope_prefix,
            ecs_network,
            resp_bytes,
            cache_age_ms: now.saturating_sub(cache_time_ms),
            last_access_age_ms: now.saturating_sub(last_access_ms),
            ttl: value.ttl,
            remaining_ttl_ms,
        });
    }

    Ok(wincode::serialize(&entries)?)
}

fn parse_persisted_entries(data: &[u8]) -> Option<Vec<PersistedCacheEntry>> {
    if let Ok(entries) = wincode::deserialize::<Vec<PersistedCacheEntry>>(data) {
        return Some(entries);
    }
    None
}

fn to_cache_key(entry: &PersistedCacheEntry, ecs_in_key: bool) -> Option<CacheKey> {
    let domain = normalize_domain_key(&entry.domain);
    if domain.is_empty() {
        return None;
    }

    let ecs_scope = if ecs_in_key {
        match (
            entry.ecs_family,
            entry.ecs_source_prefix,
            entry.ecs_scope_prefix,
            entry.ecs_network.as_ref(),
        ) {
            (None, None, None, None) => None,
            (Some(family), Some(source_prefix), Some(scope_prefix), Some(network)) => {
                let mut digest = [0u8; 16];
                let network_len = network.len().min(digest.len()) as u8;
                digest[..usize::from(network_len)]
                    .copy_from_slice(&network[..usize::from(network_len)]);
                Some(EcsScopeDigest {
                    family,
                    source_prefix,
                    scope_prefix,
                    network_len,
                    network: digest,
                })
            }
            // Persisted ECS metadata must be complete; mixed Some/None implies
            // corrupt or partially-written entry and should be discarded.
            _ => return None,
        }
    } else {
        // Runtime config disabled ECS dimension; intentionally merge all
        // persisted ECS variants into one non-ECS cache key bucket.
        None
    };

    Some(CacheKey {
        domain,
        record_type: RecordType::from(entry.record_type),
        dns_class: DNSClass::from(entry.dns_class),
        do_bit: entry.do_bit,
        cd_bit: entry.cd_bit,
        ecs_scope,
    })
}

pub(super) async fn load_cache_from_file(
    cache_map: &CacheMap,
    dump_path: &str,
    ecs_in_key: bool,
) -> Result<()> {
    let Some(file_len) = file_len_if_exists(dump_path).await? else {
        return Ok(());
    };
    if file_len == 0 {
        return Ok(());
    }

    let data = fs::read(dump_path).await?;
    let loaded = load_cache_from_bytes(cache_map, &data, ecs_in_key, false)?;

    if loaded > 0 {
        info!("Loaded {} cache entries from {}", loaded, dump_path);
    }

    Ok(())
}

pub(super) fn load_cache_from_bytes(
    cache_map: &CacheMap,
    data: &[u8],
    ecs_in_key: bool,
    replace: bool,
) -> Result<usize> {
    let Some(entries) = parse_persisted_entries(data) else {
        warn!("Failed to deserialize cache dump, skip loading incompatible data");
        return Ok(0);
    };

    if replace {
        cache_map.clear();
    }

    let now = AppClock::elapsed_millis();
    let mut loaded = 0usize;
    for entry in entries {
        if entry.remaining_ttl_ms == 0 {
            continue;
        }

        let Some(key) = to_cache_key(&entry, ecs_in_key) else {
            continue;
        };

        let resp = match Message::from_bytes(&entry.resp_bytes) {
            Ok(resp) => resp,
            Err(e) => {
                warn!(
                    "Failed to parse cached DNS message for {}: {}",
                    entry.domain, e
                );
                continue;
            }
        };

        let disposition = response_disposition_for_cache(&resp, &key);
        if !is_cache_disposition_valid(disposition) {
            continue;
        }

        let (ttl, remaining_ttl_ms) = clamp_persisted_cache_ttl(
            &resp,
            &key,
            disposition,
            entry.ttl,
            entry.remaining_ttl_ms,
            entry.cache_age_ms,
        );
        if ttl == 0 || remaining_ttl_ms == 0 {
            continue;
        }

        let expire_time = now.saturating_add(remaining_ttl_ms);
        let cache_time = now.saturating_sub(entry.cache_age_ms);
        let fresh_until_ms = cache_time.saturating_add(u64::from(ttl) * 1000);
        let last_access_time = now.saturating_sub(entry.last_access_age_ms);

        cache_map.insert_or_update_with_meta(
            key,
            Arc::new(CacheItem::new_validated(resp, ttl, fresh_until_ms)),
            cache_time,
            expire_time,
            last_access_time,
        );
        loaded += 1;
    }

    Ok(loaded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::rdata::{CNAME, SOA};
    use crate::proto::{Name, Question, RData, Record};

    fn make_entry() -> PersistedCacheEntry {
        PersistedCacheEntry {
            domain: "WWW.Example.COM.".to_string(),
            record_type: u16::from(RecordType::AAAA),
            dns_class: u16::from(DNSClass::IN),
            do_bit: true,
            cd_bit: false,
            ecs_family: Some(2),
            ecs_source_prefix: Some(56),
            ecs_scope_prefix: Some(64),
            ecs_network: Some(vec![0x20, 0x01, 0x0D, 0xB8, 0, 0, 0, 0]),
            resp_bytes: vec![0, 1, 2],
            cache_age_ms: 10,
            last_access_age_ms: 5,
            ttl: 60,
            remaining_ttl_ms: 30_000,
        }
    }

    fn cname_only_response_bytes() -> Vec<u8> {
        let mut response = Message::new();
        response.set_rcode(crate::proto::Rcode::NoError);
        response.add_question(Question::new(
            Name::from_ascii("example.com.").unwrap(),
            RecordType::A,
            DNSClass::IN,
        ));
        response.add_answer(Record::from_rdata(
            Name::from_ascii("example.com.").unwrap(),
            60,
            RData::CNAME(CNAME(Name::from_ascii("target.example.com.").unwrap())),
        ));
        response.to_bytes().expect("response should encode")
    }

    fn cname_nodata_response_bytes() -> Vec<u8> {
        cname_nodata_response_bytes_with_ttls(60, 30)
    }

    fn cname_nodata_response_bytes_with_ttls(cname_ttl: u32, negative_ttl: u32) -> Vec<u8> {
        let mut response = Message::new();
        response.set_rcode(crate::proto::Rcode::NoError);
        response.add_question(Question::new(
            Name::from_ascii("example.com.").unwrap(),
            RecordType::A,
            DNSClass::IN,
        ));
        response.add_answer(Record::from_rdata(
            Name::from_ascii("example.com.").unwrap(),
            cname_ttl,
            RData::CNAME(CNAME(Name::from_ascii("target.example.com.").unwrap())),
        ));
        response.add_authority(Record::from_rdata(
            Name::from_ascii("example.com.").unwrap(),
            120,
            RData::SOA(SOA::new(
                Name::from_ascii("ns1.example.com.").unwrap(),
                Name::from_ascii("hostmaster.example.com.").unwrap(),
                1,
                3600,
                600,
                86400,
                negative_ttl,
            )),
        ));
        response.to_bytes().expect("response should encode")
    }

    #[test]
    fn test_parse_persisted_entries_returns_none_for_invalid_bytes() {
        let parsed = parse_persisted_entries(b"not a valid cache dump");

        assert!(parsed.is_none());
    }

    #[test]
    fn test_to_cache_key_normalizes_domain_and_preserves_ecs_when_enabled() {
        let entry = make_entry();

        let cache_key = to_cache_key(&entry, true).expect("cache key should be built");

        assert_eq!(cache_key.domain, "www.example.com");
        assert_eq!(cache_key.record_type, RecordType::AAAA);
        assert_eq!(cache_key.dns_class, DNSClass::IN);
        assert!(cache_key.do_bit);
        assert!(!cache_key.cd_bit);
        let ecs = cache_key.ecs_scope.expect("ecs should be present");
        assert_eq!(ecs.family, 2);
        assert_eq!(ecs.source_prefix, 56);
        assert_eq!(ecs.scope_prefix, 64);
        assert_eq!(ecs.network_len, 8);
        assert_eq!(&ecs.network[..8], &[0x20, 0x01, 0x0D, 0xB8, 0, 0, 0, 0]);
    }

    #[test]
    fn test_to_cache_key_drops_partial_ecs_metadata() {
        let mut entry = make_entry();
        entry.ecs_network = None;

        let cache_key = to_cache_key(&entry, true);

        assert_eq!(cache_key, None);
    }

    #[test]
    fn test_to_cache_key_discards_empty_normalized_domain() {
        let mut entry = make_entry();
        entry.domain = "   ".to_string();

        let cache_key = to_cache_key(&entry, false);

        assert_eq!(cache_key, None);
    }

    #[test]
    fn test_to_cache_key_omits_ecs_when_runtime_keying_is_disabled() {
        let entry = make_entry();

        let cache_key = to_cache_key(&entry, false).expect("cache key should be built");

        assert_eq!(cache_key.ecs_scope, None);
    }

    #[test]
    fn test_load_cache_skips_cname_only_address_entry() {
        AppClock::start();
        let cache_map = CacheMap::with_capacity(1);
        let mut entry = make_entry();
        entry.domain = "example.com.".to_string();
        entry.record_type = u16::from(RecordType::A);
        entry.ecs_family = None;
        entry.ecs_source_prefix = None;
        entry.ecs_scope_prefix = None;
        entry.ecs_network = None;
        entry.resp_bytes = cname_only_response_bytes();

        let data = wincode::serialize(&vec![entry]).expect("entry should serialize");
        let loaded =
            load_cache_from_bytes(&cache_map, &data, false, false).expect("load should succeed");

        assert_eq!(loaded, 0);
        assert_eq!(cache_map.len(), 0);
    }

    #[test]
    fn test_load_cache_keeps_cname_nodata_address_entry() {
        AppClock::start();
        let cache_map = CacheMap::with_capacity(1);
        let mut entry = make_entry();
        entry.domain = "example.com.".to_string();
        entry.record_type = u16::from(RecordType::A);
        entry.ecs_family = None;
        entry.ecs_source_prefix = None;
        entry.ecs_scope_prefix = None;
        entry.ecs_network = None;
        entry.resp_bytes = cname_nodata_response_bytes();

        let data = wincode::serialize(&vec![entry]).expect("entry should serialize");
        let loaded =
            load_cache_from_bytes(&cache_map, &data, false, false).expect("load should succeed");

        assert_eq!(loaded, 1);
        assert_eq!(cache_map.len(), 1);
    }

    #[test]
    fn test_load_cache_rejects_response_question_mismatched_with_key() {
        AppClock::start();
        let cache_map = CacheMap::with_capacity(1);
        let mut entry = make_entry();
        entry.domain = "other.example.com.".to_string();
        entry.record_type = u16::from(RecordType::A);
        entry.ecs_family = None;
        entry.ecs_source_prefix = None;
        entry.ecs_scope_prefix = None;
        entry.ecs_network = None;
        entry.resp_bytes = cname_nodata_response_bytes();

        let data = wincode::serialize(&vec![entry]).expect("entry should serialize");
        let loaded =
            load_cache_from_bytes(&cache_map, &data, false, false).expect("load should succeed");

        assert_eq!(loaded, 0);
        assert_eq!(cache_map.len(), 0);
    }

    #[test]
    fn test_load_cache_clamps_cname_nodata_to_cname_ttl() {
        AppClock::start();
        let cache_map = CacheMap::with_capacity(1);
        let mut entry = make_entry();
        entry.domain = "example.com.".to_string();
        entry.record_type = u16::from(RecordType::A);
        entry.ecs_family = None;
        entry.ecs_source_prefix = None;
        entry.ecs_scope_prefix = None;
        entry.ecs_network = None;
        entry.ttl = 30;
        entry.remaining_ttl_ms = 30_000;
        entry.cache_age_ms = 2_000;
        entry.resp_bytes = cname_nodata_response_bytes_with_ttls(5, 30);

        let data = wincode::serialize(&vec![entry]).expect("entry should serialize");
        let loaded =
            load_cache_from_bytes(&cache_map, &data, false, false).expect("load should succeed");
        let stored = cache_map
            .iter_entries_cloned()
            .into_iter()
            .next()
            .expect("entry should be loaded")
            .1;

        assert_eq!(loaded, 1);
        assert_eq!(stored.value.ttl, 5);
        assert!(
            stored
                .expire_at_ms
                .saturating_sub(AppClock::elapsed_millis())
                <= 3_000
        );
    }

    #[test]
    fn test_load_cache_skips_negative_entry_expired_by_clamped_ttl() {
        AppClock::start();
        let cache_map = CacheMap::with_capacity(1);
        let mut entry = make_entry();
        entry.domain = "example.com.".to_string();
        entry.record_type = u16::from(RecordType::A);
        entry.ecs_family = None;
        entry.ecs_source_prefix = None;
        entry.ecs_scope_prefix = None;
        entry.ecs_network = None;
        entry.ttl = 30;
        entry.remaining_ttl_ms = 10_000;
        entry.cache_age_ms = 20_000;
        entry.resp_bytes = cname_nodata_response_bytes_with_ttls(5, 30);

        let data = wincode::serialize(&vec![entry]).expect("entry should serialize");
        let loaded =
            load_cache_from_bytes(&cache_map, &data, false, false).expect("load should succeed");

        assert_eq!(loaded, 0);
        assert_eq!(cache_map.len(), 0);
    }
}
