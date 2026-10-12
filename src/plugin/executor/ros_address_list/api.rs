//! RouterOS API adapter for ros_address_list executor.
//!
//! This module isolates all RouterOS address-list command paths and response
//! decoding so manager logic does not depend on `mikrotik-rs` protocol details.
//! The business layer only sees normalized address-list keys, ownership-aware
//! upsert behavior, and stable plugin errors.

use std::collections::HashMap;
use std::fmt::Debug;

use async_trait::async_trait;
use mikrotik_rs::{Command, CommandBuilder};
use tracing::debug;

use super::model::{AddressListFamily, AddressListKey, decode_owned_comment, parse_router_address};
use crate::infra::error::{DnsError, Result};
use crate::plugin::executor::routeros::batching::join_all_bounded;
use crate::plugin::executor::routeros::transport::{
    RouterOsConnectionConfig, RouterOsEvent, RouterOsResult, RouterOsTimeouts, RouterOsTransport,
    RouterOsTransportSnapshot,
};

/// RouterOS field containing the internal row id.
const ADDRESS_ID_FIELD: &str = ".id";
/// RouterOS field containing the address-list name.
const ADDRESS_LIST_FIELD: &str = "list";
/// RouterOS field containing the IP or CIDR value.
const ADDRESS_FIELD: &str = "address";
/// RouterOS field containing the native timeout string.
const TIMEOUT_FIELD: &str = "timeout";
/// RouterOS field containing ownership metadata.
const COMMENT_FIELD: &str = "comment";
const ADDRESS_LIST_PROPLIST: &str = ".id,list,address,timeout,comment";
const MUTATION_PIPELINE_SIZE: usize = 16;

/// RouterOS command for listing IPv4 firewall address-list rows.
const COMMAND_IP_ADDRESS_LIST_PRINT: &str = "/ip/firewall/address-list/print";
/// RouterOS command for creating IPv4 firewall address-list rows.
const COMMAND_IP_ADDRESS_LIST_ADD: &str = "/ip/firewall/address-list/add";
/// RouterOS command for updating IPv4 firewall address-list rows.
const COMMAND_IP_ADDRESS_LIST_SET: &str = "/ip/firewall/address-list/set";
/// RouterOS command for deleting IPv4 firewall address-list rows.
const COMMAND_IP_ADDRESS_LIST_REMOVE: &str = "/ip/firewall/address-list/remove";

/// RouterOS command for listing IPv6 firewall address-list rows.
const COMMAND_IPV6_ADDRESS_LIST_PRINT: &str = "/ipv6/firewall/address-list/print";
/// RouterOS command for creating IPv6 firewall address-list rows.
const COMMAND_IPV6_ADDRESS_LIST_ADD: &str = "/ipv6/firewall/address-list/add";
/// RouterOS command for updating IPv6 firewall address-list rows.
const COMMAND_IPV6_ADDRESS_LIST_SET: &str = "/ipv6/firewall/address-list/set";
/// RouterOS command for deleting IPv6 firewall address-list rows.
const COMMAND_IPV6_ADDRESS_LIST_REMOVE: &str = "/ipv6/firewall/address-list/remove";

/// Default timeout for establishing a RouterOS API connection.
pub(super) const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 5;
/// Default timeout for sending one RouterOS API command.
pub(super) const DEFAULT_SEND_TIMEOUT_SECS: u64 = 5;
/// Default timeout for receiving one chunk of RouterOS API response data.
pub(super) const DEFAULT_RECEIVE_TIMEOUT_SECS: u64 = 5;

pub(super) type MikrotikApiTimeouts = RouterOsTimeouts;

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) struct RouterListEntry {
    /// RouterOS internal row id (for example `*123`).
    pub(super) id: String,
    /// Normalized key reconstructed from RouterOS list/address fields.
    pub(super) key: AddressListKey,
    /// Timeout string returned by RouterOS when present.
    pub(super) timeout: Option<String>,
    /// Comment field used for ownership checks and diagnostics.
    pub(super) comment: Option<String>,
}

