// SPDX-FileCopyrightText: 2026 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Wire-level tests for address-list recovery and mutation authorization.

use std::sync::Arc;
use std::time::Duration;

use mikrotik_rs::proto::codec::{Decode, decode_sentence, encode_terminator, encode_word};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use super::*;

type Row = HashMap<String, String>;
const OWNED_COMMENT: &str = "oxi;pg=test;kind=D";
const TEST_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy)]
enum Change {
    Set(&'static str, &'static str),
    Remove,
}

#[derive(Clone, Default)]
struct PeerState {
    rows: Vec<Row>,
    commands: Vec<Vec<String>>,
    change_before_recheck: Option<Change>,
    omit_list: bool,
}

impl PeerState {
    fn has_update_or_remove(&self) -> bool {
        self.commands
            .iter()
            .any(|words| words[0].ends_with("/set") || words[0].ends_with("/remove"))
    }

    fn handle(&mut self, words: &[String]) -> Vec<Vec<String>> {
        if words[0] == "/login" {
            return vec![vec!["!done".to_string()]];
        }
        self.commands.push(words.to_vec());
        let attributes: Row = words
            .iter()
            .filter_map(|word| word.strip_prefix('='))
            .filter_map(|word| word.split_once('='))
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        let mut replies = Vec::new();
        if words[0].ends_with("/print") {
            let queries = words
                .iter()
                .filter_map(|word| word.strip_prefix('?'))
                .filter_map(|word| word.split_once('='))
                .collect::<Vec<_>>();
            if queries.iter().any(|(key, _)| *key == ADDRESS_ID_FIELD)
                && let Some(change) = self.change_before_recheck.take()
            {
                match change {
                    Change::Set(key, value) => {
                        self.rows[0].insert(key.to_string(), value.to_string());
                    }
                    Change::Remove => self.rows.clear(),
                }
            }
            // Filters compare the stored property text. Equivalent address
            // spellings therefore disappear from address-filtered lookups,
            // while a full scan or an id lookup can still return the row.
            for row in &self.rows {
                if queries
                    .iter()
                    .all(|(key, value)| row.get(*key).map(String::as_str) == Some(*value))
                {
                    let mut reply = vec!["!re".to_string()];
                    reply.extend(
                        row.iter()
                            .filter(|(key, _)| {
                                !self.omit_list || key.as_str() != ADDRESS_LIST_FIELD
                            })
                            .map(|(key, value)| format!("={key}={value}")),
                    );
                    replies.push(reply);
                }
            }
        } else if words[0].ends_with("/add") {
            // Every seeded row represents the same host key; a successful
            // timeout-kind transition must remove it before the next insert.
            if self.rows.is_empty() {
                let mut row = attributes;
                row.insert(ADDRESS_ID_FIELD.to_string(), "*new".to_string());
                self.rows.push(row);
            } else {
                replies.push(vec![
                    "!trap".to_string(),
                    "=message=failure: already have such entry".to_string(),
                ]);
            }
        } else if words[0].ends_with("/set") {
            let id = attributes.get(ADDRESS_ID_FIELD).expect("update id");
            let row = self
                .rows
                .iter_mut()
                .find(|row| row.get(ADDRESS_ID_FIELD) == Some(id))
                .expect("live row to update");
            row.extend(attributes);
        } else if words[0].ends_with("/remove") {
            let id = attributes.get(ADDRESS_ID_FIELD).expect("remove id");
            self.rows
                .retain(|row| row.get(ADDRESS_ID_FIELD) != Some(id));
        } else {
            panic!("unexpected command: {words:?}");
        }
        replies.push(vec!["!done".to_string()]);
        replies
    }
}

struct FakeRouterOs {
    client: MikrotikRsClient,
    state: Arc<Mutex<PeerState>>,
    task: Option<JoinHandle<()>>,
}

