import { zhCNPluginDefined } from "../zh-CN/plugin-defined";
import type { LocaleResourceShape } from "../../types";

export const enUSPluginDefined = {
  pluginTypes: {
    labels: {
      server: "Server",
      executor: "Executor",
      matcher: "Matcher",
      provider: "Provider",
    },
    descriptions: {
      server: "Listener service",
      executor: "Request processor",
      matcher: "Matcher",
      provider: "Data source",
    },
    statuses: {
      running: "Running",
      stopped: "Stopped",
      error: "Error",
    },
  },
  kinds: {
    udp_server: {
      name: "UDP Server",
      description:
        "Standard DNS UDP entry, handing the request to the specified executor",
      fields: {
        entry: {
          label: "Entry executor",
          description:
            "Specify the entry executor that handles all requests for this listener, usually the sequence plug-in.",
        },
        listen: {
          label: "listening address",
          description: "Specify the UDP listening address.",
          placeholder: "0.0.0.0:53",
        },
      },
      metrics: {
        labels: {
          server_request_total: "Total requests",
          server_completed_total: "Completed",
          server_controlled_total: "Stopped early",
          server_failed_total: "Failed (SERVFAIL)",
          server_inflight: "In flight",
          server_latency_count: "Latency samples",
          server_latency_sum_ms: "Total latency (ms)",
        },
        help: {
          server_request_total:
            "The total number of inbound DNS requests received and processed by the server.",
          server_completed_total:
            "The total number of requests that the executor chain has completed normally.",
          server_controlled_total:
            "The total number of requests that ended prematurely due to the executor actively stopping (stop/return).",
          server_failed_total:
            "The total number of requests that returned SERVFAIL due to entry executor failure.",
          server_inflight:
            "The number of requests currently being processed by the server.",
          server_latency_count:
            "The number of completed requests included in server latency statistics.",
          server_latency_sum_ms:
            "Total processing latency (milliseconds) of all completed requests.",
        },
        derived: {
          "latency:server": "Average latency",
          "percent:server_failed_total/server_request_total": "Failure rate",
        },
      },
    },
    tcp_server: {
      name: "TCP / DoT Server",
      description:
        "DNS over TCP; after configuring the certificate, it serves as the DoT entrance",
      fields: {
        entry: {
          label: "Entry executor",
          description:
            "Specifies the entry executor used when TCP or DoT requests enter the policy chain.",
        },
        listen: {
          label: "listening address",
          description: "Specify the TCP listening address.",
          placeholder: ":53",
        },
        cert: {
          label: "TLS certificate",
          description: "Specify the TLS certificate file path.",
          placeholder: "/etc/oxidns/server.crt",
        },
        key: {
          label: "TLS private key",
          description: "Specify the TLS private key file path.",
          placeholder: "/etc/oxidns/server.key",
        },
        idle_timeout: {
          label: "Idle timeout (seconds)",
          description: "Specify connection idle timeout settings.",
        },
      },
      metrics: {
        labels: {
          server_request_total: "Total requests",
          server_completed_total: "Completed",
          server_controlled_total: "Stopped early",
          server_failed_total: "Failed (SERVFAIL)",
          server_inflight: "In flight",
          server_latency_count: "Latency samples",
          server_latency_sum_ms: "Total latency (ms)",
        },
        help: {
          server_request_total:
            "The total number of inbound DNS requests received and processed by the server.",
          server_completed_total:
            "The total number of requests that the executor chain has completed normally.",
          server_controlled_total:
            "The total number of requests that ended prematurely due to the executor actively stopping (stop/return).",
          server_failed_total:
            "The total number of requests that returned SERVFAIL due to entry executor failure.",
          server_inflight:
            "The number of requests currently being processed by the server.",
          server_latency_count:
            "The number of completed requests included in server latency statistics.",
          server_latency_sum_ms:
            "Total processing latency (milliseconds) of all completed requests.",
        },
        derived: {
          "latency:server": "Average latency",
          "percent:server_failed_total/server_request_total": "Failure rate",
        },
      },
    },
    http_server: {
      name: "HTTP / DoH Server",
      description:
        "DNS over HTTPS, supports multi-entry mapping of paths to executors",
      fields: {
        entries: {
          label: "path mapping",
          description: "Define the mapping of HTTP paths to executors.",
          placeholder: '[{"path":"/dns-query","exec":"seq_main"}]',
        },
        "entries[]": {
          label: "path mapping",
        },
        "entries[].path": {
          label: "path",
          description: "Specify the DoH request path.",
          placeholder: "/dns-query",
        },
        "entries[].exec": {
          label: "Executor",
          description:
            "Specifies the executor that handles requests for this path.",
          placeholder: "seq_main",
        },
        listen: {
          label: "listening address",
          description: "Specify the HTTP/HTTPS listening address.",
          placeholder: ":443",
        },
        src_ip_header: {
          label: "Source IP Header",
          description:
            "Specifies the field name to read the real client source address from the request header.",
          placeholder: "X-Forwarded-For",
        },
        cert: {
          label: "HTTPS certificate",
          description: "Specify the HTTPS certificate file path.",
          placeholder: "/etc/oxidns/server.crt",
        },
        key: {
          label: "HTTPS private key",
          description: "Specify the HTTPS private key file path.",
          placeholder: "/etc/oxidns/server.key",
        },
        idle_timeout: {
          label: "Idle timeout (seconds)",
          description: "Specifies the HTTP connection idle timeout.",
        },
        enable_http3: {
          label: "Enable HTTP/3",
          description: "Specifies whether HTTP/3 is also enabled.",
        },
      },
      metrics: {
        labels: {
          server_request_total: "Total requests",
          server_completed_total: "Completed",
          server_controlled_total: "Stopped early",
          server_failed_total: "Failed (SERVFAIL)",
          server_inflight: "In flight",
          server_latency_count: "Latency samples",
          server_latency_sum_ms: "Total latency (ms)",
        },
        help: {
          server_request_total:
            "The total number of inbound DNS requests received and processed by the server.",
          server_completed_total:
            "The total number of requests that the executor chain has completed normally.",
          server_controlled_total:
            "The total number of requests that ended prematurely due to the executor actively stopping (stop/return).",
          server_failed_total:
            "The total number of requests that returned SERVFAIL due to entry executor failure.",
          server_inflight:
            "The number of requests currently being processed by the server.",
          server_latency_count:
            "The number of completed requests included in server latency statistics.",
          server_latency_sum_ms:
            "Total processing latency (milliseconds) of all completed requests.",
        },
        derived: {
          "latency:server": "Average latency",
          "percent:server_failed_total/server_request_total": "Failure rate",
        },
      },
    },
    quic_server: {
      name: "QUIC / DoQ Server",
      description: "DNS over QUIC portal",
      fields: {
        entry: {
          label: "Entry executor",
          description:
            "Specifies the entry executor used when DoQ requests enter the policy chain.",
        },
        listen: {
          label: "listening address",
          description: "Specify the QUIC listening address.",
          placeholder: ":853",
        },
        cert: {
          label: "TLS certificate",
          description: "Specify the TLS certificate file required by DoQ.",
          placeholder: "/etc/oxidns/server.crt",
        },
        key: {
          label: "TLS private key",
          description: "Specify the TLS private key file required by DoQ.",
          placeholder: "/etc/oxidns/server.key",
        },
        idle_timeout: {
          label: "Idle timeout (seconds)",
          description: "Specifies the idle timeout for the QUIC transport.",
        },
      },
      metrics: {
        labels: {
          server_request_total: "Total requests",
          server_completed_total: "Completed",
          server_controlled_total: "Stopped early",
          server_failed_total: "Failed (SERVFAIL)",
          server_inflight: "In flight",
          server_latency_count: "Latency samples",
          server_latency_sum_ms: "Total latency (ms)",
        },
        help: {
          server_request_total:
            "The total number of inbound DNS requests received and processed by the server.",
          server_completed_total:
            "The total number of requests that the executor chain has completed normally.",
          server_controlled_total:
            "The total number of requests that ended prematurely due to the executor actively stopping (stop/return).",
          server_failed_total:
            "The total number of requests that returned SERVFAIL due to entry executor failure.",
          server_inflight:
            "The number of requests currently being processed by the server.",
          server_latency_count:
            "The number of completed requests included in server latency statistics.",
          server_latency_sum_ms:
            "Total processing latency (milliseconds) of all completed requests.",
        },
        derived: {
          "latency:server": "Average latency",
          "percent:server_failed_total/server_request_total": "Failure rate",
        },
      },
    },
    sequence: {
      name: "Sequence",
      description:
        "Arrange matcher and executor in order, which is the most commonly used entry executor",
      fields: {
        args: {
          label: "chain of rules",
          description: "Define the chain of rules for the sequence.",
          placeholder: "$cache_main\nmatches: !$has_resp, exec: $forward_main",
        },
        "args[]": {
          label: "rule",
        },
        "args[].matches": {
          label: "Match condition",
          description: "Define the matching conditions for the current rule.",
          placeholder: "$has_resp\nqname domain:example.com\n!$blocked",
        },
        "args[].matches.$matcher_ref": {
          label: "Quote matcher",
          placeholder: "has_resp",
        },
        "args[].matches.$input": {
          label: "Enter value",
          placeholder: "qname domain:example.com",
        },
        "args[].exec": {
          label: "perform action",
          description:
            "Define the action to be performed when the rule is hit.",
          placeholder: "$forward_main / accept / reject 3 / jump seq_tag",
        },
      },
    },
    forward: {
      name: "Forward",
      description: "Initiate a query to one or more upstream DNS",
      fields: {
        concurrent: {
          label: "Number of concurrent upstreams",
          description:
            "Defines the number of concurrent query fanouts in multi-upstream mode.",
        },
        upstreams: {
          label: "upstream list",
          description: "Define one or more upstream targets.",
          placeholder: "udp://1.1.1.1:53",
        },
        "upstreams[].tag": {
          label: "upstream identifier",
          description:
            "Provide log identification for a single upstream to facilitate troubleshooting multi-upstream competition results.",
          placeholder: "cf_udp",
        },
        "upstreams[].addr": {
          label: "upstream address",
          description:
            "Define the upstream address, protocol type, and target host.",
          placeholder: "udp://1.1.1.1:53",
        },
        "upstreams[].dial_addr": {
          label: "Dial-up IP",
          description:
            "Specify the actual connection IP, while retaining the host name in addr for SNI, Host and certificate verification; this field takes precedence when configured at the same time as bootstrap.",
          placeholder: "203.0.113.53",
        },
        "upstreams[].port": {
          label: "Port coverage",
          description: "Override the protocol default port.",
          placeholder: "443",
        },
        "upstreams[].bootstrap": {
          label: "Bootstrap",
          description:
            "Provides a bootstrap resolver for domain-based upstreams. Must be IP:port; if omitted, system resolution is used on first connection; ignored when dial_addr is also configured.",
          placeholder: "8.8.8.8:53",
        },
        "upstreams[].bootstrap_version": {
          label: "Bootstrap IP version",
          description: "Specifies that bootstrap prefers IPv4 or IPv6.",
          options: {
            "4": "IPv4",
            "6": "IPv6",
          },
        },
        "upstreams[].socks5": {
          label: "SOCKS5 proxy",
          description: "Specify a SOCKS5 proxy for upstream connections.",
          placeholder: "user:pass@127.0.0.1:1080",
        },
        "upstreams[].idle_timeout": {
          label: "Connection idle timeout (seconds)",
          description:
            "Define the connection pool idle connection retention time.",
          placeholder: "30",
        },
        "upstreams[].max_conns": {
          label: "Maximum number of connections",
          description:
            "Define the upper limit of connection pool connections, in the range 1..4096.",
          placeholder: "256",
        },
        "upstreams[].min_conns": {
          label: "Minimum number of connections",
          description:
            "Define the minimum warmed connection count kept by the pool. Default is 0, range is 0..4096, and it must not exceed max_conns.",
          placeholder: "0",
        },
        "upstreams[].insecure_skip_verify": {
          label: "Skip TLS check",
          description:
            "Controls whether TLS certificate verification is skipped.",
        },
        "upstreams[].timeout": {
          label: "Query timeout",
          description: "Define a single upstream query timeout.",
          placeholder: "3s",
        },
        "upstreams[].enable_pipeline": {
          label: "Enable Pipeline",
          description: "Control the TCP or DoT pipeline.",
        },
        "upstreams[].enable_http3": {
          label: "Enable HTTP/3",
          description: "Controls whether DoH uses HTTP/3.",
        },
        "upstreams[].so_mark": {
          label: "SO_MARK",
          description: "Set Linux SO_MARK.",
          placeholder: "100",
        },
        "upstreams[].bind_to_device": {
          label: "Bind network card",
          description: "Set Linux SO_BINDTODEVICE.",
          placeholder: "eth0",
        },
        short_circuit: {
          label: "Stop subsequent execution after success",
          description:
            "Controls whether to stop the subsequent executor chain immediately after receiving a successful upstream response.",
        },
      },
      quickSetup: {
        paramPlaceholder: "1.1.1.1 short_circuit=true",
      },
      metrics: {
        labels: {
          forward_query_total: "Forwarded queries",
          forward_success_total: "Successful forwards",
          forward_error_total: "Forward failures",
          forward_timeout_total: "Forward timeouts",
          forward_latency_count: "Latency samples",
          forward_latency_sum_ms: "Total latency (ms)",
          forward_upstream_query_total: "Upstream queries",
          forward_upstream_success_total: "Upstream successes",
          forward_upstream_error_total: "Upstream failures",
          forward_upstream_timeout_total: "Upstream timeouts",
          forward_upstream_latency_count: "Upstream latency samples",
          forward_upstream_latency_sum_ms: "Upstream total latency (ms)",
        },
        help: {
          forward_query_total:
            "The total number of queries initiated by the forwarding executor.",
          forward_success_total:
            "The total number of queries that successfully obtained a response from the upstream.",
          forward_error_total:
            "The total number of queries for which the upstream returned an error or could not get a response.",
          forward_timeout_total:
            "The total number of queries that did not receive a response from the upstream due to timeout.",
          forward_latency_count:
            "The number of completed queries included in latency statistics.",
          forward_latency_sum_ms:
            "Total latency in milliseconds for all completed forwarded queries.",
          forward_upstream_query_total:
            "The total number of requests made to this upstream.",
          forward_upstream_success_total:
            "The number of successful responses returned by this upstream.",
          forward_upstream_error_total:
            "The number of times this upstream request failed.",
          forward_upstream_timeout_total:
            "The number of times this upstream request has timed out.",
          forward_upstream_latency_count:
            "The number of requests included in this upstream latency statistics.",
          forward_upstream_latency_sum_ms:
            "The total latency (in milliseconds) of all requests to this upstream.",
        },
        derived: {
          "percent:forward_success_total/forward_query_total": "Success rate",
          "latency:forward": "Average latency",
        },
      },
    },
    cache: {
      name: "Cache",
      description:
        "TTL-aware cache with negative caching, lazy cache, and persistence",
      fields: {
        size: {
          label: "Maximum entries",
          description: "Defines the maximum number of cache entries.",
        },
        lazy_cache_ttl: {
          label: "Lazy Cache TTL (seconds)",
          description: "Enable lazy cache for positive success responses.",
        },
        dump_file: {
          label: "Persistence file",
          description: "Specify the cache persistence file path.",
          placeholder: "./dns_cache.dump",
        },
        dump_interval: {
          label: "Dump interval (seconds)",
          description: "Defines how often the cache is flushed to disk.",
        },
        short_circuit: {
          label: "Stop subsequent execution after hit",
          description:
            "Controls whether to end subsequent execution immediately after a cache hit.",
        },
        cache_negative: {
          label: "Cache negative responses",
          description: "Controls whether NXDOMAIN and NODATA are cached.",
        },
        max_negative_ttl: {
          label: "Negative cache TTL cap",
          description: "Defines the negative cache TTL limit.",
        },
        negative_ttl_without_soa: {
          label: "No SOA negative cache TTL",
          description:
            "Defines the fallback TTL for negative responses without SOA.",
        },
        max_positive_ttl: {
          label: "Positive response TTL cap",
          description: "Defines the upper TTL limit for positive responses.",
        },
        ecs_in_key: {
          label: "Include ECS in cache key",
          description:
            "Controls whether the ECS scope is included in cache key calculation.",
        },
      },
      quickSetup: {
        paramPlaceholder: "short_circuit=true",
      },
      metrics: {
        labels: {
          cache_lookup_total: "Cache lookups",
          cache_hit_total: "Hits",
          cache_miss_total: "Misses",
          cache_expired_total: "Expired",
          cache_insert_total: "Writes",
          cache_skip_total: "Skipped",
          cache_lazy_refresh_total: "Lazy refresh",
          cache_entry_count: "Entries",
        },
        help: {
          cache_lookup_total:
            "The total number of cached queries with cacheable request keys.",
          cache_hit_total:
            "Total cache hits classified by freshness (fresh = direct hits, stale = stale available).",
          cache_miss_total: "The total number of cache misses for queries.",
          cache_expired_total:
            "The number of times expired entries were found and removed during a lookup.",
          cache_insert_total:
            "The total number of times cache entries have been inserted or updated.",
          cache_skip_total:
            "The total number of responses that were skipped from cache due to write policy (truncated responses, no TTL).",
          cache_lazy_refresh_total:
            "Total number of Lazy Cache background refresh attempts (by result: started / success / failed).",
          cache_entry_count: "The number of entries currently in the cache.",
        },
        derived: {
          "percent:cache_hit_total/cache_lookup_total": "Hit rate",
        },
      },
    },
    fallback: {
      name: "Fallback",
      description:
        "Switch to backup executor when primary path fails or is too slow",
      fields: {
        primary: {
          label: "Primary executor",
          description: "Specify the main executor.",
        },
        secondary: {
          label: "Secondary executor",
          description: "Specify the secondary executor.",
        },
        threshold: {
          label: "Takeover threshold (ms)",
          description:
            "Define the primary path timeout or delay determination threshold.",
        },
        always_standby: {
          label: "Alternate path on standby in parallel",
          description:
            "Controls whether the backup path is on standby at the same time as the primary path.",
        },
        short_circuit: {
          label: "Stop subsequent execution after success",
          description:
            "Controls whether to stop subsequent executor chains immediately after the primary/standby path selects the final response.",
        },
      },
      metrics: {
        labels: {
          fallback_primary_total: "main chain",
          fallback_primary_error_total: "Main chain failed",
          fallback_secondary_total: "Downgrade",
        },
        help: {
          fallback_primary_total:
            "The total number of times the main executor has been called.",
          fallback_primary_error_total:
            "The total number of times the main executor failed to respond.",
          fallback_secondary_total:
            "The total number of times the backup executor has been called.",
        },
        derived: {
          "percent:fallback_secondary_total/fallback_primary_total":
            "downgrade rate",
        },
      },
    },
    hosts: {
      name: "Hosts",
      description:
        "Directly return static A/AAAA according to domain name rules",
      fields: {
        entries: {
          label: "Inline hosts rules",
          description: "Define inline hosts rules.",
          placeholder:
            "router.local 192.168.1.1\nfull:gateway.local 192.168.1.2\ndomain:svc.local 10.0.0.10\nkeyword:nas 192.168.1.20\nregexp:^api[0-9]+\\.corp\\.local$ 10.10.0.5",
        },
        "entries[]": {
          label: "Enter value",
          placeholder: "router.local 192.168.1.1",
        },
        files: {
          label: "hosts file",
          description: "Specify a list of external hosts rule files.",
          placeholder: "/etc/oxidns/hosts.txt",
        },
        "files[]": {
          label: "Enter value",
          placeholder: "/etc/oxidns/hosts.txt",
        },
        short_circuit: {
          label: "Stop subsequent execution after hit",
          description:
            "Whether to stop the subsequent executor chain immediately after hitting and generating a local reply.",
        },
      },
      metrics: {
        labels: {
          hosts_hit_total: "hit",
          hosts_miss_total: "miss",
        },
        help: {
          hosts_hit_total:
            "The total number of times the hosts rule was hit and a local response was generated.",
          hosts_miss_total:
            "The total number of queries that did not hit any hosts rule.",
        },
        derived: {
          "percent_of_sum:hosts_hit_total/hosts_hit_total+hosts_miss_total":
            "hit rate",
        },
      },
    },
    arbitrary: {
      name: "Arbitrary",
      description: "Load static DNS records and construct responses on hits",
      fields: {
        rules: {
          label: "static record",
          description: "Define an inline static record list.",
          placeholder:
            'example.com. 60 IN TXT "hello world"\nwww.example.com. 120 IN A 192.0.2.10',
        },
        "rules[]": {
          label: "Enter value",
          placeholder: 'example.com. 60 IN TXT "hello world"',
        },
        files: {
          label: "log file",
          description: "Specify a list of static record files.",
          placeholder: "/etc/oxidns/zone.txt",
        },
        "files[]": {
          label: "Enter value",
          placeholder: "/etc/oxidns/zone.txt",
        },
        short_circuit: {
          label: "Stop subsequent execution after hit",
          description:
            "Whether to stop subsequent executor chains immediately after a hit and generating a local response.",
        },
      },
    },
    redirect: {
      name: "Redirect",
      description:
        "Rewrite the request domain name and use forward to generate the target response",
      fields: {
        rules: {
          label: "Redirect rules",
          description: "Define inline redirection rules.",
          placeholder:
            "full:old.example.com new.example.net\ndomain:legacy.example.com modern.example.net\nkeyword:staging staging-gateway.example.net\nregexp:^api[0-9]+\\.legacy\\.example\\.com$ api-gateway.example.net",
        },
        "rules[]": {
          label: "Enter value",
          placeholder: "full:old.example.com new.example.net",
        },
        files: {
          label: "rules file",
          description: "Specify a list of external redirection rule files.",
          placeholder: "/etc/oxidns/redirect.txt",
        },
        "files[]": {
          label: "Enter value",
          placeholder: "/etc/oxidns/redirect.txt",
        },
      },
    },
    ecs_handler: {
      name: "ECS Handler",
      description:
        "Handles retention, injection, and fallback cleanup of the EDNS Client Subnet",
      fields: {
        forward: {
          label: "Keep client ECS",
          description:
            "Controls whether existing ECSs in client requests are retained.",
        },
        send: {
          label: "Send ECS when missing",
          description:
            "Controls whether ECS is automatically replenished based on the source address when a request is missing ECS.",
        },
        preset: {
          label: "Default ECS address",
          description: "Specify a fixed ECS source address.",
          placeholder: "203.0.113.10",
        },
        mask4: {
          label: "IPv4 prefix length",
          description: "Specify the IPv4 ECS prefix length.",
        },
        mask6: {
          label: "IPv6 prefix length",
          description: "Specify the IPv6 ECS prefix length.",
        },
      },
      quickSetup: {
        paramPlaceholder: "203.0.113.10/24",
      },
    },
    forward_edns0opt: {
      name: "Forward EDNS0 Opt",
      description:
        "Forward the specified EDNS0 option from the request to the response",
      fields: {
        codes: {
          label: "Option Code",
          description:
            "Defines the set of EDNS0 option codes that are allowed to be copied from the request into the response.",
          placeholder: "10\n12",
        },
        "codes[]": {
          label: "Enter value",
          placeholder: "10",
        },
      },
      quickSetup: {
        paramPlaceholder: "10,12",
      },
    },
    ttl: {
      name: "TTL",
      description: "Fixed, raised or limited response TTL",
      fields: {
        fix: {
          label: "Fixed TTL",
          description: "Fix all response TTLs to the same value.",
        },
        min: {
          label: "TTL lower limit",
          description: "Define the lower TTL limit.",
        },
        max: {
          label: "TTL cap",
          description: "Define upper TTL limit.",
        },
      },
      quickSetup: {
        paramPlaceholder: "300 / 60-600",
      },
    },
    ip_selector: {
      name: "IP Selector",
      description:
        "Speed ​​test sorting and filtering of A/AAAA addresses in responses",
      fields: {
        selection_mode: {
          label: "preferred mode",
          description: "Define the response IP preference policy.",
          options: {
            first_success: "First success",
            best_within_budget: "Best within budget",
            background: "Background",
          },
        },
        probe_methods: {
          label: "Speed ​​measurement method",
          description:
            "Define the detection method used to score response IP, supporting tcp:<port>, ping, none.",
          placeholder: "tcp:443\ntcp:80",
        },
        "probe_methods[]": {
          label: "Enter value",
          placeholder: "tcp:443",
        },
        probe_stagger: {
          label: "Speed ​​peak deviation (ms)",
          description:
            "Staggered start intervals between multiple speed measurement methods.",
        },
        probe_timeout: {
          label: "Single timeout (ms)",
          description: "Single IP detection timeout.",
        },
        max_wait: {
          label: "Maximum wait(ms)",
          description: "The maximum length of time to wait for this response.",
        },
        top_n: {
          label: "Number of reserved addresses",
          description:
            "Keep the first N addresses after sorting; 0 means only rearrange without deletion.",
        },
        dnssec_policy: {
          label: "DNSSEC policy",
          description: "Policy for handling DNSSEC sensitive responses.",
          options: {
            reorder_only: "Reorder only",
            skip: "Skip",
          },
        },
        max_parallel_probes: {
          label: "Maximum concurrent detection",
          description:
            "The maximum number of concurrent probes at the plug-in level.",
        },
        cache: {
          label: "Rating cache",
          description: "Configure IP probe score caching.",
        },
        "cache.enabled": {
          label: "Enable caching",
          description: "Whether to enable probe score caching.",
        },
        "cache.size": {
          label: "cache capacity",
          description: "Cache capacity target.",
        },
        "cache.ttl": {
          label: "Success TTL (seconds)",
          description: "Success score retention time.",
        },
        "cache.failure_ttl": {
          label: "Failure TTL (seconds)",
          description: "Failure score retention time.",
        },
      },
      quickSetup: {
        paramPlaceholder: "best_within_budget tcp:443,tcp:80,ping",
      },
      metrics: {
        labels: {
          ip_selector_probe_total: "detection",
          ip_selector_probe_latency_count: "Latency samples",
          ip_selector_probe_latency_sum_ms: "Total latency (ms)",
          ip_selector_selected_total: "Preferred results",
          ip_selector_cache_entries: "cache entry",
          ip_selector_dropped_probe_total: "Skip detection",
        },
        help: {
          ip_selector_probe_total:
            "Number of IP probes counted by speed measurement method and results.",
          ip_selector_probe_latency_count:
            "The number of delay samples for successful speed measurement.",
          ip_selector_probe_latency_sum_ms:
            "Cumulative value of successful speed measurement delay (milliseconds).",
          ip_selector_selected_total:
            "The preferred number of statistics based on probe/cache/fallback sources.",
          ip_selector_cache_entries:
            "The current number of IP probe score cache entries.",
          ip_selector_dropped_probe_total:
            "The number of probes that were not newly launched due to concurrency limitations or existing in-flight probes.",
        },
      },
    },
    prefer_ipv4: {
      name: "Prefer IPv4",
      description:
        "Dual-stack optimizer that favors A records and suppresses alternative AAAA requests",
      fields: {
        cache: {
          label: "Cache preference status",
          description:
            "Controls whether preferred type presence status is cached.",
        },
        cache_ttl: {
          label: "Preference state cache TTL (seconds)",
          description: "Define preferred state cache duration.",
        },
      },
    },
    prefer_ipv6: {
      name: "Prefer IPv6",
      description:
        "Dual stack optimizer, favoring AAAA records and suppressing alternative A requests",
      fields: {
        cache: {
          label: "Cache preference status",
          description:
            "Controls whether preferred type presence status is cached.",
        },
        cache_ttl: {
          label: "Preference state cache TTL (seconds)",
          description: "Define preferred state cache duration.",
        },
      },
    },
    black_hole: {
      name: "Black Hole",
      description:
        "Generates full-qtype local interception responses by mode",
      fields: {
        mode: {
          label: "Interception mode",
          description:
            "Defines the interception response type; defaults to nxdomain without ips, and custom when ips are configured.",
          options: {
            nxdomain: "NXDOMAIN",
            nodata: "NODATA",
            null: "Null addresses",
            custom: "Custom addresses",
            refused: "REFUSED",
          },
        },
        ips: {
          label: "Custom return addresses",
          description:
            "Defines local synthetic return addresses used by custom mode.",
          placeholder: "0.0.0.0\n::",
        },
        "ips[]": {
          label: "Enter value",
          placeholder: "0.0.0.0",
        },
        short_circuit: {
          label: "Stop subsequent execution after hit",
          description:
            "Whether to stop the subsequent executor chain immediately after generating an interception response.",
        },
      },
      quickSetup: {
        paramPlaceholder: "nxdomain short_circuit=true",
      },
      metrics: {
        labels: {
          blackhole_block_total: "intercept",
        },
        help: {
          blackhole_block_total:
            "The total number of interception responses generated by black_hole.",
        },
      },
    },
    drop_resp: {
      name: "Drop Response",
      description: "Clear the response in the current context",
      fields: {},
    },
    reverse_lookup: {
      name: "Reverse Lookup",
      description:
        "Cache IP to domain relationships and optionally handle PTR queries directly",
      fields: {
        size: {
          label: "Check cache capacity",
          description:
            "Define the upper limit of the anti-check cache capacity.",
        },
        handle_ptr: {
          label: "Direct response to PTR",
          description:
            "Controls whether to respond to PTR requests directly with the reverse lookup cache.",
        },
        ttl: {
          label: "Mapping TTL (seconds)",
          description: "Defines the cache TTL for IP to domain name mapping.",
        },
      },
      metrics: {
        labels: {
          reverse_lookup_ptr_hit_total: "PTR hit",
          reverse_lookup_ptr_miss_total: "PTR miss",
          reverse_lookup_cache_insert_total: "cache writes",
          reverse_lookup_cache_entries: "cache entry",
        },
        help: {
          reverse_lookup_ptr_hit_total:
            "The total number of successful responses to PTR queries from the reverse lookup cache.",
          reverse_lookup_ptr_miss_total:
            "The total number of PTR query misses in the reverse cache.",
          reverse_lookup_cache_insert_total:
            "The total number of times IP → domain name mapping entries are written.",
          reverse_lookup_cache_entries:
            "The number of entries currently in the reverse lookup cache.",
        },
      },
    },
    query_summary: {
      name: "Query Summary",
      description:
        "Output compact query summary after subsequent link execution",
      fields: {
        msg: {
          label: "Log title",
          description: "Defines the summary log title.",
          placeholder: "main pipeline",
        },
      },
      quickSetup: {
        paramPlaceholder: "main pipeline",
      },
    },
    learn_domain: {
      name: "Learn Domain",
      description:
        "Write the requested domain name into dynamic_domain_set for dynamic rule learning",
      fields: {
        provider: {
          label: "Target Provider",
          description: "References the target dynamic_domain_set provider.",
          placeholder: "learned_allow",
        },
        phase: {
          label: "learning stage",
          description:
            "Controls whether to learn before the downstream executor or after the response returns.",
          options: {
            before: "Before",
            after: "After",
          },
        },
        questions: {
          label: "Questions",
          description: "Controls learning the first question or all questions.",
          options: {
            first: "First",
            all: "All",
          },
        },
        qtypes: {
          label: "Query type",
          description: "Just learn to specify DNS query types.",
          placeholder: "A\nAAAA",
        },
        "qtypes[]": {
          label: "Enter value",
          placeholder: "A",
        },
        success_only: {
          label: "Only successful response",
          description:
            "Only learned when the response is NOERROR; only takes effect in the after phase.",
        },
        answer_required: {
          label: "Need Answer",
          description:
            "Only learned when the response contains answer; only takes effect in the after stage.",
        },
        rule_kind: {
          label: "Rule type",
          description:
            "Controls the type of rules written to dynamic_domain_set.",
          options: {
            full: "Full",
            domain: "Domain",
          },
        },
        async: {
          label: "Asynchronous writing",
          description:
            "Controls whether to continue execution only after joining the queue; after closing, it will wait for the writing to complete.",
        },
        error_mode: {
          label: "Error handling",
          description: "Controlling executive behavior after learning failure.",
          options: {
            continue: "Continue",
            stop: "Stop",
            fail: "Fail",
          },
        },
        timeout: {
          label: "Sync timeout",
          description:
            "The maximum amount of time to wait for provider writes to complete when async is closed.",
        },
      },
    },
    query_recorder: {
      name: "Query Recorder",
      description:
        "Persists requests, responses, and sequence path events to SQLite",
      fields: {
        path: {
          label: "SQLite file",
          description:
            "Specifies the SQLite file path of the current recorder.",
          placeholder: "./data/query-recorder-main.sqlite",
        },
        queue_size: {
          label: "Queue size",
          description:
            "Defines the bounded queue size between the request path and the background writer.",
        },
        batch_size: {
          label: "Batch size",
          description:
            "Defines how many records are written to SQLite in each background batch.",
        },
        flush_interval_ms: {
          label: "Flush interval (ms)",
          description:
            "Defines the batch flush interval for the background writer.",
        },
        memory_tail: {
          label: "Memory tail length",
          description:
            "Defines how many recent records are kept in memory for stream?tail=n replay.",
        },
        retention_days: {
          label: "Retention days",
          description:
            "Defines how many days logs are retained; expired data is periodically deleted.",
        },
        cleanup_interval_hours: {
          label: "Cleanup interval (hours)",
          description: "Defines how often the expired-data cleanup task runs.",
        },
      },
    },
    metrics_collector: {
      name: "Metrics Collector",
      description:
        "Collect lightweight request counts and latency metrics and export them to Prometheus format",
      fields: {
        name: {
          label: "Metric name",
          description: "Defines the name tag of the current metric collector.",
        },
      },
      quickSetup: {
        paramPlaceholder: "main",
      },
      metrics: {
        labels: {
          query_total: "Total queries",
          query_error_total: "Query errors",
          query_inflight: "In flight",
          query_latency_count: "Latency samples",
          query_latency_sum_ms: "Total latency (ms)",
        },
        help: {
          query_total:
            "Total number of DNS queries observed by the metrics collector.",
          query_error_total:
            "The total number of queries that did not produce a response (errors or no response).",
          query_inflight:
            "The number of DNS queries currently being processed.",
          query_latency_count:
            "The number of completed queries included in latency statistics.",
          query_latency_sum_ms:
            "Total latency in milliseconds for all completed queries.",
        },
        derived: {
          "latency:query": "Average latency",
          "percent:query_error_total/query_total": "Error rate",
        },
      },
    },
    debug_print: {
      name: "Debug Print",
      description:
        "Print request and response objects to facilitate troubleshooting",
      fields: {
        msg: {
          label: "Log title",
          description: "Define the log output title.",
        },
      },
      quickSetup: {
        paramPlaceholder: "cron refresh",
      },
    },
    sleep: {
      name: "Sleep",
      description: "Add controllable asynchronous delay to the policy chain",
      fields: {
        duration: {
          label: "Delay(ms)",
          description:
            "Defines the additional asynchronous wait time for the current request on this executor.",
        },
      },
      quickSetup: {
        paramPlaceholder: "250ms / 2s",
      },
    },
    http_request: {
      name: "HTTP Request",
      description:
        "Send webhook, audit or linkage requests to external HTTP/HTTPS services",
      fields: {
        method: {
          label: "HTTP method",
          description:
            "Specify the HTTP method, such as GET, POST, PUT, PATCH, DELETE.",
          options: {
            GET: "GET",
            POST: "POST",
            PUT: "PUT",
            PATCH: "PATCH",
            DELETE: "DELETE",
          },
        },
        url: {
          label: "Target URL",
          description: "Target URL.",
          placeholder: "https://hooks.example.com/dns",
        },
        phase: {
          label: "triggering phase",
          description:
            "Controls whether requests are sent before downstream executors or after downstream execution completes.",
          options: {
            before: "Before",
            after: "After",
          },
        },
        async: {
          label: "Send asynchronously",
          description:
            "Controls whether to use an asynchronous background queue to send, or to wait for HTTP completion synchronously on the current request path.",
        },
        timeout: {
          label: "Timeout",
          description: "Limit the total timeout for a single HTTP call.",
        },
        error_mode: {
          label: "Error handling",
          description: "Controls how HTTP calls are handled when they fail.",
          options: {
            continue: "Continue",
            stop: "Stop",
            fail: "Fail",
          },
        },
        headers: {
          label: "Request header",
          description: "Append HTTP request headers.",
          keyPlaceholder: "X-Qname",
          valuePlaceholder: "${qname}",
        },
        query_params: {
          label: "Query Parameters",
          description: "Append extra parameters to the URL query.",
          keyPlaceholder: "qname",
          valuePlaceholder: "${qname}",
        },
        body: {
          label: "Original Body",
          description: "Raw string request body.",
          placeholder: "qname=${qname}",
        },
        json: {
          label: "JSON Body",
          description: "Send the request body as JSON.",
          placeholder: '{"qname":"${qname}","client_ip":"${client_ip}"}',
        },
        form: {
          label: "Form Body",
          description: "Send the form as application/x-www-form-urlencoded.",
          keyPlaceholder: "qname",
          valuePlaceholder: "${qname}",
        },
        content_type: {
          label: "Content-Type",
          description: "Specify Content-Type for raw args.body.",
        },
        socks5: {
          label: "SOCKS5 proxy",
          description: "Specify SOCKS5 proxy.",
          placeholder: "127.0.0.1:1080",
        },
        insecure_skip_verify: {
          label: "Skip HTTPS certificate verification",
          description: "Whether to skip HTTPS certificate verification.",
        },
        max_redirects: {
          label: "Maximum number of redirects",
          description: "Limit the maximum number of redirects to follow.",
        },
        queue_size: {
          label: "Asynchronous queue size",
          description:
            "The capacity of the background send queue in asynchronous mode.",
        },
      },
      metrics: {
        labels: {
          http_request_dispatch_total: "Request initiated",
          http_request_error_total: "Request failed",
          http_request_dropped_total: "queue dropped",
        },
        help: {
          http_request_dispatch_total:
            "http_request The total number of HTTP requests made by the executor.",
          http_request_error_total:
            "The total number of HTTP request failures (render failure, send failure, or asynchronous delivery failure).",
          http_request_dropped_total:
            "The total number of HTTP requests dropped because the asynchronous queue was full or closed.",
        },
      },
    },
    script: {
      name: "Script",
      description:
        "Execute external commands and inject stable fields in DnsContext",
      fields: {
        command: {
          label: "Order",
          description: "The path or name of the command to be executed.",
          placeholder: "bash",
        },
        args: {
          label: "Command parameters",
          description: "Array of parameters passed to the command.",
          placeholder: "/etc/oxidns/notify.sh\n${qname}",
        },
        "args[]": {
          label: "Enter value",
          placeholder: "/etc/oxidns/notify.sh",
        },
        env: {
          label: "environment variables",
          description:
            "Key-value pairs appended to the child process's environment variables.",
          keyPlaceholder: "FDNS_QNAME",
          valuePlaceholder: "${qname}",
        },
        cwd: {
          label: "Working directory",
          description: "Specify the working directory when the script is run.",
          placeholder: "/etc/oxidns",
        },
        timeout: {
          label: "Timeout",
          description: "Limit the execution time of a single script.",
        },
        error_mode: {
          label: "Error handling",
          description: "Controls what to do if a script fails or times out.",
          options: {
            continue: "Continue",
            stop: "Stop",
            fail: "Fail",
          },
        },
        max_output_bytes: {
          label: "Maximum output capture bytes",
          description:
            "Limit the capture length of stdout / stderr, and only mark the excess part as truncation.",
        },
      },
      metrics: {
        labels: {
          script_run_total: "Runs",
          script_success_total: "Successful runs",
          script_error_total: "Failed runs",
          script_timeout_total: "Timeouts",
        },
        help: {
          script_run_total:
            "The total number of external commands started by the script executor.",
          script_success_total:
            "The total number of times the external command exited normally (exit code 0).",
          script_error_total:
            "The total number of external command execution failures (non-zero exit or runtime error).",
          script_timeout_total:
            "The total number of times external commands were terminated due to timeouts.",
        },
      },
    },
    ipset: {
      name: "IPSet",
      description: "Write the IP in the response to Linux ipset",
      fields: {
        set_name4: {
          label: "IPv4 ipset name",
          description:
            "Specifies the ipset name to which IPv4 addresses are written.",
        },
        set_name6: {
          label: "IPv6 ipset name",
          description:
            "Specifies the ipset name to which IPv6 addresses are written.",
        },
        mask4: {
          label: "IPv4 prefix length",
          description:
            "Specifies the prefix length used when writing IPv4 addresses to ipset.",
        },
        mask6: {
          label: "IPv6 prefix length",
          description:
            "Specifies the prefix length used when writing IPv6 addresses to ipset.",
        },
      },
      quickSetup: {
        paramPlaceholder: "oxidns_v4,inet,24 oxidns_v6,inet6,64",
      },
      metrics: {
        labels: {
          ipset_entries_total: "Enqueue entry",
          ipset_dropped_total: "discard batch",
          ipset_write_total: "write entry",
          ipset_write_error_total: "Write failed",
        },
        help: {
          ipset_entries_total:
            "The total number of IP entries queued to be written to ipset.",
          ipset_dropped_total:
            "The total number of batches discarded because the write queue was full.",
          ipset_write_total:
            "The total number of IP entries successfully written to ipset via netlink.",
          ipset_write_error_total:
            "The total number of ipset netlink write failures.",
        },
      },
    },
    nftset: {
      name: "NFTSet",
      description: "Write response IP to Linux nftables set",
      fields: {
        ipv4: {
          label: "IPv4 target",
          description: "Define IPv4 target nftables set.",
        },
        "ipv4.table_family": {
          label: "Table Family",
          placeholder: "ip",
        },
        "ipv4.table_name": {
          label: "table name",
          placeholder: "mangle",
        },
        "ipv4.set_name": {
          label: "Set name",
          placeholder: "dns_v4",
        },
        "ipv4.mask": {
          label: "prefix length",
          placeholder: "24",
        },
        ipv6: {
          label: "IPv6 target",
          description: "Define IPv6 target nftables set.",
        },
        "ipv6.table_family": {
          label: "Table Family",
          placeholder: "ip6",
        },
        "ipv6.table_name": {
          label: "table name",
          placeholder: "mangle",
        },
        "ipv6.set_name": {
          label: "Set name",
          placeholder: "dns_v6",
        },
        "ipv6.mask": {
          label: "prefix length",
          placeholder: "24",
        },
        table_family4: {
          label: "IPv4 table family",
          description:
            "Define the nftables table family of IPv4 in compatible writing.",
        },
        table_name4: {
          label: "IPv4 table name",
          description:
            "Define the nftables table name of IPv4 in compatible writing.",
        },
        set_name4: {
          label: "IPv4 set name",
          description: "Define the IPv4 set name in compatible writing.",
        },
        mask4: {
          label: "IPv4 prefix length",
          description: "Define the IPv4 prefix length in compatible writing.",
        },
        table_family6: {
          label: "IPv6 table family",
          description:
            "Define the nftables table family of IPv6 in compatible writing.",
        },
        table_name6: {
          label: "IPv6 table name",
          description:
            "Define the nftables table name of IPv6 in compatible writing.",
        },
        set_name6: {
          label: "IPv6 set name",
          description: "Define the IPv6 set name in compatible writing.",
        },
        mask6: {
          label: "IPv6 prefix length",
          description: "Define the IPv6 prefix length in compatible writing.",
        },
      },
      quickSetup: {
        paramPlaceholder: "ip,mangle,dns_v4,ipv4_addr,24",
      },
      metrics: {
        labels: {
          nftset_entries_total: "enqueuing prefix",
          nftset_dropped_total: "discard batch",
          nftset_write_total: "write prefix",
          nftset_write_error_total: "Write failed",
        },
        help: {
          nftset_entries_total:
            "The total number of IP prefixes queued to be written to nftset.",
          nftset_dropped_total:
            "The total number of batches discarded because the write queue was full.",
          nftset_write_total:
            "The total number of IP prefixes successfully written to nftables via netlink.",
          nftset_write_error_total:
            "The total number of nftset netlink write failures.",
        },
      },
    },
    ros_address_list: {
      name: "RouterOS Address List",
      description: "Synchronize answering IP to RouterOS address-list",
      fields: {
        address: {
          label: "RouterOS API address",
          description:
            "Specify the RouterOS API service address, usually written as host:port.",
          placeholder: "172.16.1.1:8728",
        },
        username: {
          label: "username",
          description: "Specify the RouterOS API login username.",
        },
        password: {
          label: "password",
          description: "Specify the RouterOS API login password.",
        },
        connect_timeout: {
          label: "Connection timeout",
          description:
            "Maximum wait time for establishing a RouterOS API connection, in seconds.",
        },
        send_timeout: {
          label: "Send timeout",
          description:
            "Maximum wait time for sending one RouterOS API command, in seconds.",
        },
        receive_timeout: {
          label: "Receive timeout",
          description:
            "Maximum wait time for the next RouterOS API response chunk, in seconds.",
        },
        async: {
          label: "Asynchronous submission",
          description:
            "Controls whether address writing behavior is asynchronous.",
        },
        address_list4: {
          label: "IPv4 Address List",
          description:
            "Specifies the address-list name to which IPv4 addresses are written.",
        },
        address_list6: {
          label: "IPv6 Address List",
          description:
            "Specifies the address-list name to which IPv6 addresses are written.",
        },
        comment_prefix: {
          label: "annotation prefix",
          description:
            "Specifies the annotation prefix used by the plug-in when writing RouterOS entries.",
        },
        persistent: {
          label: "Resident address",
          description:
            "Define a set of static addresses that need to be retained for a long time.",
        },
        "persistent.ips": {
          label: "IP / CIDR",
          description: "Declare the resident IP or CIDR segment inline.",
          placeholder: "1.1.1.1\n100.64.1.0/24",
        },
        "persistent.ips[]": {
          label: "Enter value",
          placeholder: "1.1.1.1",
        },
        "persistent.files": {
          label: "document",
          description:
            "Loads a collection of resident addresses from an external file.",
          placeholder: "/etc/oxidns/persistent_ips.txt",
        },
        "persistent.files[]": {
          label: "Enter value",
          placeholder: "/etc/oxidns/persistent_ips.txt",
        },
        min_ttl: {
          label: "Dynamic item minimum TTL",
          description:
            "Defines the minimum TTL allowed for dynamic address entries.",
        },
        max_ttl: {
          label: "Dynamic item maximum TTL",
          description:
            "Defines the maximum TTL allowed for dynamic address entries.",
        },
        fixed_ttl: {
          label: "Dynamic items fixed TTL",
          description: "Specify a fixed TTL for all dynamic writes.",
        },
        cleanup_on_shutdown: {
          label: "Clean up on shutdown",
          description:
            "Controls whether entries managed by the plugin are cleaned up when it exits.",
        },
      },
      metrics: {
        labels: {
          ros_address_list_observe_total: "Observation domain name",
          ros_address_list_dropped_total: "Asynchronous discard",
          ros_address_list_sync_error_total: "Sync failed",
          ros_address_list_sync_timeout_total: "Sync timeout",
        },
        help: {
          ros_address_list_observe_total:
            "The total number of domain name observations submitted to the RouterOS address-list manager.",
          ros_address_list_dropped_total:
            "The total number of observations dropped in asynchronous mode because the queue was full or the channel was closed.",
          ros_address_list_sync_error_total:
            "The total number of observations that failed on the RouterOS manager side in sync mode.",
          ros_address_list_sync_timeout_total:
            "The total number of observations that were queued or waited for timeout in sync mode.",
        },
      },
    },
    upgrade: {
      name: "Upgrade",
      description: "Perform the OxiDNS upgrade process",
      fields: {
        force: {
          label: "Forced upgrade",
          description:
            "Even if the target release is not newer than the current version, download, verify and replace will continue.",
        },
        cleanup: {
          label: "Clean cache after upgrade",
          description:
            "Clean cache_dir and backup_dir after successful upgrade.",
        },
        repository: {
          label: "GitHub repository",
          description: "GitHub repository.",
        },
        asset: {
          label: "Release Asset",
          description:
            "Release asset name; auto will select archive based on the current platform and compilation version.",
        },
        bundle: {
          label: "compiled version",
          description:
            "The compiled version used when asset is auto; auto will follow the compiled version of the current binary.",
          options: {
            auto: "Auto",
            full: "Full",
            standard: "Standard",
            minimal: "Minimal",
          },
        },
        github_token: {
          label: "GitHub Token",
          description:
            "GitHub personal access token, used to increase API rate limits or access private repositories.",
        },
        cache_dir: {
          label: "Download cache directory",
          description: "Download cache directory.",
          placeholder: "./upgrade/cache",
        },
        backup_dir: {
          label: "Backup directory",
          description: "Back up the directory before replacing.",
          placeholder: "./upgrade/backups",
        },
        webui_dir: {
          label: "WebUI directory",
          description:
            "The directory where WebUI static resources are installed during upgrade.",
          placeholder: "./webui",
        },
        skip_webui: {
          label: "Skip WebUI upgrade",
          description:
            "Skip WebUI directory upgrade when apply and only replace binary files.",
        },
        no_restart: {
          label: "Skip automatic restart",
          description:
            "After enabling it, a successful upgrade will not trigger an automatic restart.",
        },
        timeout: {
          label: "Timeout",
          description: "Limit the total wait time for the upgrade process.",
        },
        socks5: {
          label: "SOCKS5 proxy",
          description: "SOCKS5 proxy used when updating downloads.",
          placeholder: "127.0.0.1:1080",
        },
        insecure_skip_verify: {
          label: "Skip HTTPS certificate verification",
          description:
            "Skip HTTPS certificate verification when downloading upgrades.",
        },
      },
      quickSetup: {
        paramPlaceholder: "force=true",
      },
    },
    download: {
      name: "Download",
      description:
        "Download HTTP/HTTPS files to local directory and overwrite them atomically",
      fields: {
        downloads: {
          label: "Download items",
          description:
            "Download one or more HTTP/HTTPS files to a local directory and overwrite the target files after the new content is completely written.",
          placeholder:
            '[{"url":"https://example.com/geosite.dat","dir":"/etc/oxidns","filename":"geosite.dat"}]',
        },
        "downloads[]": {
          label: "Download items",
        },
        "downloads[].url": {
          label: "URL",
          description: "The HTTP/HTTPS URL of the download.",
          placeholder: "https://example.com/geosite.dat",
        },
        "downloads[].dir": {
          label: "Directory",
          description: "The destination directory for downloaded items.",
          placeholder: "/etc/oxidns",
        },
        "downloads[].filename": {
          label: "File name",
          description: "The target file name of the downloaded item.",
          placeholder: "geosite.dat",
        },
        timeout: {
          label: "Timeout",
          description: "Download timeout.",
        },
        socks5: {
          label: "SOCKS5 proxy",
          description:
            "All download connections will be initiated through this SOCKS5 proxy.",
          placeholder: "127.0.0.1:1080",
        },
        startup_if_missing: {
          label: "Complete missing files at startup",
          description:
            "The target file is checked at startup, and missing items are automatically downloaded before other plug-ins are initialized.",
        },
      },
      quickSetup: {
        paramPlaceholder: "https://example.com/rules.txt /etc/oxidns",
      },
      metrics: {
        labels: {
          download_success_total: "Download successful",
          download_failure_total: "Download failed",
          download_timeout_total: "Download timeout",
        },
        help: {
          download_success_total:
            "The total number of successfully completed file downloads.",
          download_failure_total:
            "The total number of failed downloads (excluding timeouts).",
          download_timeout_total:
            "The total number of file downloads interrupted by timeouts.",
        },
      },
    },
    reload_provider: {
      name: "Reload Provider",
      description: "Refresh one or more providers by tag",
      fields: {
        args: {
          label: "Provider reference",
          description:
            "Execute targeted provider reload one by one in the order declared in args.",
          placeholder: "$geosite_cn\n$geoip_cn",
        },
        "args[]": {
          placeholder: "geosite_cn",
        },
      },
      quickSetup: {
        paramPlaceholder: "$geosite_cn",
      },
      metrics: {
        labels: {
          reload_provider_reload_total: "Data source reloading",
          reload_provider_reload_error_total: "Reload failed",
        },
        help: {
          reload_provider_reload_total:
            "The total number of provider reload attempts triggered by this executor.",
          reload_provider_reload_error_total:
            "The total number of failed provider reloads.",
        },
      },
    },
    reload: {
      name: "Reload",
      description: "Trigger an application-level full reload",
      fields: {},
      metrics: {
        labels: {
          reload_trigger_total: "Reload trigger",
          reload_error_total: "Reload failed",
        },
        help: {
          reload_trigger_total:
            "The total number of application-level full reloads requested by this executor.",
          reload_error_total:
            "The total number of reload request scheduling failures.",
        },
      },
    },
    cron: {
      name: "Cron",
      description:
        "The background schedules a group of executors according to cron or fixed intervals",
      fields: {
        jobs: {
          label: "task list",
          description: "Define one or more background tasks.",
          placeholder:
            '[{"name":"refresh_sets","interval":"5m","executors":["$seq_refresh"]}]',
        },
        "jobs[]": {
          label: "Task",
        },
        "jobs[].name": {
          label: "Task name",
          description:
            "Task name, used for logging and runtime identification.",
          placeholder: "refresh_sets",
        },
        "jobs[].schedule": {
          label: "Cron expression",
          description:
            "Schedule tasks using standard 5-field cron expressions.",
          placeholder: "0 */6 * * *",
        },
        "jobs[].interval": {
          label: "fixed interval",
          description: "Schedule tasks with simple fixed intervals.",
          placeholder: "5m",
        },
        "jobs[].executors": {
          label: "Executor",
          description:
            "Defines a list of executors to be executed sequentially when a task is triggered.",
          placeholder: "$seq_refresh\ndebug_print cron refresh",
        },
        "jobs[].executors.$executor_ref": {
          label: "Reference executor",
          placeholder: "seq_refresh",
        },
        "jobs[].executors.$input": {
          label: "Enter value",
          placeholder: "debug_print cron refresh",
        },
        timezone: {
          label: "time zone",
          description:
            "Specify the time zone for all schedule tasks under the current cron plugin.",
          placeholder: "Asia/Shanghai",
        },
      },
      metrics: {
        labels: {
          cron_job_run_total: "task run",
          cron_job_skipped_total: "overlap skip",
          cron_executor_error_total: "Executor failed",
        },
        help: {
          cron_job_run_total:
            "The total number of times the cron task has been started.",
          cron_job_skipped_total:
            "The total number of times this trigger has been skipped because the last run has not yet ended.",
          cron_executor_error_total:
            "The total number of executor failures during the Cron job run.",
        },
      },
    },
    any_match: {
      name: "Any Match",
      description: "Combine multiple matchers and return true if any one hits",
      fields: {
        args: {
          label: "match expression",
          description:
            "One matcher expression per line, supporting $tag, shortcut expressions and ! negation",
          placeholder: "$match_tag\nqname domain:example.com\n!$blocked",
        },
        "args.$matcher_ref": {
          label: "Quote matcher",
          placeholder: "match_tag",
        },
        "args.$input": {
          label: "Enter value",
          placeholder: "qname domain:example.com",
        },
      },
    },
    qname: {
      name: "QName",
      description: "Match the query domain name in the request",
      fields: {
        args: {
          label: "Domain name rules",
          description: "Define the source of domain name matching rules.",
          placeholder:
            "full:login.example.com\ndomain:example.com\nkeyword:cdn\nregexp:^api[0-9]+\\.example\\.net$\n$core_domains\n&/etc/oxidns/domains.txt",
        },
        "args.$provider_ref": {
          label: "Reference provider",
          placeholder: "core_domains",
        },
        "args.$input": {
          label: "Enter value",
          placeholder: "regexp:^api[0-9]+\\.example\\.net$",
        },
      },
      quickSetup: {
        paramPlaceholder: "$domain_set",
      },
    },
    question: {
      name: "Question",
      description:
        "Match request question by provider's contains_question semantics",
      fields: {
        args: {
          label: "Provider reference",
          description:
            "Use the $provider_tag form to reference the provider that implements contains_question.",
          placeholder: "$ad_rules\n$shared_domains",
        },
        "args[]": {
          label: "Reference provider",
          placeholder: "ad_rules",
        },
      },
      quickSetup: {
        paramPlaceholder: "$ad_rules",
      },
    },
    qtype: {
      name: "QType",
      description: "Match request qtype",
      fields: {
        args: {
          label: "QType text or numeric",
          description:
            "Define a set of query types that are allowed to hit, supporting text such as A/AAAA and corresponding values.",
          placeholder: "A\nAAAA\n1\n28",
        },
        "args[]": {
          label: "Enter value",
          placeholder: "A",
        },
      },
      quickSetup: {
        paramPlaceholder: "A,AAAA or 1,28",
      },
    },
    qclass: {
      name: "QClass",
      description: "Match request qclass",
      fields: {
        args: {
          label: "QClass text or numeric",
          description:
            "Define a set of query categories that are allowed to hit, and support text such as IN/CH and corresponding values.",
          placeholder: "IN\n1",
        },
        "args[]": {
          label: "Enter value",
          placeholder: "IN",
        },
      },
      quickSetup: {
        paramPlaceholder: "IN or 1",
      },
    },
    client_ip: {
      name: "Client IP",
      description: "Match client source IP",
      fields: {
        args: {
          label: "IP / CIDR / ip_set",
          description: "Define client source address matching conditions.",
          placeholder: "192.168.0.0/16\n$lan_ip_set",
        },
        "args.$provider_ref": {
          label: "Reference provider",
          placeholder: "lan_ip_set",
        },
        "args.$input": {
          label: "Enter value",
          placeholder: "192.168.0.0/16",
        },
      },
      quickSetup: {
        paramPlaceholder: "$lan_ip_set",
      },
    },
    resp_ip: {
      name: "Response IP",
      description: "Matches A/AAAA IP in responses answers",
      fields: {
        args: {
          label: "IP / CIDR / ip_set",
          description: "Define reply address matching conditions.",
          placeholder: "100.64.0.0/10\n$special_targets",
        },
        "args.$provider_ref": {
          label: "Reference provider",
          placeholder: "special_targets",
        },
        "args.$input": {
          label: "Enter value",
          placeholder: "100.64.0.0/10",
        },
      },
      quickSetup: {
        paramPlaceholder: "$ip_set",
      },
    },
    ptr_ip: {
      name: "PTR IP",
      description: "Match after requesting name from PTR after IP resolution",
      fields: {
        args: {
          label: "IP / CIDR / ip_set",
          description: "Define PTR reverse check address matching conditions.",
          placeholder: "192.168.0.0/16\n$lan_ip_set",
        },
        "args.$provider_ref": {
          label: "Reference provider",
          placeholder: "lan_ip_set",
        },
        "args.$input": {
          label: "Enter value",
          placeholder: "192.168.0.0/16",
        },
      },
      quickSetup: {
        paramPlaceholder: "$lan_ip_set",
      },
    },
    cname: {
      name: "CNAME",
      description: "Match the CNAME target domain name in the response",
      fields: {
        args: {
          label: "CNAME rules",
          description:
            "Defines the response CNAME target domain name matching rule source.",
          placeholder:
            "full:alias.example.com\ndomain:example.com\nkeyword:cdn\nregexp:^edge[0-9]+\\.example\\.net$\n$core_domains\n&/etc/oxidns/cnames.txt",
        },
        "args.$provider_ref": {
          label: "Reference provider",
          placeholder: "core_domains",
        },
        "args.$input": {
          label: "Enter value",
          placeholder: "regexp:^edge[0-9]+\\.example\\.net$",
        },
      },
      quickSetup: {
        paramPlaceholder: "$domain_set",
      },
    },
    rcode: {
      name: "RCode",
      description: "Match current response rcode",
      fields: {
        args: {
          label: "RCode text or value",
          description:
            "Defines a set of response codes that allow hits, and supports text and corresponding values ​​such as SERVFAIL/NXDOMAIN.",
          placeholder: "NOERROR\nSERVFAIL\nNXDOMAIN\n0\n2\n3",
        },
        "args[]": {
          label: "Enter value",
          placeholder: "NOERROR",
        },
      },
      quickSetup: {
        paramPlaceholder: "SERVFAIL,NXDOMAIN or 2,3",
      },
    },
    has_resp: {
      name: "Has Response",
      description: "Hit when there is already a response in the context",
      fields: {},
    },
    has_wanted_ans: {
      name: "Has Wanted Answer",
      description:
        "Hit when the response answers contains the record corresponding to the request qtype",
      fields: {},
    },
    mark: {
      name: "Mark",
      description: "Matches the mark collection in the context",
      fields: {
        args: {
          label: "Mark",
          description: "Defines the set of marks that are allowed to hit.",
          placeholder: "100\n200",
        },
        "args[]": {
          label: "Enter value",
          placeholder: "100",
        },
      },
      quickSetup: {
        paramPlaceholder: "100,200",
      },
    },
    env: {
      name: "Env",
      description: "Match process environment variables",
      fields: {
        args: {
          label: "environment variable conditions",
          description:
            "Define environment variable conditions that need to be met at the same time.",
          placeholder: "PROFILE=prod\nFEATURE_X",
        },
        "args[]": {
          label: "Enter value",
          placeholder: "PROFILE=prod",
        },
      },
      quickSetup: {
        paramPlaceholder: "PROFILE=prod FEATURE_X",
      },
    },
    random: {
      name: "Random",
      description: "hit according to probability",
      fields: {
        args: {
          label: "Probability",
          description: "Define matcher hit probability.",
          placeholder: "0.1",
        },
        "args[]": {
          label: "Enter value",
          placeholder: "0.1",
        },
      },
      quickSetup: {
        paramPlaceholder: "0.1",
      },
    },
    rate_limiter: {
      name: "Rate Limiter",
      description: "Token bucket current limit based on client IP",
      fields: {
        qps: {
          label: "QPS",
          description: "Defines the token replenishment rate per second.",
        },
        burst: {
          label: "Burst capacity",
          description: "Define the upper limit of the token bucket capacity.",
        },
        mask4: {
          label: "IPv4 aggregate prefix",
          description: "Define IPv4 client aggregation granularity.",
        },
        mask6: {
          label: "IPv6 aggregate prefix",
          description: "Define IPv6 client aggregation granularity.",
        },
      },
      quickSetup: {
        paramPlaceholder: "20 40",
      },
      metrics: {
        labels: {
          ratelimit_allowed_total: "release",
          ratelimit_rejected_total: "Current limit rejection",
        },
        help: {
          ratelimit_allowed_total:
            "The total number of matches that passed the throttling check (sufficient tokens).",
          ratelimit_rejected_total:
            "The total number of matches rejected by throttling due to token exhaustion.",
        },
        derived: {
          "percent_of_sum:ratelimit_rejected_total/ratelimit_rejected_total+ratelimit_allowed_total":
            "rejection rate",
        },
      },
    },
    string_exp: {
      name: "String Expression",
      description: "Universal string expression matcher",
      fields: {
        args: {
          label: "expression",
          description: "Universal string expression matcher expression.",
          placeholder: "url_path prefix /dns-",
        },
      },
      quickSetup: {
        paramPlaceholder: "url_path prefix /dns-",
      },
    },
    _true: {
      name: "Always True",
      description: "Always true and can be used as a guaranteed hit condition",
      fields: {},
    },
    _false: {
      name: "Always False",
      description: "Always false, can be used to temporarily disable a rule",
      fields: {},
    },
    domain_set: {
      name: "Domain Set",
      description:
        "A collection of high-performance domain name rules that can be referenced by qname, cname, etc.",
      fields: {
        exps: {
          label: "Inline domain name rules",
          description: "Define a list of inline domain name expressions.",
          placeholder:
            "full:login.example.com\ndomain:example.com\nkeyword:cdn\nregexp:^api[0-9]+\\.example\\.net$",
        },
        "exps[]": {
          label: "Enter value",
          placeholder: "full:login.example.com",
        },
        files: {
          label: "Domain name rules file",
          description: "Specify a list of external rule file paths.",
          placeholder: "/etc/oxidns/domains.txt",
        },
        "files[]": {
          label: "Enter value",
          placeholder: "/etc/oxidns/domains.txt",
        },
        sets: {
          label: "Downstream Provider",
          description:
            "Reference other providers with domain name matching capabilities.",
          placeholder: "shared_domains\nshared_geosite",
        },
        "sets[]": {
          label: "Reference provider",
          placeholder: "shared_domains",
        },
      },
    },
    dynamic_domain_set: {
      name: "Dynamic Domain Set",
      description:
        "Writable local domain rule file with automatic learning and API management",
      fields: {
        path: {
          label: "Rules file",
          description:
            "Specify the path to the local rules file managed by this dynamic provider.",
          placeholder: "/etc/oxidns/learned-allow.txt",
        },
        bootstrap_rules: {
          label: "Initial rules",
          description:
            "Initial rules are only written if the rules file does not exist.",
          placeholder: "full:login.example.com\ndomain:example.com",
        },
        "bootstrap_rules[]": {
          label: "Enter value",
          placeholder: "full:login.example.com",
        },
        queue_size: {
          label: "Queue size",
          description: "Defines the auto-learning write queue size.",
        },
        batch_size: {
          label: "Batch size",
          description:
            "Defines the batch flush threshold for background appends.",
        },
        flush_interval_ms: {
          label: "Flush interval (ms)",
          description:
            "Defines the scheduled flush interval for background appends.",
        },
      },
    },
    geosite: {
      name: "Geosite",
      description: "Extract domain name rule set from geosite.dat",
      fields: {
        file: {
          label: "geosite.dat",
          description: "Specify the geosite.dat file path.",
          placeholder: "/etc/oxidns/geosite.dat",
        },
        selectors: {
          label: "Selector",
          description:
            "Extract some rules by code, and also support code@attribute syntax to further filter by attribute.",
          placeholder: "cn\ngeolocation-!cn",
        },
        "selectors[]": {
          label: "Enter value",
          placeholder: "cn",
        },
      },
    },
    adguard_rule: {
      name: "AdGuard Rule",
      description: "Provides a subset of AdGuard Home DNS rules",
      fields: {
        rules: {
          label: "inline rules",
          description: "Provides a subset of AdGuard Home DNS rules.",
          placeholder: "||ads.example.com^\n@@||safe.ads.example.com^",
        },
        "rules[]": {
          label: "Enter value",
          placeholder: "||ads.example.com^",
        },
        files: {
          label: "rules file",
          description: "Load from external rules file.",
          placeholder: "/etc/oxidns/adguard.txt",
        },
        "files[]": {
          label: "Enter value",
          placeholder: "/etc/oxidns/adguard.txt",
        },
      },
    },
    ip_set: {
      name: "IP Set",
      description:
        "IP / CIDR rule set, can be referenced by client_ip, resp_ip, ptr_ip",
      fields: {
        ips: {
          label: "IP / CIDR",
          description: "Define a list of inline IP or CIDR rules.",
          placeholder: "192.168.0.0/16\nfd00::/8",
        },
        "ips[]": {
          label: "Enter value",
          placeholder: "192.168.0.0/16",
        },
        files: {
          label: "IP rules file",
          description: "Specify a list of external IP rule file paths.",
          placeholder: "/etc/oxidns/ips.txt",
        },
        "files[]": {
          label: "Enter value",
          placeholder: "/etc/oxidns/ips.txt",
        },
        sets: {
          label: "Downstream Provider",
          description: "Reference other ip_set instances.",
          placeholder: "shared_ip_set\nshared_geoip",
        },
        "sets[]": {
          label: "Reference provider",
          placeholder: "shared_ip_set",
        },
      },
    },
    geoip: {
      name: "GeoIP",
      description: "Extract IP/CIDR collection from geoip.dat",
      fields: {
        file: {
          label: "geoip.dat",
          description: "Specify the geoip.dat file path.",
          placeholder: "/etc/oxidns/geoip.dat",
        },
        selectors: {
          label: "Selector",
          description: "Extract IP/CIDR collection by code.",
          placeholder: "cn",
        },
        "selectors[]": {
          label: "Enter value",
          placeholder: "cn",
        },
      },
    },
  },
} as const satisfies LocaleResourceShape<typeof zhCNPluginDefined>;