#[async_trait]
pub(super) trait MikrotikApi: Debug + Send + Sync {
    /// Enter shutdown-cleanup mode, bypassing reconnect backoff while keeping
    /// per-operation timeouts.
    fn begin_shutdown_cleanup(&self) {}
    /// Transport health used by retry scheduling and metrics.
    async fn transport_snapshot(&self) -> Option<RouterOsTransportSnapshot> {
        None
    }
    /// List all entries from the configured IPv4/IPv6 address lists.
    async fn list_entries(
        &self,
        list4: Option<&str>,
        list6: Option<&str>,
    ) -> Result<Vec<RouterListEntry>>;
    /// Upsert one plugin-owned address-list entry.
    ///
    /// Returning `Ok(None)` means the key is already occupied by a row this
    /// plugin must not overwrite: a foreign entry, or a row that stays outside
    /// its ownership projection because RouterOS stores the address in a form
    /// the filtered lookup does not report. RouterOS rejecting an insert
    /// because an equivalent row already exists is an idempotent outcome and
    /// never surfaces as an error.
    async fn upsert_owned_entry(
        &self,
        key: &AddressListKey,
        timeout: Option<&str>,
        comment: &str,
        comment_prefix: &str,
        plugin_tag: &str,
        refresh_timeout: bool,
    ) -> Result<Option<()>>;
    /// Re-read one row and delete it only while the ownership-relevant
    /// snapshot still matches.
    async fn delete_entry_if_matches(&self, expected: &RouterListEntry) -> Result<bool>;
}

#[derive(Debug, Clone)]
struct RouterReply {
    attributes: HashMap<String, Option<String>>,
}

impl RouterReply {
    #[inline]
    fn get(&self, key: &str) -> Option<&str> {
        self.attributes.get(key).and_then(|v| v.as_deref())
    }

    fn require(&self, key: &str, action: &str) -> Result<String> {
        self.get(key).map(str::to_string).ok_or_else(|| {
            DnsError::plugin(format!(
                "ros_address_list {action} response missing '{key}'"
            ))
        })
    }
}

pub(super) struct MikrotikRsClient {
    transport: RouterOsTransport,
}

impl Debug for MikrotikRsClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MikrotikRsClient")
            .field("transport", &self.transport)
            .finish()
    }
}

impl MikrotikRsClient {
    pub(super) fn new(config: RouterOsConnectionConfig) -> Self {
        Self {
            transport: RouterOsTransport::new(config),
        }
    }

    async fn send_rows(&self, action: &str, command: Command) -> Result<Vec<RouterReply>> {
        // All network/protocol details are normalized into `DnsError::plugin`
        // here so the manager only sees semantic success/failure.
        self.send_rows_transport(action, command)
            .await
            .map_err(Into::into)
    }

    async fn send_rows_transport(
        &self,
        action: &str,
        command: Command,
    ) -> RouterOsResult<Vec<RouterReply>> {
        let mut stream = self.transport.send_command(action, command).await?;
        let mut rows = Vec::new();
        loop {
            match stream.next().await? {
                RouterOsEvent::Reply(attributes) => rows.push(RouterReply { attributes }),
                RouterOsEvent::Complete => return Ok(rows),
            }
        }
    }

    async fn find_entries_by_key(&self, key: &AddressListKey) -> Result<Vec<RouterListEntry>> {
        // RouterOS separates IPv4 and IPv6 address-lists into different command
        // namespaces, but the manager uses one normalized key type.
        let print = CommandBuilder::new()
            .command(address_list_command(key.family, ListOp::Print))
            .attribute(".proplist", Some(ADDRESS_LIST_PROPLIST))
            .query_equal(ADDRESS_LIST_FIELD, key.list.as_str())
            .query_equal(ADDRESS_FIELD, key.router_value().as_str())
            .build();
        let rows = self.send_rows("find address-list entries", print).await?;
        let mut entries = Vec::new();
        for row in rows {
            if let Some(entry) = parse_router_list_entry(
                "find address-list entries parse",
                key.family,
                &row,
                Some(key.list.as_str()),
            )? {
                entries.push(entry);
            }
        }
        Ok(entries)
    }