impl FakeRouterOs {
    async fn start(state: PeerState) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fake RouterOS");
        let address = listener.local_addr().expect("peer address").to_string();
        let state = Arc::new(Mutex::new(state));
        let peer_state = state.clone();
        let task = tokio::spawn(async move {
            tokio::time::timeout(TEST_TIMEOUT, async move {
                let (mut stream, _) = listener.accept().await.expect("accept API client");
                let mut buffer = Vec::new();
                loop {
                    let words = loop {
                        if let Decode::Complete {
                            value,
                            bytes_consumed,
                        } = decode_sentence(&buffer).expect("decode API command")
                        {
                            let words = value
                                .words()
                                .map(|word| String::from_utf8(word.to_vec()).expect("UTF-8 word"))
                                .collect::<Vec<_>>();
                            buffer.drain(..bytes_consumed);
                            break words;
                        }
                        if stream.read_buf(&mut buffer).await.expect("read command") == 0 {
                            return;
                        }
                    };
                    let tag = words.iter().find(|word| word.starts_with(".tag="));
                    let replies = peer_state.lock().await.handle(&words);
                    let mut bytes = Vec::new();
                    for reply in replies {
                        for word in reply.iter().chain(tag) {
                            encode_word(word.as_bytes(), &mut bytes);
                        }
                        encode_terminator(&mut bytes);
                    }
                    stream.write_all(&bytes).await.expect("write API response");
                }
            })
            .await
            .expect("fake API deadline");
        });
        let client = MikrotikRsClient::new(
            RouterOsConnectionConfig::new(
                address,
                "test".to_string(),
                "dummy".to_string(),
                RouterOsTimeouts::from_secs(1, 1, 1),
                None,
            )
            .expect("peer connection config"),
        );
        Self {
            client,
            state,
            task: Some(task),
        }
    }

    async fn finish(mut self) -> PeerState {
        let task = self.task.take().expect("peer task");
        task.abort();
        if let Err(error) = task.await {
            assert!(error.is_cancelled(), "fake API failed: {error}");
        }
        self.state.lock().await.clone()
    }
}

