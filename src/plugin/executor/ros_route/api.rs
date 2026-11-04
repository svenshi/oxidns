// SPDX-FileCopyrightText: 2026 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! RouterOS API adapter for `ros_route`.
//!
//! This module isolates all RouterOS command paths and response decoding so
//! manager logic does not depend on `mikrotik-rs` protocol details.
//! Business layer only sees strongly-typed route snapshots and idempotent APIs.

use std::collections::HashMap;
use std::fmt::Debug;
use std::net::IpAddr;

use ahash::AHashSet;
use async_trait::async_trait;
use mikrotik_rs::{Command, CommandBuilder, QueryOperator};
use tracing::warn;

use super::model::{RouteCommentCodec, RouteFamily, RouteKey};
use crate::infra::error::{DnsError, Result};
use crate::plugin::executor::routeros::batching::join_all_bounded;
use crate::plugin::executor::routeros::transport::{
    RouterOsConnectionConfig, RouterOsEvent, RouterOsResult, RouterOsTimeouts, RouterOsTransport,
    RouterOsTransportSnapshot,
};

const ROUTER_ID_FIELD: &str = ".id";
const ROUTE_DST_FIELD: &str = "dst-address";
const ROUTE_TABLE_FIELD: &str = "routing-table";
const ROUTE_GATEWAY_FIELD: &str = "gateway";
const ROUTE_DISTANCE_FIELD: &str = "distance";
const ROUTE_COMMENT_FIELD: &str = "comment";
const ROUTE_DISABLED_FIELD: &str = "disabled";
const CONNECTION_DST_FIELD: &str = "dst-address";
const ROUTE_PROPLIST: &str = ".id,dst-address,routing-table,gateway,distance,comment,disabled";
const MUTATION_PIPELINE_SIZE: usize = 16;

const COMMAND_IP_ROUTE_PRINT: &str = "/ip/route/print";
const COMMAND_IP_ROUTE_ADD: &str = "/ip/route/add";
const COMMAND_IP_ROUTE_SET: &str = "/ip/route/set";
const COMMAND_IP_ROUTE_REMOVE: &str = "/ip/route/remove";

const COMMAND_IPV6_ROUTE_PRINT: &str = "/ipv6/route/print";
const COMMAND_IPV6_ROUTE_ADD: &str = "/ipv6/route/add";
const COMMAND_IPV6_ROUTE_SET: &str = "/ipv6/route/set";
const COMMAND_IPV6_ROUTE_REMOVE: &str = "/ipv6/route/remove";

const COMMAND_IP_FIREWALL_CONNECTION_PRINT: &str = "/ip/firewall/connection/print";
const COMMAND_IPV6_FIREWALL_CONNECTION_PRINT: &str = "/ipv6/firewall/connection/print";

pub(super) const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 5;
pub(super) const DEFAULT_SEND_TIMEOUT_SECS: u64 = 5;
pub(super) const DEFAULT_RECEIVE_TIMEOUT_SECS: u64 = 5;