    async fn add_entry(
        &self,
        key: &AddressListKey,
        timeout: Option<&str>,
        comment: &str,
    ) -> RouterOsResult<()> {
        let mut add = CommandBuilder::new()
            .command(address_list_command(key.family, ListOp::Add))
            .attribute(ADDRESS_LIST_FIELD, Some(key.list.as_str()))
            .attribute(ADDRESS_FIELD, Some(key.router_value().as_str()))
            .attribute(COMMENT_FIELD, Some(comment));
        if let Some(timeout) = timeout {
            add = add.attribute(TIMEOUT_FIELD, Some(timeout));
        }
        // The typed transport error is kept so duplicate-insert traps can be
        // classified before they are flattened into a plugin error.
        let _ = self
            .send_rows_transport("add address-list entry", add.build())
            .await?;
        Ok(())
    }

    async fn remove_entry_by_id(&self, id: &str, family: AddressListFamily) -> Result<()> {
        let remove = CommandBuilder::new()
            .command(address_list_command(family, ListOp::Remove))
            .attribute(ADDRESS_ID_FIELD, Some(id))
            .build();
        match self
            .send_rows_transport("remove address-list entry", remove)
            .await
        {
            Ok(_) => Ok(()),
            Err(error) if error.is_missing_item() => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    async fn set_entry_if_matches(
        &self,
        expected: &RouterListEntry,
        timeout: Option<&str>,
        comment: &str,
    ) -> Result<bool> {
        let current = self.find_entries_by_key(&expected.key).await?;
        let Some(current) = current
            .into_iter()
            .find(|current| entry_matches_delete_snapshot(expected, current))
        else {
            return Ok(false);
        };
        let mut set = CommandBuilder::new()
            .command(address_list_command(current.key.family, ListOp::Set))
            .attribute(ADDRESS_ID_FIELD, Some(current.id.as_str()))
            .attribute(COMMENT_FIELD, Some(comment));
        if let Some(timeout) = timeout {
            set = set.attribute(TIMEOUT_FIELD, Some(timeout));
        }
        let _ = self
            .send_rows("set address-list entry", set.build())
            .await?;
        Ok(true)
    }

    async fn delete_entries_if_matches(&self, entries: Vec<RouterListEntry>) -> Result<()> {
        let results = join_all_bounded(
            entries
                .iter()
                .map(|entry| self.delete_entry_if_matches(entry)),
            MUTATION_PIPELINE_SIZE,
        )
        .await;
        for result in results {
            result?;
        }
        Ok(())
    }

    /// List every entry of one address-list namespace without an address
    /// filter.
    ///
    /// This is the full-scan form used by cold paths and by the duplicate
    /// insert recovery path, where the filtered lookup already proved unable to
    /// report the row RouterOS holds.
    async fn list_entries_for_list(
        &self,
        family: AddressListFamily,
        list: &str,
    ) -> Result<Vec<RouterListEntry>> {
        let (action, command) = match family {
            AddressListFamily::Ipv4 => (
                "print ipv4 address-list entries",
                address_list_command(AddressListFamily::Ipv4, ListOp::Print),
            ),
            AddressListFamily::Ipv6 => (
                "print ipv6 address-list entries",
                address_list_command(AddressListFamily::Ipv6, ListOp::Print),
            ),
        };
        let print = CommandBuilder::new()
            .command(command)
            .attribute(".proplist", Some(ADDRESS_LIST_PROPLIST))
            .query_equal(ADDRESS_LIST_FIELD, list)
            .build();
        let mut entries = Vec::new();
        for row in self.send_rows(action, print).await? {
            if let Some(entry) = parse_router_list_entry(action, family, &row, Some(list))? {
                entries.push(entry);
            }
        }
        Ok(entries)
    }

    /// Run one ownership-aware upsert attempt.
    async fn upsert_attempt(
        &self,
        request: &UpsertRequest<'_>,
        scope: LookupScope,
    ) -> Result<UpsertAttempt> {
        let UpsertRequest {
            key,
            timeout,
            comment,
            comment_prefix,
            plugin_tag,
            refresh_timeout,
        } = *request;
        // Upsert policy:
        // 1) collect all rows that belong to the exact `(family, list, address,
        //    prefix)` key
        // 2) refuse overwrite when only foreign rows exist
        // 3) deduplicate multiple owned rows down to one canonical row
        // 4) update the canonical row in place when safe
        // 5) recreate when switching between timed and persistent forms
        let candidates: Vec<RouterListEntry> = match scope {
            LookupScope::Filtered => self.find_entries_by_key(key).await?,
            LookupScope::ListScan => self
                .list_entries_for_list(key.family, key.list.as_str())
                .await?
                .into_iter()
                .filter(|entry| entry.key == *key)
                .collect(),
        };

        let mut owned = Vec::new();
        let mut has_foreign = false;
        for entry in candidates {
            if decode_owned_comment(comment_prefix, plugin_tag, entry.comment.as_deref()).is_some()
            {
                owned.push(entry);
            } else {
                has_foreign = true;
            }
        }

        if owned.is_empty() && has_foreign {
            return Ok(UpsertAttempt::Foreign);
        }

        let canonical = owned
            .iter()
            .position(|entry| entry.timeout.is_some() == timeout.is_some())
            .unwrap_or_default();
        let mut canonical = (!owned.is_empty()).then(|| owned.swap_remove(canonical));
        self.delete_entries_if_matches(owned).await?;

        if let Some(existing) = canonical.take() {
            // RouterOS timed and timeless rows are different enough that the
            // safest transition is delete-and-add when the timeout kind
            // changes.
            let timeout_kind_changed = existing.timeout.is_some() != timeout.is_some();
            if timeout_kind_changed {
                if !self.delete_entry_if_matches(&existing).await? {
                    return Err(DnsError::plugin(
                        "ros_address_list entry changed during timeout-kind transition",
                    ));
                }
                return self.insert_entry(request).await;
            }

            // `refresh_timeout` lets callers force a timeout rewrite even when
            // the string looks unchanged, which keeps the remote timer alive.
            let timeout_changed = existing.timeout.as_deref() != timeout;
            let comment_changed = existing.comment.as_deref() != Some(comment);
            if (refresh_timeout || timeout_changed || comment_changed)
                && !self
                    .set_entry_if_matches(&existing, timeout, comment)
                    .await?
            {
                return Err(DnsError::plugin(
                    "ros_address_list entry changed before update",
                ));
            }
            return Ok(UpsertAttempt::Applied);
        }

        self.insert_entry(request).await
    }

    /// Insert a row, reporting a RouterOS duplicate rejection as
    /// [`UpsertAttempt::HiddenDuplicate`] instead of an error.
    async fn insert_entry(&self, request: &UpsertRequest<'_>) -> Result<UpsertAttempt> {
        match self
            .add_entry(request.key, request.timeout, request.comment)
            .await
        {
            Ok(()) => Ok(UpsertAttempt::Applied),
            Err(error) if error.is_duplicate_entry() => Ok(UpsertAttempt::HiddenDuplicate),
            Err(error) => Err(error.into()),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum ListOp {
    Print,
    Add,
    Set,
    Remove,
}

/// How one upsert attempt attributes RouterOS rows to the target key.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum LookupScope {
    /// Exact `(list, address)` query, which is what the plugin writes.
    Filtered,
    /// Whole-list scan matched on normalized keys.
    ListScan,
}

/// Result of one ownership-aware upsert attempt.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum UpsertAttempt {
    /// The key already holds the desired plugin state, or it was written.
    Applied,
    /// A row this plugin does not own occupies the key; never overwrite it.
    Foreign,
    /// RouterOS refused the insert because an equivalent row exists that the
    /// lookup used for this attempt did not report.
    HiddenDuplicate,
}

/// One desired address-list row plus the ownership namespace that authorizes
/// managing it.
#[derive(Debug, Clone, Copy)]
struct UpsertRequest<'a> {
    key: &'a AddressListKey,
    timeout: Option<&'a str>,
    comment: &'a str,
    comment_prefix: &'a str,
    plugin_tag: &'a str,
    refresh_timeout: bool,
}

/// Map a normalized family/op pair to the RouterOS command namespace.
fn address_list_command(family: AddressListFamily, op: ListOp) -> &'static str {
    match (family, op) {
        (AddressListFamily::Ipv4, ListOp::Print) => COMMAND_IP_ADDRESS_LIST_PRINT,
        (AddressListFamily::Ipv4, ListOp::Add) => COMMAND_IP_ADDRESS_LIST_ADD,
        (AddressListFamily::Ipv4, ListOp::Set) => COMMAND_IP_ADDRESS_LIST_SET,
        (AddressListFamily::Ipv4, ListOp::Remove) => COMMAND_IP_ADDRESS_LIST_REMOVE,
        (AddressListFamily::Ipv6, ListOp::Print) => COMMAND_IPV6_ADDRESS_LIST_PRINT,
        (AddressListFamily::Ipv6, ListOp::Add) => COMMAND_IPV6_ADDRESS_LIST_ADD,
        (AddressListFamily::Ipv6, ListOp::Set) => COMMAND_IPV6_ADDRESS_LIST_SET,
        (AddressListFamily::Ipv6, ListOp::Remove) => COMMAND_IPV6_ADDRESS_LIST_REMOVE,
    }
}

fn parse_router_list_entry(
    action: &str,
    family: AddressListFamily,
    reply: &RouterReply,
    fallback_list: Option<&str>,
) -> Result<Option<RouterListEntry>> {
    // RouterOS may omit the list name in some filtered query paths, so callers
    // can provide the already-known list as a fallback.
    let id = reply.require(ADDRESS_ID_FIELD, action)?;
    let list = reply
        .get(ADDRESS_LIST_FIELD)
        .map(str::to_string)
        .or_else(|| fallback_list.map(str::to_string))
        .ok_or_else(|| {
            DnsError::plugin(format!(
                "ros_address_list {action} response missing '{ADDRESS_LIST_FIELD}'"
            ))
        })?;
    let address_raw = reply.require(ADDRESS_FIELD, action)?;
    let Some((address, prefix)) = parse_router_address(family, address_raw.as_str()) else {
        // RouterOS address lists also accept DNS names and other address
        // forms that are not IP/CIDR lease keys. They are outside this
        // plugin's projection and must not make a shared-list scan fail.
        return Ok(None);
    };
    let key = AddressListKey::new_with_prefix(address, prefix, list).ok_or_else(|| {
        DnsError::plugin(format!(
            "ros_address_list {action} response has invalid normalized address '{address_raw}'"
        ))
    })?;
    let timeout = reply.get(TIMEOUT_FIELD).map(str::to_string);
    let comment = reply.get(COMMENT_FIELD).map(str::to_string);
    Ok(Some(RouterListEntry {
        id,
        key,
        timeout,
        comment,
    }))
}

fn entry_matches_delete_snapshot(expected: &RouterListEntry, current: &RouterListEntry) -> bool {
    current.id == expected.id && current.key == expected.key && current.comment == expected.comment
}

#[async_trait]
impl MikrotikApi for MikrotikRsClient {
    fn begin_shutdown_cleanup(&self) {
        self.transport.begin_shutdown_cleanup();
    }