impl Drop for FakeRouterOs {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

fn owned_row(address: &str, timeout: Option<&str>) -> Row {
    let mut row = Row::from([
        (ADDRESS_ID_FIELD.to_string(), "*1".to_string()),
        (ADDRESS_LIST_FIELD.to_string(), "policy".to_string()),
        (ADDRESS_FIELD.to_string(), address.to_string()),
        (COMMENT_FIELD.to_string(), OWNED_COMMENT.to_string()),
    ]);
    if let Some(timeout) = timeout {
        row.insert(TIMEOUT_FIELD.to_string(), timeout.to_string());
    }
    row
}

fn key(ip: &str) -> AddressListKey {
    AddressListKey::new(ip.parse().expect("host IP"), "policy".to_string())
}

async fn upsert(
    client: &MikrotikRsClient,
    key: &AddressListKey,
    timeout: Option<&str>,
) -> Result<Option<()>> {
    tokio::time::timeout(
        TEST_TIMEOUT,
        client.upsert_owned_entry(
            key,
            timeout,
            OWNED_COMMENT,
            "oxi",
            "test",
            timeout.is_some(),
        ),
    )
    .await
    .expect("upsert deadline")
}

async fn cleanup(client: &MikrotikRsClient) -> Result<bool> {
    let expected = RouterListEntry {
        id: "*1".to_string(),
        key: key("2001:db8::1"),
        timeout: Some("300s".to_string()),
        comment: Some(OWNED_COMMENT.to_string()),
    };
    tokio::time::timeout(TEST_TIMEOUT, client.delete_entry_if_matches(&expected))
        .await
        .expect("cleanup deadline")
}

#[tokio::test]
async fn upsert_refreshes_owned_rows_with_canonical_or_alternate_address_text() {
    for (ip, stored) in [
        ("192.0.2.10", "192.0.2.10/32"),
        ("2001:db8::1", "2001:db8::1"),
        ("2001:db8::1", "2001:db8::1/128"),
    ] {
        let peer = FakeRouterOs::start(PeerState {
            rows: vec![owned_row(stored, Some("299s"))],
            ..Default::default()
        })
        .await;
        let result = upsert(&peer.client, &key(ip), Some("300s")).await;
        let state = peer.finish().await;
        assert!(matches!(result, Ok(Some(()))), "{result:?}");
        assert_eq!(state.rows.len(), 1);
        assert_eq!(state.rows[0][ADDRESS_ID_FIELD], "*1");
        assert_eq!(state.rows[0][TIMEOUT_FIELD], "300s");
        assert_eq!(state.rows[0][COMMENT_FIELD], OWNED_COMMENT);
    }
}

#[tokio::test]
async fn duplicate_recovery_can_switch_both_timeout_kinds() {
    for (existing, desired) in [(None, Some("300s")), (Some("299s"), None)] {
        let peer = FakeRouterOs::start(PeerState {
            rows: vec![owned_row("2001:db8::1", existing)],
            ..Default::default()
        })
        .await;
        let result = upsert(&peer.client, &key("2001:db8::1"), desired).await;
        let state = peer.finish().await;
        assert!(matches!(result, Ok(Some(()))), "{result:?}");
        assert_eq!(state.rows.len(), 1);
        assert_eq!(state.rows[0][ADDRESS_ID_FIELD], "*new");
        assert_eq!(
            state.rows[0].get(TIMEOUT_FIELD).map(String::as_str),
            desired
        );
        assert_eq!(state.rows[0][COMMENT_FIELD], OWNED_COMMENT);
    }
}

#[tokio::test]
async fn duplicate_recovery_removes_extra_owned_rows() {
    let first = owned_row("2001:db8::1", Some("299s"));
    let mut second = first.clone();
    second.insert(ADDRESS_ID_FIELD.to_string(), "*2".to_string());
    let peer = FakeRouterOs::start(PeerState {
        rows: vec![first, second],
        ..Default::default()
    })
    .await;
    let result = upsert(&peer.client, &key("2001:db8::1"), Some("300s")).await;
    let state = peer.finish().await;
    assert!(matches!(result, Ok(Some(()))), "{result:?}");
    assert_eq!(state.rows.len(), 1);
    assert_eq!(state.rows[0][ADDRESS_ID_FIELD], "*1");
    assert_eq!(state.rows[0][TIMEOUT_FIELD], "300s");
}

#[tokio::test]
async fn duplicate_recovery_preserves_foreign_rows() {
    let mut row = owned_row("2001:db8::1", Some("299s"));
    row.insert(COMMENT_FIELD.to_string(), "operator-owned".to_string());
    let peer = FakeRouterOs::start(PeerState {
        rows: vec![row.clone()],
        ..Default::default()
    })
    .await;
    let result = upsert(&peer.client, &key("2001:db8::1"), Some("300s")).await;
    let state = peer.finish().await;
    assert!(matches!(result, Ok(None)), "{result:?}");
    assert_eq!(state.rows, vec![row]);
    assert!(!state.has_update_or_remove());
}

const SNAPSHOT_CHANGES: &[Change] = &[
    Change::Set(COMMENT_FIELD, "operator-owned"),
    Change::Set(ADDRESS_FIELD, "2001:db8::2/128"),
    Change::Set(ADDRESS_LIST_FIELD, "other-policy"),
    Change::Set(ADDRESS_ID_FIELD, "*replacement"),
    Change::Remove,
];

#[tokio::test]
async fn duplicate_recovery_rejects_changed_or_missing_snapshots() {
    for &change in SNAPSHOT_CHANGES {
        let peer = FakeRouterOs::start(PeerState {
            rows: vec![owned_row("2001:db8::1", Some("299s"))],
            change_before_recheck: Some(change),
            omit_list: true,
            ..Default::default()
        })
        .await;
        let result = upsert(&peer.client, &key("2001:db8::1"), Some("300s")).await;
        let state = peer.finish().await;
        assert!(result.is_err(), "stale snapshot was accepted: {result:?}");
        assert!(!state.has_update_or_remove());
    }
}

#[tokio::test]
async fn cleanup_deletes_owned_rows_with_alternate_address_text() {
    let peer = FakeRouterOs::start(PeerState {
        rows: vec![owned_row("2001:db8::1", Some("299s"))],
        ..Default::default()
    })
    .await;
    let result = cleanup(&peer.client).await;
    let state = peer.finish().await;
    assert!(matches!(result, Ok(true)), "{result:?}");
    assert!(state.rows.is_empty());
}

#[tokio::test]
async fn cleanup_rejects_changed_or_missing_snapshots() {
    for &change in SNAPSHOT_CHANGES {
        let peer = FakeRouterOs::start(PeerState {
            rows: vec![owned_row("2001:db8::1", Some("299s"))],
            change_before_recheck: Some(change),
            omit_list: true,
            ..Default::default()
        })
        .await;
        let result = cleanup(&peer.client).await;
        let state = peer.finish().await;
        assert!(matches!(result, Ok(false)), "{result:?}");
        assert!(!state.has_update_or_remove());
    }
}

#[tokio::test]
async fn snapshot_revalidation_accepts_omitted_list_after_filtering() {
    let peer = FakeRouterOs::start(PeerState {
        rows: vec![owned_row("2001:db8::1", Some("299s"))],
        omit_list: true,
        ..Default::default()
    })
    .await;
    let result = upsert(&peer.client, &key("2001:db8::1"), Some("300s")).await;
    let state = peer.finish().await;
    assert!(matches!(result, Ok(Some(()))), "{result:?}");
    assert_eq!(state.rows[0][TIMEOUT_FIELD], "300s");
}