pub(super) type MikrotikApiTimeouts = RouterOsTimeouts;

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) struct RouterRoute {
    /// RouterOS internal route identifier (e.g. `*123`).
    pub(super) id: String,
    /// Address family inferred by command namespace (`/ip/route` or
    /// `/ipv6/route`).
    pub(super) family: RouteFamily,
    /// Destination address in RouterOS format (`a.b.c.d/32` or `x::y/128`).
    pub(super) dst_address: String,
    /// Routing table name where the route lives.
    pub(super) routing_table: String,
    /// Optional gateway string from RouterOS.
    pub(super) gateway: Option<String>,
    /// Optional route distance from RouterOS.
    pub(super) distance: Option<u8>,
    /// Optional comment field from RouterOS.
    pub(super) comment: Option<String>,
    /// Whether RouterOS currently excludes this route from forwarding.
    pub(super) disabled: bool,
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
    /// List all routes in target table that can be considered by manager
    /// reconciliation.
    async fn list_managed_routes(
        &self,
        table: &str,
        require_ipv4: bool,
        require_ipv6: bool,
    ) -> Result<Vec<RouterRoute>>;
    /// Find all plugin-owned routes by route key (family + table +
    /// destination).
    async fn find_routes(
        &self,
        key: &RouteKey,
        comment_prefix: &str,
        plugin_tag: &str,
    ) -> Result<Vec<RouterRoute>>;
    /// Create or update one host route and return its RouterOS internal id.
    async fn upsert_host_route(
        &self,
        key: &RouteKey,
        gateway: &str,
        distance: u8,
        comment: &str,
        comment_prefix: &str,
        plugin_tag: &str,
    ) -> Result<String>;
    /// Re-read the RouterOS row and delete only when its id, route key, and
    /// ownership namespace still authorize the deletion.
    ///
    /// RouterOS exposes the final remove as an id-only command, not an atomic
    /// compare-and-delete operation. The plugin serializes all in-process
    /// writers for one ownership namespace; external writers must not mutate
    /// plugin-owned rows concurrently.
    async fn delete_route_if_matches(&self, expected: &RouterRoute) -> Result<bool>;
    /// Return tracked connection destinations for exact host IPs in one family.
    async fn connection_destinations(
        &self,
        family: RouteFamily,
        destinations: &[IpAddr],
    ) -> Result<AHashSet<IpAddr>>;
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
        self.get(key)
            .map(str::to_string)
            .ok_or_else(|| DnsError::plugin(format!("ros_route {action} response missing '{key}'")))
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
    async fn remove_route_by_id(
        &self,
        id: &str,
        family: RouteFamily,
        bypass_backoff: bool,
    ) -> Result<()> {
        let remove = CommandBuilder::new()
            .command(route_command(family, RouteOp::Remove))
            .attribute(ROUTER_ID_FIELD, Some(id))
            .build();
        match self
            .send_rows_transport("remove route", remove, bypass_backoff)
            .await
        {
            Ok(_) => Ok(()),
            Err(error) if error.is_missing_item() => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    pub(super) fn new(config: RouterOsConnectionConfig) -> Self {
        Self {
            transport: RouterOsTransport::new(config),
        }
    }

    async fn send_rows(&self, action: &str, command: Command) -> Result<Vec<RouterReply>> {
        self.send_rows_transport(action, command, false)
            .await
            .map_err(Into::into)
    }

    async fn send_rows_bypassing_backoff(
        &self,
        action: &str,
        command: Command,
    ) -> Result<Vec<RouterReply>> {
        self.send_rows_transport(action, command, true)
            .await
            .map_err(Into::into)
    }

    async fn send_rows_transport(
        &self,
        action: &str,
        command: Command,
        bypass_backoff: bool,
    ) -> RouterOsResult<Vec<RouterReply>> {
        let mut stream = if bypass_backoff {
            self.transport
                .send_command_bypassing_backoff(action, command)
                .await?
        } else {
            self.transport.send_command(action, command).await?
        };
        let mut rows = Vec::new();
        loop {
            match stream.next().await? {
                RouterOsEvent::Reply(attributes) => rows.push(RouterReply { attributes }),
                RouterOsEvent::Complete => return Ok(rows),
            }
        }
    }

    async fn find_route_by_exact_comment(
        &self,
        key: &RouteKey,
        comment: &str,
        bypass_backoff: bool,
    ) -> Result<Option<RouterRoute>> {
        let print = CommandBuilder::new()
            .command(route_command(key.family(), RouteOp::Print))
            .attribute(".proplist", Some(ROUTE_PROPLIST))
            .query_equal(ROUTE_TABLE_FIELD, &key.table)
            .query_equal(ROUTE_DST_FIELD, &key.dst_address())
            .query_equal(ROUTE_COMMENT_FIELD, comment)
            .build();
        let rows = if bypass_backoff {
            self.send_rows_bypassing_backoff("find route by comment", print)
                .await?
        } else {
            self.send_rows("find route by comment", print).await?
        };
        if let Some(row) = rows.into_iter().next() {
            let mut route =
                parse_router_route_from_reply("find route by comment parse", key.family(), &row)?;
            if route.routing_table.is_empty() {
                route.routing_table = key.table.clone();
            }
            return Ok(Some(route));
        }
        Ok(None)
    }

    async fn delete_route_if_matches_with_backoff(
        &self,
        expected: &RouterRoute,
        bypass_backoff: bool,
    ) -> Result<bool> {
        // Re-query by every stable key component before comparing the mutable
        // fields. Besides avoiding an accidental id-reuse deletion, filtering
        // by table also compensates for RouterOS versions that omit
        // `routing-table` from an id-only print reply.
        let print = CommandBuilder::new()
            .command(route_command(expected.family, RouteOp::Print))
            .attribute(".proplist", Some(ROUTE_PROPLIST))
            .query_equal(ROUTER_ID_FIELD, &expected.id)
            .query_equal(ROUTE_TABLE_FIELD, &expected.routing_table)
            .query_equal(ROUTE_DST_FIELD, &expected.dst_address)
            .build();
        let rows = if bypass_backoff {
            self.send_rows_bypassing_backoff("verify route before delete", print)
                .await?
        } else {
            self.send_rows("verify route before delete", print).await?
        };
        let Some(row) = rows.into_iter().next() else {
            return Ok(false);
        };
        let mut current = parse_router_route_from_reply(
            "verify route before delete parse",
            expected.family,
            &row,
        )?;
        if current.routing_table.is_empty() {
            current.routing_table.clone_from(&expected.routing_table);
        }
        if current.id != expected.id
            || current.family != expected.family
            || current.dst_address != expected.dst_address
            || current.routing_table != expected.routing_table
            || !same_ownership_namespace(current.comment.as_deref(), expected.comment.as_deref())
        {
            return Ok(false);
        }
        // The RouterOS API has no conditional remove primitive. This final
        // id-only command is safe against OxiDNS reload races because the
        // ownership namespace has one in-process writer. The fresh identity,
        // key, and ownership checks above guard against deleting a replaced or
        // repurposed row; operators must not concurrently edit owned rows.
        self.remove_route_by_id(&expected.id, expected.family, bypass_backoff)
            .await?;
        Ok(true)
    }

    async fn inspect_exact_routes(
        &self,
        key: &RouteKey,
        comment_prefix: &str,
        plugin_tag: &str,
    ) -> Result<ExactRouteOwnership> {
        let print = CommandBuilder::new()
            .command(route_command(key.family(), RouteOp::Print))
            .attribute(".proplist", Some(ROUTE_PROPLIST))
            .query_equal(ROUTE_TABLE_FIELD, &key.table)
            .query_equal(ROUTE_DST_FIELD, &key.dst_address())
            .build();
        let mut routes = Vec::new();
        for row in self.send_rows("find exact routes", print).await? {
            let mut route =
                parse_router_route_from_reply("find exact route parse", key.family(), &row)?;
            if route.routing_table.is_empty() {
                route.routing_table = key.table.clone();
            }
            routes.push(route);
        }
        Ok(classify_exact_routes(routes, comment_prefix, plugin_tag))
    }

    async fn prune_duplicate_owned_routes(&self, duplicates: Vec<RouterRoute>) -> Result<()> {
        let results = join_all_bounded(
            duplicates
                .iter()
                .map(|route| self.delete_route_if_matches(route)),
            MUTATION_PIPELINE_SIZE,
        )
        .await;
        for result in results {
            result?;
        }
        Ok(())
    }

    async fn set_route(
        &self,
        expected: &RouterRoute,
        gateway: &str,
        distance: u8,
        comment: &str,
    ) -> Result<bool> {
        let distance_str = distance.to_string();
        let set = CommandBuilder::new()
            .command(route_command(expected.family, RouteOp::Set))
            .attribute(ROUTER_ID_FIELD, Some(expected.id.as_str()))
            .attribute(ROUTE_GATEWAY_FIELD, Some(gateway))
            .attribute(ROUTE_DISTANCE_FIELD, Some(distance_str.as_str()))
            .attribute(ROUTE_COMMENT_FIELD, Some(comment))
            .attribute(ROUTE_DISABLED_FIELD, Some("no"))
            .build();
        let _ = self.send_rows("set host route", set).await?;
        Ok(true)
    }

    async fn list_routes_for_family(
        &self,
        table: &str,
        family: RouteFamily,
    ) -> Result<Vec<RouterRoute>> {
        let print = CommandBuilder::new()
            .command(route_command(family, RouteOp::Print))
            .attribute(".proplist", Some(ROUTE_PROPLIST))
            .query_equal(ROUTE_TABLE_FIELD, table)
            .build();
        let action = match family {
            RouteFamily::Ipv4 => "print ipv4 routes",
            RouteFamily::Ipv6 => "print ipv6 routes",
        };
        let parse_action = match family {
            RouteFamily::Ipv4 => "parse ipv4 route",
            RouteFamily::Ipv6 => "parse ipv6 route",
        };
        let rows = self.send_rows(action, print).await?;
        rows.iter()
            .map(|row| {
                let mut route = parse_router_route_from_reply(parse_action, family, row)?;
                if route.routing_table.is_empty() {
                    route.routing_table = table.to_string();
                }
                Ok(route)
            })
            .collect()
    }
}

fn insert_connection_destination(
    action: &str,
    family: RouteFamily,
    attributes: HashMap<String, Option<String>>,
    destinations: &mut AHashSet<IpAddr>,
) -> Result<()> {
    let raw = attributes
        .get(CONNECTION_DST_FIELD)
        .and_then(|value| value.as_deref())
        .ok_or_else(|| {
            DnsError::plugin(format!(
                "ros_route {action} response missing '{CONNECTION_DST_FIELD}'"
            ))
        })?;
    let ip = raw.parse::<IpAddr>().map_err(|error| {
        DnsError::plugin(format!(
            "ros_route {action} response has invalid '{CONNECTION_DST_FIELD}' '{raw}': {error}"
        ))
    })?;
    if RouteFamily::from_ip(ip) != family {
        return Err(DnsError::plugin(format!(
            "ros_route {action} response returned {ip} for the wrong address family"
        )));
    }
    destinations.insert(ip);
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum RouteOp {
    Print,
    Add,
    Set,
    Remove,
}

/// Build a connection-tracking read limited to destination addresses.
///
/// RouterOS has separate IPv4 and IPv6 connection tables. Exact destination
/// filters are combined with OR by the API query stack; an empty filter list
/// intentionally reads all destination addresses for CIDR containment checks.
fn connection_destinations_command(family: RouteFamily, destinations: &[IpAddr]) -> Command {
    let mut command = CommandBuilder::new()
        .command(connection_command(family))
        .attribute(".proplist", Some(CONNECTION_DST_FIELD));
    for ip in destinations {
        command = command.query_equal(CONNECTION_DST_FIELD, &ip.to_string());
    }
    if destinations.len() > 1 {
        command = command.query_operations(std::iter::repeat_n(
            QueryOperator::Or,
            destinations.len().saturating_sub(1),
        ));
    }
    command.build()
}

#[inline]
fn connection_command(family: RouteFamily) -> &'static str {
    match family {
        RouteFamily::Ipv4 => COMMAND_IP_FIREWALL_CONNECTION_PRINT,
        RouteFamily::Ipv6 => COMMAND_IPV6_FIREWALL_CONNECTION_PRINT,
    }
}

/// Map logical route operation to RouterOS command path by address family.
fn route_command(family: RouteFamily, op: RouteOp) -> &'static str {
    match (family, op) {
        (RouteFamily::Ipv4, RouteOp::Print) => COMMAND_IP_ROUTE_PRINT,
        (RouteFamily::Ipv4, RouteOp::Add) => COMMAND_IP_ROUTE_ADD,
        (RouteFamily::Ipv4, RouteOp::Set) => COMMAND_IP_ROUTE_SET,
        (RouteFamily::Ipv4, RouteOp::Remove) => COMMAND_IP_ROUTE_REMOVE,
        (RouteFamily::Ipv6, RouteOp::Print) => COMMAND_IPV6_ROUTE_PRINT,
        (RouteFamily::Ipv6, RouteOp::Add) => COMMAND_IPV6_ROUTE_ADD,
        (RouteFamily::Ipv6, RouteOp::Set) => COMMAND_IPV6_ROUTE_SET,
        (RouteFamily::Ipv6, RouteOp::Remove) => COMMAND_IPV6_ROUTE_REMOVE,
    }
}

/// Decode one RouterOS reply row into stable business route snapshot.
fn parse_router_route_from_reply(
    action: &str,
    family: RouteFamily,
    reply: &RouterReply,
) -> Result<RouterRoute> {
    let id = reply.require(ROUTER_ID_FIELD, action)?;
    let dst_address = reply.require(ROUTE_DST_FIELD, action)?;
    let routing_table = reply
        .get(ROUTE_TABLE_FIELD)
        .map(str::to_string)
        .unwrap_or_default();
    let gateway = reply.get(ROUTE_GATEWAY_FIELD).map(str::to_string);
    let distance = reply
        .get(ROUTE_DISTANCE_FIELD)
        .map(|raw| {
            raw.parse::<u8>().map_err(|e| {
                DnsError::plugin(format!(
                    "ros_route {action} response has invalid '{ROUTE_DISTANCE_FIELD}' value '{raw}': {e}"
                ))
            })
        })
        .transpose()?;
    let comment = reply.get(ROUTE_COMMENT_FIELD).map(str::to_string);
    let disabled = reply
        .get(ROUTE_DISABLED_FIELD)
        .map(|raw| parse_routeros_bool(raw, ROUTE_DISABLED_FIELD, action))
        .transpose()?
        .unwrap_or(false);

    Ok(RouterRoute {
        id,
        family,
        dst_address,
        routing_table,
        gateway,
        distance,
        comment,
        disabled,
    })
}

fn same_ownership_namespace(current: Option<&str>, expected: Option<&str>) -> bool {
    fn namespace(comment: &str) -> Option<(&str, &str)> {
        let mut parts = comment.split(';');
        let prefix = parts.next()?.trim();
        let plugin = parts.find_map(|part| part.trim().strip_prefix("pg="))?;
        (!prefix.is_empty() && !plugin.is_empty()).then_some((prefix, plugin))
    }

    match (current.and_then(namespace), expected.and_then(namespace)) {
        (Some(current), Some(expected)) => current == expected,
        _ => false,
    }
}

fn parse_routeros_bool(raw: &str, field: &str, action: &str) -> Result<bool> {
    if raw.eq_ignore_ascii_case("true") || raw.eq_ignore_ascii_case("yes") || raw == "1" {
        return Ok(true);
    }
    if raw.eq_ignore_ascii_case("false") || raw.eq_ignore_ascii_case("no") || raw == "0" {
        return Ok(false);
    }
    Err(DnsError::plugin(format!(
        "ros_route {action} response has invalid '{field}' value '{raw}'"
    )))
}

fn route_owned_by_plugin(route: &RouterRoute, comment_prefix: &str, plugin_tag: &str) -> bool {
    let Some(comment) = route.comment.as_deref() else {
        return false;
    };
    matches!(
        RouteCommentCodec::decode(
            comment_prefix,
            plugin_tag,
            route.family,
            &route.dst_address,
            comment,
        ),
        Ok(Some(_))
    )
}

#[derive(Debug, Default)]
struct ExactRouteOwnership {
    owned: Option<RouterRoute>,
    duplicate_owned: Vec<RouterRoute>,
    has_foreign: bool,
}

fn classify_exact_routes(
    routes: impl IntoIterator<Item = RouterRoute>,
    comment_prefix: &str,
    plugin_tag: &str,
) -> ExactRouteOwnership {
    let mut inspection = ExactRouteOwnership::default();
    for route in routes {
        if route_owned_by_plugin(&route, comment_prefix, plugin_tag) {
            if inspection.owned.is_some() {
                inspection.duplicate_owned.push(route);
            } else {
                inspection.owned = Some(route);
            }
        } else {
            inspection.has_foreign = true;
        }
    }
    inspection
}

#[async_trait]
impl MikrotikApi for MikrotikRsClient {
    fn begin_shutdown_cleanup(&self) {
        self.transport.begin_shutdown_cleanup();
    }

    async fn transport_snapshot(&self) -> Option<RouterOsTransportSnapshot> {
        Some(self.transport.snapshot().await)
    }

    async fn list_managed_routes(
        &self,
        table: &str,
        require_ipv4: bool,
        require_ipv6: bool,
    ) -> Result<Vec<RouterRoute>> {
        // RouterOS IPv4/IPv6 routes live in different namespaces. Disabled
        // families are still scanned when available so stale owned routes can
        // be removed, but an unavailable optional namespace must not prevent
        // the configured family from operating.
        let mut routes = Vec::new();
        for (family, required) in [
            (RouteFamily::Ipv4, require_ipv4),
            (RouteFamily::Ipv6, require_ipv6),
        ] {
            match self.list_routes_for_family(table, family).await {
                Ok(family_routes) => routes.extend(family_routes),
                Err(error) if !required => {
                    warn!(
                        ?family,
                        err = %error,
                        "ros_route optional RouterOS route family is unavailable"
                    );
                }
                Err(error) => return Err(error),
            }
        }

        Ok(routes)
    }

    async fn find_routes(
        &self,
        key: &RouteKey,
        comment_prefix: &str,
        plugin_tag: &str,
    ) -> Result<Vec<RouterRoute>> {
        let mut inspection = self
            .inspect_exact_routes(key, comment_prefix, plugin_tag)
            .await?;
        let mut routes = Vec::with_capacity(
            usize::from(inspection.owned.is_some()) + inspection.duplicate_owned.len(),
        );
        if let Some(route) = inspection.owned.take() {
            routes.push(route);
        }
        routes.append(&mut inspection.duplicate_owned);
        Ok(routes)
    }

    async fn upsert_host_route(
        &self,
        key: &RouteKey,
        gateway: &str,
        distance: u8,
        comment: &str,
        comment_prefix: &str,
        plugin_tag: &str,
    ) -> Result<String> {
        // Upsert strategy:
        // 1) inspect every exact-prefix row for owned and foreign routes
        // 2) reject any foreign duplicate before refreshing an owned row
        // 3) prune duplicate owned rows so one key converges to one route
        // 4) update changed/disabled fields, or add when no row exists
        let inspection = self
            .inspect_exact_routes(key, comment_prefix, plugin_tag)
            .await?;
        if inspection.has_foreign {
            return Err(DnsError::plugin(format!(
                "ros_route conflicts with a foreign route for {} in table '{}'",
                key.dst_address(),
                key.table
            )));
        }
        self.prune_duplicate_owned_routes(inspection.duplicate_owned)
            .await?;
        if let Some(existing) = inspection.owned {
            let gateway_changed = existing.gateway.as_deref() != Some(gateway);
            let distance_changed = existing.distance != Some(distance);
            let comment_changed = existing.comment.as_deref() != Some(comment);
            let disabled_changed = existing.disabled;
            if (gateway_changed || distance_changed || comment_changed || disabled_changed)
                && !self
                    .set_route(&existing, gateway, distance, comment)
                    .await?
            {
                return Err(DnsError::plugin("ros_route route changed before update"));
            }
            return Ok(existing.id);
        }

        let distance_str = distance.to_string();
        let add = CommandBuilder::new()
            .command(route_command(key.family(), RouteOp::Add))
            .attribute(ROUTE_DST_FIELD, Some(&key.dst_address()))
            .attribute(ROUTE_TABLE_FIELD, Some(&key.table))
            .attribute(ROUTE_GATEWAY_FIELD, Some(gateway))
            .attribute(ROUTE_DISTANCE_FIELD, Some(distance_str.as_str()))
            .attribute(ROUTE_COMMENT_FIELD, Some(comment))
            .build();
        let _ = self.send_rows("add host route", add).await?;

        let created = if let Some(route) = self
            .find_route_by_exact_comment(key, comment, false)
            .await?
        {
            route
        } else {
            self.find_routes(key, comment_prefix, plugin_tag)
                .await?
                .into_iter()
                .next()
                .ok_or_else(|| {
                    DnsError::plugin("ros_route upsert route succeeded but route id not found")
                })?
        };
        Ok(created.id)
    }

    async fn delete_route_if_matches(&self, expected: &RouterRoute) -> Result<bool> {
        self.delete_route_if_matches_with_backoff(expected, false)
            .await
    }

    async fn connection_destinations(
        &self,
        family: RouteFamily,
        destinations: &[IpAddr],
    ) -> Result<AHashSet<IpAddr>> {
        if destinations
            .iter()
            .any(|destination| RouteFamily::from_ip(*destination) != family)
        {
            return Err(DnsError::plugin(
                "ros_route connection destination filter has mixed address families",
            ));
        }
        let action = match family {
            RouteFamily::Ipv4 => "print ipv4 connection destinations",
            RouteFamily::Ipv6 => "print ipv6 connection destinations",
        };
        let command = connection_destinations_command(family, destinations);
        let mut stream = self.transport.send_command(action, command).await?;
        let mut found = AHashSet::new();
        loop {
            match stream.next().await? {
                RouterOsEvent::Reply(attributes) => {
                    insert_connection_destination(action, family, attributes, &mut found)?;
                }
                RouterOsEvent::Complete => return Ok(found),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(id: &str, comment: Option<&str>) -> RouterRoute {
        RouterRoute {
            id: id.to_string(),
            family: RouteFamily::Ipv4,
            dst_address: "203.0.113.20/32".to_string(),
            routing_table: "policy".to_string(),
            gateway: Some("192.0.2.1".to_string()),
            distance: Some(100),
            comment: comment.map(str::to_string),
            disabled: false,
        }
    }

    #[test]
    fn exact_route_inspection_reports_owned_and_foreign_duplicates() {
        let inspection = classify_exact_routes(
            [
                route("*owned", Some("fdns;pg=route-test;kind=D;exp=400;seen=100")),
                route(
                    "*owned-duplicate",
                    Some("fdns;pg=route-test;kind=D;exp=400;seen=100"),
                ),
                route("*foreign", Some("operator-managed")),
            ],
            "fdns",
            "route-test",
        );

        assert_eq!(
            inspection.owned.as_ref().map(|route| route.id.as_str()),
            Some("*owned")
        );
        assert_eq!(
            inspection
                .duplicate_owned
                .iter()
                .map(|route| route.id.as_str())
                .collect::<Vec<_>>(),
            vec!["*owned-duplicate"]
        );
        assert!(inspection.has_foreign);
    }

    #[test]
    fn route_without_ros_route_kind_is_foreign() {
        let reused_tag = route("*reused-tag", Some("fdns;pg=route-test;dm=operator-route"));

        assert!(!route_owned_by_plugin(&reused_tag, "fdns", "route-test"));
    }

    #[test]
    fn deletion_guard_accepts_payload_changes_only_within_the_same_owner_namespace() {
        let expected = "fdns;pg=route-test;kind=D;exp=400;seen=100";

        assert!(same_ownership_namespace(
            Some("fdns;pg=route-test;kind=D;exp=900;seen=500"),
            Some(expected),
        ));
        assert!(!same_ownership_namespace(
            Some("fdns;pg=another-plugin;kind=D;exp=900;seen=500"),
            Some(expected),
        ));
        assert!(!same_ownership_namespace(
            Some("operator-managed"),
            Some(expected),
        ));
    }

    #[test]
    fn route_parser_recognizes_routeros_disabled_values() {
        let reply = RouterReply {
            attributes: HashMap::from([
                (ROUTER_ID_FIELD.to_string(), Some("*disabled".to_string())),
                (
                    ROUTE_DST_FIELD.to_string(),
                    Some("203.0.113.20/32".to_string()),
                ),
                (ROUTE_DISABLED_FIELD.to_string(), Some("yes".to_string())),
            ]),
        };

        let route = parse_router_route_from_reply("test parse", RouteFamily::Ipv4, &reply)
            .expect("parse disabled route");

        assert!(route.disabled);
        assert!(parse_routeros_bool("true", ROUTE_DISABLED_FIELD, "test").unwrap());
        assert!(!parse_routeros_bool("false", ROUTE_DISABLED_FIELD, "test").unwrap());
        assert!(parse_routeros_bool("invalid", ROUTE_DISABLED_FIELD, "test").is_err());
    }

    #[test]
    fn connection_query_uses_family_specific_table_and_exact_or_filters() {
        let first: IpAddr = "203.0.113.10".parse().expect("ip");
        let second: IpAddr = "203.0.113.11".parse().expect("ip");
        let command = connection_destinations_command(RouteFamily::Ipv4, &[first, second]);
        let wire = String::from_utf8_lossy(command.data());

        assert!(wire.contains(COMMAND_IP_FIREWALL_CONNECTION_PRINT));
        assert!(wire.contains(".proplist=dst-address"));
        assert!(wire.contains("?dst-address=203.0.113.10"));
        assert!(wire.contains("?dst-address=203.0.113.11"));
        assert!(wire.contains("?#|"));

        let ipv6_addr: IpAddr = "2001:db8::1".parse().expect("ipv6");
        let ipv6 = connection_destinations_command(RouteFamily::Ipv6, &[ipv6_addr]);
        assert!(
            String::from_utf8_lossy(ipv6.data()).contains(COMMAND_IPV6_FIREWALL_CONNECTION_PRINT)
        );
    }

    #[test]
    fn connection_destinations_are_reduced_while_streaming() {
        let mut destinations = AHashSet::new();
        for _ in 0..2 {
            insert_connection_destination(
                "test connections",
                RouteFamily::Ipv4,
                HashMap::from([(
                    CONNECTION_DST_FIELD.to_string(),
                    Some("203.0.113.10".to_string()),
                )]),
                &mut destinations,
            )
            .expect("insert destination");
        }

        assert_eq!(
            destinations,
            AHashSet::from_iter(["203.0.113.10".parse().expect("ip")])
        );
    }
}