    async fn transport_snapshot(&self) -> Option<RouterOsTransportSnapshot> {
        Some(self.transport.snapshot().await)
    }

    /// List all entries from the configured IPv4/IPv6 address lists.
    async fn list_entries(
        &self,
        list4: Option<&str>,
        list6: Option<&str>,
    ) -> Result<Vec<RouterListEntry>> {
        // The manager asks for a full list scan only on relatively cold paths:
        // persistent reconcile, startup repair, and cleanup.
        let mut entries = Vec::new();

        if let Some(list4) = list4 {
            entries.extend(
                self.list_entries_for_list(AddressListFamily::Ipv4, list4)
                    .await?,
            );
        }

        if let Some(list6) = list6 {
            entries.extend(
                self.list_entries_for_list(AddressListFamily::Ipv6, list6)
                    .await?,
            );
        }

        Ok(entries)
    }

    async fn upsert_owned_entry(
        &self,
        key: &AddressListKey,
        timeout: Option<&str>,
        comment: &str,
        comment_prefix: &str,
        plugin_tag: &str,
        refresh_timeout: bool,
    ) -> Result<Option<()>> {
        let request = UpsertRequest {
            key,
            timeout,
            comment,
            comment_prefix,
            plugin_tag,
            refresh_timeout,
        };
        match self.upsert_attempt(&request, LookupScope::Filtered).await? {
            UpsertAttempt::Applied => Ok(Some(())),
            UpsertAttempt::Foreign => Ok(None),
            UpsertAttempt::HiddenDuplicate => {
                // RouterOS holds an equivalent row that the exact-key query did
                // not report. Stored addresses are normalized on insert (`/32`
                // and IPv6 host forms), filtered prints may omit rows, and
                // another writer can win the race between lookup and insert.
                // Re-read the whole list and match on normalized keys before
                // accepting the insert as already satisfied.
                debug!(
                    list = %key.list,
                    address = %key.normalized_value(),
                    "ros_address_list insert reported an existing entry, rescanning the list"
                );
                match self.upsert_attempt(&request, LookupScope::ListScan).await? {
                    UpsertAttempt::Applied => Ok(Some(())),
                    // The key is occupied but stays outside this plugin's
                    // ownership projection. The policy goal is already met, so
                    // report it as an occupied key rather than a failure.
                    UpsertAttempt::Foreign | UpsertAttempt::HiddenDuplicate => Ok(None),
                }
            }
        }
    }

    async fn delete_entry_if_matches(&self, expected: &RouterListEntry) -> Result<bool> {
        let current = self.find_entries_by_key(&expected.key).await?;
        let Some(current) = current
            .into_iter()
            .find(|current| entry_matches_delete_snapshot(expected, current))
        else {
            return Ok(false);
        };
        self.remove_entry_by_id(&current.id, current.key.family)
            .await?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(address: Option<&str>) -> RouterReply {
        RouterReply {
            attributes: HashMap::from([
                (ADDRESS_ID_FIELD.to_string(), Some("*1".to_string())),
                (ADDRESS_LIST_FIELD.to_string(), Some("policy".to_string())),
                (ADDRESS_FIELD.to_string(), address.map(ToString::to_string)),
                (
                    COMMENT_FIELD.to_string(),
                    Some("operator-owned".to_string()),
                ),
            ]),
        }
    }

    /// RouterOS stores host addresses in mask form for some inputs. The
    /// duplicate-insert recovery path relies on such rows normalizing back to
    /// the plugin's own host key.
    #[test]
    fn list_parser_normalizes_host_mask_addresses_to_host_keys() {
        for (family, address, expected) in [
            (AddressListFamily::Ipv4, "192.0.2.10/32", "192.0.2.10"),
            (AddressListFamily::Ipv6, "2001:db8::1/128", "2001:db8::1"),
        ] {
            let entry =
                parse_router_list_entry("parse shared list", family, &reply(Some(address)), None)
                    .expect("host mask form is a projected entry")
                    .expect("parsed entry");
            assert_eq!(
                entry.key,
                AddressListKey::new(
                    expected.parse().expect("expected address"),
                    "policy".to_string()
                )
            );
        }
    }

    #[test]
    fn list_parser_skips_supported_routeros_non_ip_addresses() {
        let parsed = parse_router_list_entry(
            "parse shared list",
            AddressListFamily::Ipv4,
            &reply(Some("example.com")),
            None,
        )
        .expect("DNS names are valid foreign RouterOS entries");

        assert!(parsed.is_none());
    }

    #[test]
    fn list_parser_keeps_missing_address_as_protocol_error() {
        let error = parse_router_list_entry(
            "parse shared list",
            AddressListFamily::Ipv4,
            &reply(None),
            None,
        )
        .expect_err("missing address field must remain an error");

        assert!(error.to_string().contains("missing 'address'"));
    }

    #[test]
    fn delete_snapshot_ignores_timeout_changes_but_rejects_ownership_change() {
        let key = AddressListKey::new("192.0.2.10".parse().expect("ip"), "policy".to_string());
        let expected = RouterListEntry {
            id: "*1".to_string(),
            key,
            timeout: Some("300s".to_string()),
            comment: Some("oxi;pg=test;kind=D".to_string()),
        };
        let mut current = expected.clone();
        current.timeout = Some("299s".to_string());
        assert!(entry_matches_delete_snapshot(&expected, &current));

        current.timeout = Some("301s".to_string());
        assert!(entry_matches_delete_snapshot(&expected, &current));

        current.timeout = Some("299s".to_string());
        current.comment = Some("operator-owned".to_string());
        assert!(!entry_matches_delete_snapshot(&expected, &current));
    }
}
