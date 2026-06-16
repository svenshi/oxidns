import { zhCNDocs } from "../zh-CN/docs";
import type { LocaleResourceShape } from "../../types";

export const enUSDocs = {
  udp_server: {
    entry:
      "- Type: `string`; Required: Yes; Default: None\n- Function: Specify the entry executor that handles all requests of the listener, usually the sequence plug-in.\n- Configuration requirements:\n  - Must reference a defined executor plugin.\n  - A common value is a `tag` of a certain `sequence`.\n- Operational impact:\n  - All requests entering the current `udp_server` will be handed over to this executor for continued processing.\n  - If the reference does not exist or is of the wrong type, plugin initialization will fail.",
    listen:
      "- Type: `string`; Required: Yes; Default: None\n- Function: Specify the UDP listening address.\n- Supported formats:\n  - `ip:port`\n  - `:port`\n- Operational impact:\n  - Determine the address and port to which the listener is bound.\n  - The listener cannot be started when the address is invalid, the port conflicts, or the binding fails.",
  },
  tcp_server: {
    entry:
      "- Type: `string`; Required: Yes; Default: None\n- Function: Specify the entry executor used when TCP or DoT requests enter the policy chain.\n- Configuration requirements:\n  - Must reference a defined executor plugin.\n- Operational impact:\n  - All DNS messages on the connection are handled by this executor.",
    listen:
      "- Type: `string`; Required: Yes; Default: None\n- Function: Specify the TCP listening address.\n- Supported formats:\n  - `ip:port`\n  - `:port`\n- Operational impact:\n  - Affects binding addresses for plaintext TCP or DoT services.",
    cert: "- Type: `string`; Required: No; Default: None\n- Function: Specify the TLS certificate file path.\n- Conditions of use:\n  - Enable TLS when used with `key`.\n- Operational impact:\n  - Configured to use `tcp_server` as DoT entry.",
    key: "- Type: `string`; Required: No; Default: None\n- Function: Specify the TLS private key file path.\n- Conditions of use:\n  - Enable TLS when used with `cert`.\n- Operational impact:\n  - When missing or invalid, TLS mode cannot be established.",
    idle_timeout:
      "- Type: `integer`; required: no; default value: `10`\n- Unit: seconds\n- Function: Specify the connection idle timeout setting.\n- Operational impact:\n  - Affects long connection keep-alive and idle connection life cycles.\n  - The larger the value, the longer the idle connection is retained.",
  },
  http_server: {
    entries:
      "- Type: `array`; Required: Yes; Default: None\n- Function: Define the mapping relationship between HTTP paths and executors.\n- Each element contains the following fields:\n  - `path`\n    - Type: `string`\n    - Required: Yes\n    - Function: Specify the DoH request path.\n    - Constraint: Must start with `/`.\n  - `exec`\n    - Type: `string`\n    - Required: Yes\n    - Function: Specify the executor to handle the path request.\n    - Constraint: Must reference a defined executor plugin.\n- Operational impact:\n  - Different paths can enter different strategy chains.",
    listen:
      "- Type: `string`; Required: Yes; Default: None\n- Function: Specify the HTTP/HTTPS listening address.",
    src_ip_header:
      "- Type: `string`; Required: No; Default: None\n- Function: Specify the field name to read the real client source address from the request header.\n- Operational impact:\n  - After configuration, the request source address can be transparently transmitted by the reverse proxy.",
    cert: "- Type: `string`; Required: No; Default: None\n- Function: Specify the HTTPS certificate file path.\n- Operational impact:\n  - Enable HTTPS when configured with `key`.",
    key: "- Type: `string`; Required: No; Default: None\n- Function: Specify the HTTPS private key file path.\n- Operational impact:\n  - Enable HTTPS when configured with `cert`.",
    idle_timeout:
      "- Type: `integer`; required: no; default value: `30`\n- Unit: seconds\n- Function: Specify HTTP connection idle timeout.\n- Operational impact:\n  - Affects HTTP/2 long connection life cycle.",
    enable_http3:
      '- Type: `boolean`; required: no; default value: `false`\n- Function: Specify whether to enable HTTP/3 at the same time.\n- Conditions of use:\n  - `cert` and `key` need to be configured at the same time.\n- Operational impact:\n  - When enabled, an additional QUIC-based DoH listening task will be launched.\n  - HTTP/2 response will return `Alt-Svc: h3=":<listen-port>"; ma=86400`, prompting the client to upgrade to HTTP/3 on the same port.',
  },
  quic_server: {
    entry:
      "- Type: `string`; Required: Yes; Default: None\n- Function: Specify the entry executor used when DoQ requests enter the policy chain.\n- Configuration requirements:\n  - Must reference a defined executor plugin.",
    listen:
      "- Type: `string`; Required: Yes; Default: None\n- Function: Specify the QUIC listening address.\n- Operational impact:\n  - Actual occupied UDP port.",
    cert: "- Type: `string`; Required: Yes; Default: None\n- Function: Specify the TLS certificate file required by DoQ.\n- Operational impact:\n  - The listener cannot be started when the certificate is invalid.",
    key: "- Type: `string`; Required: Yes; Default: None\n- Function: Specify the TLS private key file required by DoQ.\n- Operational impact:\n  - The listener cannot be started when the private key is invalid.",
    idle_timeout:
      "- Type: `integer`; required: no; default value: none\n- Unit: seconds\n- Function: Specify the idle timeout of QUIC transport.\n- Operational impact:\n  - Affects the recycling timing of idle QUIC connections.",
  },
  sequence: {
    args: "- Type: `array`; Required: Yes; Default: None\n- Function: Define the rule chain of sequence.\n- Operational impact:\n  - Rules are executed in the order they are written.\n  - Plugin initialization fails when `args` is empty.",
    "args[].matches":
      "- Type: `string` or `array`\n- Required: No\n- Default: None\n- Function: Define the matching conditions of the current rule.\n- Supported forms:\n  - a single matcher string\n  - A list of multiple matchers\n- Operational impact:\n  - There is a logical AND relationship between multiple conditions.\n  - When not configured, it means there is no pre-matching condition.",
    "args[].exec":
      "- Type: `string`; Required: No; Default: None\n- Function: Define the action to be performed after the rule is hit.\n- Support content:\n  - Plugin reference\n  - Shortcut expressions\n  - Built-in control flow\n- Operational impact:\n  - Directly determines the execution behavior of the current rule.",
  },
  forward: {
    concurrent:
      "- Type: `integer`; required: no; default value: `1`\n- Value range: will be limited to `1..=3` during actual operation\n- Function: Define the number of concurrent query fanouts in multi-upstream mode.\n- Operational impact:\n  - The larger the value, the more active the multi-upstream competition will be, but at the same time it will increase the amount of upstream requests.",
    upstreams:
      "- Type: `array`; Required: Yes; Default: None\n- Function: Define one or more upstream targets.\n- Operational impact:\n  - Use single upstream mode when array length is `1`.\n  - Use competitive query mode when the array length is greater than `1`.",
    short_circuit:
      "- Type: `boolean`; required: no; default value: `false`\n- Function: Control whether to stop the subsequent executor chain immediately after receiving a successful upstream response.\n- Description:\n  - When closed, `forward` will still write `response`, but subsequent executors can continue to process the response.\n  - When enabled, subsequent executor chains will be terminated directly upon successful return.",
    "upstreams[].addr":
      "- Type: `string`; Required: Yes; Default: None\n- Function: Define the upstream address, protocol type and target host.\n- Supported formats:\n  - `udp://8.8.8.8:53` or `8.8.8.8:53`\n  - `tcp://8.8.8.8:53`\n  - `tcp+pipeline://8.8.8.8:53`\n  - `tls://dns.example:853`\n  - `tls+pipeline://dns.example:853`\n  - `quic://dns.example:853` or `doq://dns.example:853`\n  - `https://resolver.example/dns-query` or `doh://resolver.example/dns-query`\n  - `h3://resolver.example/dns-query`\n- Rule description:\n  - When the protocol is not written, it is processed by `udp://`.\n  - `https://` / `doh://` means DoH, `h3://` means force DoH over HTTP/3.\n  - `tcp+pipeline://` and `tls+pipeline://` will directly enable pipeline mode.\n  - The DoH address should contain the actual request path, such as `/dns-query`.\n- Configuration recommendations: Domain name upstream is recommended to configure `bootstrap` at the same time to avoid boot resolution dependency.",
    "upstreams[].tag":
      "- Type: `string`; Required: No; Default: None\n- Function: Provide log identification for a single upstream to facilitate troubleshooting multi-upstream competition results.",
    "upstreams[].dial_addr":
      "- Type: `ip`; Required: No; Default: None\n- Function: Specify the actual connection IP, while retaining the host name in `addr` for SNI, Host and certificate verification.\n- Applicable scenarios: fixed dial-up address, bypassing local resolution or matching with custom routing exports.",
    "upstreams[].port":
      "- Type: `integer`; required: no; default value: protocol default port\n- Function: Override the protocol default port.",
    "upstreams[].bootstrap":
      "- Type: `string`; Required: No; Default: None\n- Function: Provide guidance resolution server for domain name upstream.\n- Rule description:\n  - Only meaningful when `addr` uses a domain name.\n  - It must be written as `IP:port`; hostnames are rejected.\n  - Typically used for the first resolution of upstream DoT, DoQ, and DoH domain names.",
    "upstreams[].bootstrap_version":
      "- Type: `integer`; required: no; default value: none\n- Function: Specify bootstrap to give priority to IPv4 or IPv6.\n- Value: `4` or `6`.",
    "upstreams[].socks5":
      "- Type: `string`; Required: No; Default: None\n- Function: Specify SOCKS5 proxy for upstream connections.\n- Supported formats:\n  - `host:port`\n  - `username:password@host:port`\n  - IPv6 needs to be written as `[addr]:port`\n  - IPv6 with authentication needs to be written as `username:password@[addr]:port`\n- Rule description:\n  - The proxy host can be an IP or a host name; the host name will be resolved using the system.\n  - The authentication part only separates the username and password by the first `:`, so the format must be `username:password@...`.\n  - `socks5` should not be configured when `enable_http3` is enabled upstream, as the two do not belong to the same connection model.\n- Note: When the format is incorrect, the port is illegal, or the proxy host resolution fails, the upstream will not be created normally.",
    "upstreams[].idle_timeout":
      "- Type: `integer`; required: no; default value: none\n- Unit: seconds\n- Function: Define the connection pool idle connection retention time.",
    "upstreams[].max_conns":
      "- Type: `integer`; Required: No; Default: Automatic\n- Function: Define the upper limit of connection pool connections.\n- Range: `1..4096`.",
    "upstreams[].min_conns":
      "- Type: `integer`; Required: No; Default: `0`\n- Function: Define the minimum warmed connection count kept by the pool.\n- Range: `0..4096`, and it must not exceed the upstream's effective `max_conns`.\n- Note: When omitted, connections remain lazy and are not pre-created when the pool is created.",
    "upstreams[].insecure_skip_verify":
      "- Type: `boolean`; required: no; default value: `false`\n- Function: Control whether to skip TLS certificate verification.\n- Note: Applies only to self-signed certificates or controlled environments.",
    "upstreams[].timeout":
      "- Type: `duration`; Required: No; Default: `5s`\n- Function: Define a single upstream query timeout.",
    "upstreams[].enable_pipeline":
      "- Type: `boolean`; required: no; default value: protocol default behavior\n- Function: Control TCP or DoT pipeline.\n- Note: It can also be enabled directly in `addr` through `tcp+pipeline://` or `tls+pipeline://`.",
    "upstreams[].enable_http3":
      "- Type: `boolean`; required: no; default value: `false`\n- Function: Control whether DoH uses HTTP/3.\n- Note: It can also be enabled directly in `addr` through `h3://`.",
    "upstreams[].so_mark":
      "- Type: `integer`; required: no; default value: none\n- Function: Set Linux `SO_MARK`.",
    "upstreams[].bind_to_device":
      "- Type: `string`; Required: No; Default: None\n- Function: Set Linux `SO_BINDTODEVICE`.",
  },
  cache: {
    size: "- Type: `integer`; required: no; default value: `1024`\n- Function: Define the maximum number of entries in the cache.",
    lazy_cache_ttl:
      "- Type: `integer`; required: no; default value: none\n- Unit: seconds\n- Function: Enable lazy cache for positive successful responses.\n- Operational impact:\n  - The original TTL determines the fresh-hit window.\n  - `lazy_cache_ttl` determines the TTL returned for stale responses and allows stale data to be returned briefly after the original TTL expires.\n  - Stale hits refresh the cache asynchronously in the background.\n  - This setting does not shorten the original fresh TTL.",
    dump_file:
      "- Type: `string`; Required: No; Default: None\n- Function: Specify the cache persistence file path.",
    dump_interval:
      "- Type: `integer`; required: no; default value: `600`\n- Unit: seconds\n- Function: Define how often the cache is flushed to disk.",
    short_circuit:
      "- Type: `boolean`; Required: No; Default: Automatic\n- Function: Control whether to end subsequent execution immediately after a cache hit.\n- Description:\n  - When set to `false`, even if the cache has written the response, the subsequent execution chain will continue.\n  - If you want to avoid subsequent `forward` initiating queries again, you should use it in conjunction with `has_resp`, `accept` and other control flows in `sequence`.",
    cache_negative:
      "- Type: `boolean`; Required: No; Default: Automatic\n- Function: Control whether to cache NXDOMAIN and NODATA.",
    max_negative_ttl:
      "- Type: `integer`; required: no; default value: `300`\n- Unit: seconds\n- Function: Define the upper limit of negative cache TTL.",
    negative_ttl_without_soa:
      "- Type: `integer`; required: no; default value: `60`\n- Unit: seconds\n- Function: Define the fallback TTL for negative responses without SOA.",
    max_positive_ttl:
      "- Type: `integer`; required: no; default value: none\n- Unit: seconds\n- Function: Define the upper TTL limit for positive responses.",
    ecs_in_key:
      "- Type: `boolean`; required: no; default value: `false`\n- Function: Control whether the ECS scope is included in cache key calculation.",
  },
  fallback: {
    primary:
      "- Type: `string`; Required: Yes; Default: None\n- Function: Specify the main executor.",
    secondary:
      "- Type: `string`; Required: Yes; Default: None\n- Function: Specify the secondary executor.",
    threshold:
      "- Type: `integer`; required: no; default value: `0`\n- Unit: milliseconds\n- Function: Define the main path timeout or delay determination threshold.",
    always_standby:
      "- Type: `boolean`; required: no; default value: `false`\n- Function: Control whether the backup path is on standby at the same time as the main path.",
    short_circuit:
      "- Type: `boolean`; required: no; default value: `false`\n- Function: Control whether to stop the subsequent executor chain immediately after the primary/backup path selects the final response.",
  },
  hosts: {
    entries:
      "- Type: `array`; Required: No; Default: empty array\n- Function: Define inline hosts rules.\n- Rule format:\n  - `<Domain Name Rules> <ip1> <ip2> ...`\n- `<Domain Name Rules>` supports:\n  - `full:`\n  - `domain:`\n  - `keyword:`\n  - `regexp:`\n  - Unprefixed domain name (processed by `full:` exact match)",
    files:
      "- Type: `array`; Required: No; Default: empty array\n- Function: Specify the external hosts rule file list.",
    short_circuit:
      "- Type: `bool`; required: no; default value: `false`\n- Function: After hitting and generating a local response, whether to immediately stop the subsequent executor chain.",
  },
  arbitrary: {
    rules:
      "- Type: `array`; Required: No; Default: empty array\n- Function: Define an inline static record list.\n- Grammar:\n  - Each array item is parsed as an independent zone fragment.\n  - Supports `$ORIGIN`, `$TTL`, `$INCLUDE`, `$GENERATE`, owner inheritance, TTL unit writing, comments, quoted string, multi-line `(` `)` syntax.\n  - Supports direct text parsing for common record types, including `A`, `AAAA`, `CNAME`, `NS`, `PTR`, `DNAME`, `ANAME`, `MD`, `MF`, `MB`, `MG`, `MR`, `NSAPPTR`, `MX`,` RT`, `AFSDB`, `RP`, `MINFO`, `HINFO`, `TXT`, `SPF`, `AVC`, `RESINFO`, `SOA`, `SRV`, `NAPTR`, `CAA`.\n  - Other record types can be imported via the RFC3597 common syntax `TYPE#### \\# <len> <hex>`.\n  - Defaults to `3600` when TTL is omitted.",
    files:
      "- Type: `array`; Required: No; Default: empty array\n- Function: Specify the static record file list.\n- Syntax: Use the same zone parser to support the same syntax capabilities as `rules`.",
    short_circuit:
      "- Type: `bool`; required: no; default value: `false`\n- Function: After hitting and generating a local response, whether to stop the subsequent executor chain immediately.\n- Note: By default, only response is set and execution continues; when explicitly enabled, `Stop` is returned.",
  },
  redirect: {
    rules:
      "- Type: `array`; Required: No; Default: empty array\n- Function: Define inline redirection rules.\n- Rule format:\n  - `<domain name rule> <target domain name>`\n- `<Domain Name Rules>` supports:\n  - `full:`\n  - `domain:`\n  - `keyword:`\n  - `regexp:`\n  - Unprefixed domain name (processed by `full:` exact match)\n- Instructions for use: `redirect` itself does not resolve the target domain name. It usually needs to be used before `forward` in `sequence`, and `forward` generates the real response of the target domain name.",
    files:
      "- Type: `array`; Required: No; Default: empty array\n- Function: Specify the external redirection rule file list.\n- The file format is the same as `rules`, one per line; blank lines and `#` comments are ignored.",
  },
  ecs_handler: {
    forward:
      "- Type: `boolean`; required: no; default value: `false`\n- Function: Control whether to retain the existing ECS in the client request.",
    send: "- Type: `boolean`; required: no; default value: `false`\n- Function: Control whether to automatically replenish the ECS based on the source address when the request lacks ECS.",
    preset:
      "- Type: `string`; Required: No; Default: None\n- Function: Specify a fixed ECS source address.",
    mask4:
      "- Type: `integer`; required: no; default value: `24`\n- Function: Specify the IPv4 ECS prefix length.",
    mask6:
      "- Type: `integer`; required: no; default value: `48`\n- Function: Specify the IPv6 ECS prefix length.",
  },
  forward_edns0opt: {
    codes:
      "- Type: `array`; Required: No; Default: empty array\n- Function: Define the set of EDNS0 option codes that are allowed to be copied from the request to the response.\n- Operational impact:\n  - When not configured, the plug-in basically degrades to no operation.",
  },
  ttl: {
    fix: "- Type: `integer`; required: no; default value: none\n- Function: Fix all response TTL to the same value.",
    min: "- Type: `integer`; required: no; default value: none\n- Function: Define the TTL lower limit.",
    max: "- Type: `integer`; required: no; default value: none\n- Function: Define the TTL upper limit.",
  },
  ip_selector: {
    selection_mode:
      "- Type: `string`; Required: No; Default value: `first_success`\n- Optional values:\n  - `first_success`: Within the total waiting budget, the first successfully detected address takes priority\n  - `best_within_budget`: Collect successful detection results within the total waiting budget and select the address with the lowest latency\n  - `background`: This response maintains the original order, and the background asynchronously warms up the detection score cache\n- Function: Define the address preference policy in the existing A/AAAA response.\n- Operational impact:\n  - The plug-in only handles existing DNS responses and is not responsible for upstream racing.\n  - When the detection fails, times out or has no score, the original response will be retained as a backup.\n- Configuration requirements: Only OxiDNS native naming is accepted, compatible aliases are not provided.",
    probe_methods:
      '- Type: `array<string>` or comma separated `string`; required: no; default: `["tcp:443", "tcp:80"]`\n- Supported values:\n  - `tcp:<port>`: Perform TCP connect detection on the specified port of the target IP\n  - `ping`: best-effort ICMP detection, affected by platform and permissions\n  - `none`: No active detection, only use existing cache scores or original order\n- Function: Define the detection method used to score the response IP.\n- Configuration requirements:\n  - `none` cannot be combined with other detection methods.\n  - The port of `tcp:<port>` must be greater than 0.\n  - The order of methods will affect the staggered start sequence.',
    probe_stagger:
      "- Type: `integer`; required: no; default value: `200`\n- Unit: milliseconds\n- Function: Define the staggered start interval between multiple detection methods.\n- Operational impact:\n  - Smaller values will allow multiple methods to start concurrently faster.\n  - Larger values ​​will give earlier methods a clearer chance of taking precedence, especially affecting `first_success`.",
    probe_timeout:
      "- Type: `integer`; required: no; default value: `600`\n- Unit: milliseconds\n- Function: Define the timeout period for a single IP detection.\n- Configuration requirements: must be greater than 0.\n- Operational impact: Timeout will be recorded as a failure score and the DNS response will not be cleared or interrupted.",
    max_wait:
      "- Type: `integer`; required: no; default value: `1000`\n- Unit: milliseconds\n- Function: Define the total budget allowed to wait for detection results for this response.\n- Configuration requirements: must be greater than 0.\n- Operational impact: When the budget is exhausted, ordering is based on the successful score obtained; when there is no successful score, the original response is retained.",
    top_n:
      "- Type: `integer`; required: no; default value: `1`\n- Function: Keep the first N target addresses after sorting.\n- Special value: `0` means only rearrange without deleting any A/AAAA records.\n- Operational impact:\n  - Only the address records corresponding to the current query type are clipped.\n  - CNAME and non-target address records will be retained.",
    dnssec_policy:
      "- Type: `string`; Required: No; Default: `reorder_only`\n- Optional values:\n  - `reorder_only`: DNSSEC sensitive responses are only reordered and records are not deleted.\n  - `skip`: DNSSEC sensitive responses skip preferential processing completely\n- Function: Control how to handle when the request has DO bit or the response contains RRSIG that covers the current A/AAAA.\n- Runtime impact: Avoid clipping RRsets that may be overwritten by signatures by default.",
    max_parallel_probes:
      "- Type: `integer`; required: no; default value: `256`\n- Function: Limit the number of active detections performed by the current plug-in instance at the same time.\n- Configuration requirements: must be greater than 0.\n- Operational impact: When the upper limit is reached, new detections will be processed as failures and the original DNS response will be retained.",
    cache:
      "- Type: `object`; Required: No; Default value: enabled, capacity `4096`\n- Function: Configure IP detection score cache.\n- Operational impact:\n  - Cache hits avoid repeated proactive probing on request hot paths.\n  - Use different TTL for success and failure scoring.\n  - Background mode relies on this cache to provide ordering basis for subsequent requests.",
    "cache.enabled":
      "- Type: `boolean`; required: no; default value: `true`\n- Function: Control whether to enable detection score caching.\n- Operational impact: After closing, you can only rely on this detection or the original sequence every time you need to score.",
    "cache.size":
      "- Type: `integer`; required: no; default value: `4096`\n- Function: Define the target capacity of the detection score cache.\n- Configuration requirements: Must be greater than 0 when caching is enabled.\n- Operational impact: After the target capacity is exceeded, cache items that are less frequently accessed will be evicted first.",
    "cache.ttl":
      "- Type: `integer`; Required: No; Default: `3600`\n- Unit: seconds\n- Function: Define the retention time for successful detection scores.\n- Configuration requirements: Must be greater than 0 when caching is enabled.",
    "cache.failure_ttl":
      "- Type: `integer`; required: no; default value: `60`\n- Unit: seconds\n- Function: Define the retention time of failed detection scores.\n- Configuration requirements: Must be greater than 0 when caching is enabled.\n- Operational impact: Failure caching can prevent unreachable addresses from being repeatedly detected in a short period of time, while allowing faster recovery.",
  },
  prefer_ipv4: {
    cache:
      "- Type: `boolean`; required: no; default value: `true`\n- Function: Control whether to cache the preferred type existence status.",
    cache_ttl:
      "- Type: `integer`; Required: No; Default: `3600`\n- Unit: seconds\n- Function: Define preferred status cache duration.",
  },
  prefer_ipv6: {
    cache:
      "- Type: `boolean`; required: no; default value: `true`\n- Function: Control whether to cache the preferred type existence status.",
    cache_ttl:
      "- Type: `integer`; Required: No; Default: `3600`\n- Unit: seconds\n- Function: Define preferred status cache duration.",
  },
  black_hole: {
    mode: "- Type: `string`; Required: No; Default: `nxdomain` when `ips` is empty, `custom` when `ips` is configured\n- Values: `nxdomain`, `nodata`, `null`, `custom`, `refused`\n- Function: Defines the black_hole interception response type and covers every qtype.",
    ips: "- Type: `array`; Required: No; Default: empty array\n- Function: Define local synthetic return addresses for `custom` mode.\n- Operational impact:\n  - IPv4 addresses are used only for A responses.\n  - IPv6 addresses are used only for AAAA responses.\n  - Non-address qtypes and missing address families return NODATA.",
    short_circuit:
      "- Type: `bool`; Required: No; Default: `false`\n- Function: After generating an interception response, whether to immediately stop the subsequent executor chain.",
  },
  drop_resp: {
    args: "No independent configuration fields.",
  },
  reverse_lookup: {
    size: "- Type: `integer`; Required: No; Default: `65535`\n- Function: Define the upper limit of the reverse check cache capacity.",
    handle_ptr:
      "- Type: `boolean`; required: no; default value: `false`\n- Function: Control whether to directly use the anti-check cache to respond to PTR requests.",
    ttl: "- Type: `integer`; Required: No; Default: `7200`\n- Unit: seconds\n- Function: Define the cache TTL of IP to domain name mapping.",
  },
  query_summary: {
    msg: '- Type: `string`; Required: No; Default value: `"query summary"`\n- Function: Define summary log title.',
  },
  learn_domain: {
    provider:
      "- Type: `string`; Required: Yes; Default: None\n- Function: Reference the target `dynamic_domain_set` provider.\n- Constraints:\n  - Must be of type `dynamic_domain_set`, cannot refer to a normal `domain_set`.\n- Operational impact: All learned rules will be written to the provider's local files and its hot snapshot will be updated immediately.",
    phase:
      "- Type: `string`; Required: No; Default: `after`\n- Optional values:\n  - `before`: Before subsequent executor execution, press request question to learn.\n  - `after`: Execute subsequent links first, and then determine whether to learn based on the response.\n- Operational impact: `before` does not check response conditions; `after` will be filtered by `success_only`, `answer_required`, etc.",
    questions:
      "- Type: `string`; Required: No; Default: `first`\n- Optional values: `first`, `all`\n- Function: Control whether to learn only the first question or all questions; just keep the default for regular single question requests.",
    qtypes:
      '- Type: `array<string>`; Required: No; Default value: `["A", "AAAA"]`\n- Function: Only effective for the specified DNS query type.\n- Configuration requirement: Use uppercase record types, such as `A`, `AAAA`, `HTTPS`.',
    success_only:
      "- Type: `boolean`; required: no; default value: `true`\n- Function: Only learn when the RCODE is `NOERROR`.\n- Conditions of use: Only `phase: after` takes effect.",
    answer_required:
      "- Type: `boolean`; required: no; default value: `true`\n- Function: Only learn when the response contains answer records.\n- Conditions of use: Only `phase: after` takes effect.",
    rule_kind:
      "- Type: `string`; Required: No; Default: `full`\n- Optional values:\n  - `full`: Write `full:example.com` exact rules.\n  - `domain`: Write `domain:example.com` suffix rule.\n- Operational impact: The default precise rule can avoid accidentally expanding the matching range; when changed to `domain`, the entire subdomain will be hit.",
    async:
      "- Type: `boolean`; required: no; default value: `true`\n- Function: Control whether to continue execution only after joining the queue.\n- Operational impact:\n  - `true`: writes are queued asynchronously, with zero wait on the request path.\n  - `false`: synchronously wait for provider writing to complete on the current request path, limited by `timeout`.",
    error_mode:
      "- Type: `string`; Required: No; Default: `continue`\n- Optional values:\n  - `continue`: Only log the failure and continue the subsequent links.\n  - `stop`: Return to `Stop` after failure and truncate the current sequence branch.\n  - `fail`: Return executor error directly after failure.",
    timeout:
      "- Type: `duration`; required: no; default value: `1s`\n- Function: Limit the maximum time to wait for provider writing to complete when `async: false` is used.\n- Supported units: `ms`, `s`, `m`, `h`, `d`.",
  },
  query_recorder: {
    path: "- Type: `string`; Required: Yes\n- Function: Specify the SQLite file path of the current recorder.",
    queue_size:
      "- Type: `integer`; Required: No; Default: `8192`\n- Function: Define the bounded queue size between the request path and the background writer.",
    batch_size:
      "- Type: `integer`; required: no; default value: `256`\n- Function: Define how many records are written to SQLite in each background batch.",
    flush_interval_ms:
      "- Type: `integer`; required: no; default value: `200`\n- Function: Define the batch flush interval for the background writer.",
    memory_tail:
      "- Type: `integer`; required: no; default value: `1024`\n- Function: Define how many recent records are kept in memory for `stream?tail=n` replay.",
    retention_days:
      "- Type: `integer`; required: no; default value: `7`\n- Minimum value: `1`\n- Function: Define how many days logs are retained; expired data is periodically deleted.",
    cleanup_interval_hours:
      "- Type: `integer`; required: no; default value: `1`\n- Minimum value: `1`\n- Function: Define how often the expired-data cleanup task runs.",
  },
  metrics_collector: {
    name: '- Type: `string`; Required: No; Default value: `"default"`\n- Function: Define the name label of the current indicator collector.',
  },
  debug_print: {
    msg: '- Type: `string`; Required: No; Default value: `"debug print"`\n- Function: Define the log output title.',
  },
  sleep: {
    duration:
      "- Type: `integer`; required: no; default value: `0`\n- Unit: milliseconds\n- Function: Define the additional asynchronous waiting time of the current request on this executor.",
  },
  http_request: {
    method:
      "- Type: `string`; Required: Yes\n- Function: Specify HTTP methods, such as `GET`, `POST`, `PUT`, `PATCH`, `DELETE`.",
    url: "- Type: `string`; Required: Yes\n- Function: Target URL.\n- Description: Supports `${key}` placeholder interpolation; the rendered URL is only allowed to use `http` or `https`.",
    phase:
      "- Type: `string`; Required: No; Default: `after`\n- Optional values: `before`, `after`\n- Function: Control whether the request is sent before the downstream executor or after the downstream execution is completed.",
    async:
      "- Type: `boolean`; required: no; default value: `true`\n- Function: Control whether to use an asynchronous background queue to send, or to wait for HTTP completion synchronously on the current request path.",
    timeout:
      "- Type: `string`; Required: No; Default value: `5s`\n- Function: Limit the total timeout of a single HTTP call.\n- Supported units: `ms`, `s`, `m`, `h`, `d`",
    error_mode:
      "- Type: `string`; Required: No; Default: `continue`\n- Optional values:\n  - `continue`: only log on failure, then continue with subsequent links\n  - `stop`: Return `Stop` after failure\n  - `fail`: Return executor error directly after failure",
    headers:
      "- Type: `map<string,string>`; required: no; default value: empty\n- Function: Attach HTTP request header.\n- Note: header value supports `${key}` placeholder interpolation.",
    query_params:
      "- Type: `map<string,string>`; required: no; default value: empty\n- Function: Append additional parameters to URL query.\n- Note: value supports `${key}` placeholder interpolation; it will be sent together with the URL's own query.",
    body: "- Type: `string`; Required: No\n- Function: Original string request body.\n- Description: Supports `${key}` placeholder interpolation; optional `args.content_type`.",
    json: "- Type: `object | array`; Required: No\n- Function: Send the request body in JSON mode.\n- Note: `Content-Type: application/json` will be automatically set; all string leaf nodes support `${key}` placeholder interpolation, and non-string values ​​will be retained as they are.",
    form: "- Type: `map<string,string>`; Required: No\n- Function: Send the form in `application/x-www-form-urlencoded` mode.\n- Note: value supports `${key}` placeholder interpolation; the corresponding `Content-Type` will be automatically set.",
    content_type:
      "- Type: `string`; Required: No\n- Function: Specify `Content-Type` for the original `args.body`.\n- Note: It can only be used with `args.body` and cannot be used with `args.json` or `args.form` at the same time.",
    socks5:
      "- Type: `string`; Required: No\n- Function: Specify SOCKS5 proxy.\n- Note: The format is consistent with `upstream[].socks5`, supporting `host:port`, `username:password@host:port` and IPv6 with square brackets.",
    insecure_skip_verify:
      "- Type: `boolean`; required: no; default value: `false`\n- Function: Whether to skip HTTPS certificate verification.",
    max_redirects:
      "- Type: `integer`; required: no; default value: `5`\n- Function: Limit the maximum number of redirects to follow.",
    queue_size:
      "- Type: `integer`; required: no; default value: `256`\n- Function: The capacity of the background sending queue in asynchronous mode.",
  },
  script: {
    command:
      "- Type: `string`; Required: Yes\n- Function: The command path or command name to be executed.\n- Note: This field does not support template replacement to avoid the command itself from drifting during runtime.",
    args: "- Type: `array<string>`; required: no; default value: empty\n- Function: Array of parameters passed to the command.\n- Note: Each item supports `${key}` placeholder interpolation.",
    env: "- Type: `map<string,string>`; required: no; default value: empty\n- Function: The key-value pair appended to the child process environment variable.\n- Note: value supports `${key}` placeholder interpolation; existing environment variables of the parent process will not be cleared.",
    cwd: "- Type: `string`; Required: No; Default: None\n- Function: Specify the working directory when the script is run.",
    timeout:
      "- Type: `string`; Required: No; Default value: `5s`\n- Function: Limit the execution time of a single script.\n- Supported units: `ms`, `s`, `m`, `h`, `d`",
    error_mode:
      "- Type: `string`; Required: No; Default: `continue`\n- Optional values:\n  - `continue`: only log on failure or timeout, then return `Next`\n  - `stop`: Return `Stop` after failure or timeout\n  - `fail`: Return an error directly in case of failure or timeout",
    max_output_bytes:
      "- Type: `usize`; Required: No; Default: `4096`\n- Function: Limit the capture length of stdout/stderr, and only mark the excess part as truncation.",
  },
  ipset: {
    set_name4:
      "- Type: `string`; Required: No; Default: None\n- Function: Specify the ipset name to write the IPv4 address.",
    set_name6:
      "- Type: `string`; Required: No; Default: None\n- Function: Specify the ipset name to write the IPv6 address.",
    mask4:
      "- Type: `integer`; required: no; default value: `24`\n- Function: Specify the prefix length used when writing IPv4 addresses to ipset.",
    mask6:
      "- Type: `integer`; required: no; default value: `32`\n- Function: Specify the prefix length used when writing IPv6 addresses to ipset.",
  },
  nftset: {
    ipv4: "- Type: `object`; Required: No; Default: None\n- Function: Define IPv4 target nftables set.\n- Subfield:\n  - `table_family`\n  - `table_name`\n  - `set_name`\n  - `mask`",
    ipv6: "- Type: `object`; Required: No; Default: None\n- Function: Define IPv6 target nftables set.\n- Subfield:\n  - `table_family`\n  - `table_name`\n  - `set_name`\n  - `mask`",
    table_family4:
      "- Type: `string`; Required: No; Default: None\n- Function: Define nftables table family of IPv4/IPv6 respectively under compatible writing method.",
    table_family6:
      "- Type: `string`; Required: No; Default: None\n- Function: Define nftables table family of IPv4/IPv6 respectively under compatible writing method.",
    table_name4:
      "- Type: `string`; Required: No; Default: None\n- Function: Define the nftables table names of IPv4/IPv6 respectively in compatible writing.",
    table_name6:
      "- Type: `string`; Required: No; Default: None\n- Function: Define the nftables table names of IPv4/IPv6 respectively in compatible writing.",
    set_name4:
      "- Type: `string`; Required: No; Default: None\n- Function: Define IPv4/IPv6 set names separately in compatible writing.",
    set_name6:
      "- Type: `string`; Required: No; Default: None\n- Function: Define IPv4/IPv6 set names separately in compatible writing.",
    mask4:
      "- Type: `integer`; Required: No; Default: Implementation determined\n- Function: Define the IPv4/IPv6 prefix length separately under compatible writing methods.",
    mask6:
      "- Type: `integer`; Required: No; Default: Implementation determined\n- Function: Define the IPv4/IPv6 prefix length separately under compatible writing methods.",
  },
  ros_address_list: {
    address:
      "- Type: `string`; Required: Yes; Default: None\n- Function: Specify the RouterOS API service address, usually written as `host:port`. This address will be used to establish a management connection after the plug-in is started and maintain synchronization with the device during operation.\n- Configuration recommendations: When using the RouterOS API plaintext port, it is usually `8728`. If an encrypted API is deployed, the actual port should be filled in.",
    username:
      "- Type: `string`; Required: Yes; Default: None\n- Function: Specify the RouterOS API login username. This account needs to have permission to read and maintain the target `address-list`.\n- Configuration suggestions: It is recommended to create a dedicated account for this plug-in to isolate the scope of permissions and audit records.",
    password:
      "- Type: `string`; Required: Yes; Default: None\n- Function: Specify the RouterOS API login password. Plugin initialization, reconnection, and background synchronization all rely on this credential.\n- Note: Direct exposure of real passwords in public repositories or shared samples should be avoided.",
    connect_timeout:
      "- Type: `u64`; Required: No; Default: `5`\n- Function: Specify the maximum wait time, in seconds, for establishing a RouterOS API connection.\n- Note: Must be greater than `0`. Increase it if the management network or RouterOS API occasionally responds slowly.",
    send_timeout:
      "- Type: `u64`; Required: No; Default: `5`\n- Function: Specify the maximum wait time, in seconds, for sending one RouterOS API command.\n- Note: Must be greater than `0`. The default is usually sufficient.",
    receive_timeout:
      "- Type: `u64`; Required: No; Default: `5`\n- Function: Specify the maximum wait time, in seconds, for the next chunk of RouterOS API response data.\n- Configuration recommendation: Prefer a dedicated, size-controlled `address-list` for OxiDNS. Avoid connecting the plugin to an existing large shared list. Increase this value, for example to `30` or `60`, only when slow legacy list queries or a slow RouterOS management plane cannot be avoided.",
    async:
      "- Type: `bool`; required: no; default value: `true`\n- Function: Control whether the address writing behavior is asynchronous. When enabled, the DNS response path is only responsible for delivery tasks, and the background manager completes the interaction with RouterOS.\n- Impact: Asynchronous mode helps reduce the risk of request path blocking; after closing, it will be changed to synchronous submission, which is more suitable for scenarios that require immediate confirmation of submission results.",
    address_list4:
      "- Type: `string`; Required: No; Default: None\n- Function: Specify the target `address-list` name for writing IPv4 addresses. After the plugin extracts the A records from the DNS answer, it writes to this list.\n- Configuration recommendation: If the policy only handles IPv4, at least this item should be configured.",
    address_list6:
      "- Type: `string`; Required: No; Default: None\n- Function: Specify the target `address-list` name for IPv6 address writing. The plug-in writes to this list after extracting the AAAA records from the DNS response.\n- Configuration recommendation: If the policy needs to cover IPv6, this item should be configured at the same time, and corresponding matching and routing rules should be established on the RouterOS side.",
    comment_prefix:
      "- Type: `string`; Required: No; Default: `fdns`\n- Function: Specifies the comment prefix used by the plug-in when writing RouterOS entries. This prefix is ​​used to distinguish dynamic entries and resident entries created by OxiDNS to facilitate subsequent refresh, reload and cleanup.\n- Note: This value and the plugin `tag` should not contain `;` or `=` to avoid affecting the internal tag format.",
    persistent:
      "- Type: `object`; Required: No; Default: None\n- Function: Define a static address set that needs to be retained for a long time. This part does not rely on DNS response triggering, can be directly synchronized to RouterOS after the plug-in is started, and is maintained by background reconcile to maintain consistency.\n- Subfield:\n  - `ips`\n  - `files`",
    "persistent.ips":
      "- Type: `array<string>`; required: no; default value: empty\n- Function: Declare the resident IP or CIDR network segment inline. Suitable for fixed strategy objects that are small in number and change infrequently.\n- Supported formats: single IPv4, single IPv6, IPv4 CIDR, IPv6 CIDR.",
    "persistent.files":
      "- Type: `array<string>`; required: no; default value: empty\n- Function: Load the resident address set from an external file. Suitable for address lists that need to be generated by other systems, maintained centrally, or managed in batches.\n- Behavioral note: These files are only read once when the plugin is initialized. If the file changes need to take effect, you need to reload the plug-in or application.",
    min_ttl:
      "- Type: `u64`; Required: No; Default: `60`\n- Function: Define the minimum TTL allowed for dynamic address items. When the TTL in a DNS response is too small or zero, the plugin will increase it to that value before writing to RouterOS.\n- Applicable scenarios: Used to avoid management plane jitter caused by high-frequency refresh.",
    max_ttl:
      "- Type: `u64`; Required: No; Default: `3600`\n- Function: Define the maximum TTL allowed for dynamic address items. When the TTL in a DNS response is too large, the plugin truncates to that limit.\n- Applicable scenarios: Used to limit the residence time of policy items in network devices and reduce the risk of address staleness.",
    fixed_ttl:
      "- Type: `u64`; Required: No; Default: None\n- Function: Specify a fixed TTL for all dynamically written items. After configuring this item, the plug-in will no longer use the original TTL in the DNS record, and will no longer be affected by the interval clipping of `min_ttl` and `max_ttl`. If set to `0`, dynamic items will not set RouterOS `timeout`.\n- Applicable scenarios: Suitable for scenarios that require a unified refresh cycle, easy operation and maintenance estimation, and policy convergence.",
    cleanup_on_shutdown:
      "- Type: `bool`; required: no; default value: `true`\n- Function: Control whether to clean up the entries managed by the plug-in when it exits. When enabled, the plug-in will delete the RouterOS address entries written by itself during the normal shutdown phase and can identify the owned RouterOS address entries.\n- Impact: After turning off this option, written entries will continue to be retained in RouterOS, which is suitable for scenarios that require policy status to be retained across process restarts.",
  },
  upgrade: {
    force:
      "- Type: `bool`; required: no; default value: `false`\n- Function: Even if the target release is not newer than the current version, continue to download, verify and replace it.",
    cleanup:
      "- Type: `bool`; required: no; default value: `true`\n- Function: Clean up `cache_dir` and `backup_dir` after successful upgrade.",
    repository:
      "- Type: `string`; Required: No; Default: `svenshi/oxidns`\n- Function: GitHub warehouse.",
    asset:
      "- Type: `string`; required: no; default value: `auto`\n- Function: Release asset name; `auto` will select archive based on the current platform and compilation version.\n- Priority: `bundle` derivation will be skipped when explicitly filling in asset.",
    bundle:
      "- Type: `auto | full | standard | minimal`; required: no; default value: `auto`\n- Function: Select the release compiled version when `asset: auto` is used. `full` uses the old asset name, `standard` / `minimal` uses the slim asset name with bundle prefix.",
    github_token:
      "- Type: `string`; Required: No; Default: None\n- Purpose: GitHub personal access token, used to increase API rate limits or access private repositories.\n- Description: Will be used as the Bearer token for GitHub API requests.",
    cache_dir:
      "- Type: `path`; Required: No; Default: None\n- Function: Download cache directory.",
    backup_dir:
      "- Type: `path`; Required: No; Default: None\n- Function: Back up directory before replacement.",
    webui_dir:
      "- Type: `path`; Required: No; Default: `./webui`\n- Function: The directory where WebUI static resources are installed during upgrade should be consistent with `api.http.webui.root`.",
    skip_webui:
      "- Type: `bool`; required: no; default value: `false`\n- Function: When set to `true`, only binary files will be replaced and WebUI directory upgrade will be skipped.",
    no_restart:
      "- Type: `bool`; required: no; default value: `false`\n- Function: When set to `true`, automatic restart will not be triggered after successful upgrade.",
    timeout:
      "- Type: `duration`; Required: No; Default value: `30s`\n- Function: Limit the total waiting time of the upgrade process.",
    socks5:
      "- Type: `string`; Required: No; Default: None\n- Function: Upgrade the SOCKS5 proxy used when downloading.",
    insecure_skip_verify:
      "- Type: `boolean`; required: no; default value: `false`\n- Function: Skip HTTPS certificate verification when downloading upgrades.",
  },
  download: {
    downloads:
      "- Type: `array`; Required: Yes; Default: None\n- Function: Download one or more `http` / `https` files to the local directory, and overwrite the target file after the new content is completely written.\n- Operational impact:\n  - Download items are executed serially in the order of declaration.\n  - If a single download fails, only a warning log will be written, and subsequent items will not be prevented from continuing to download.\n  - The target directory will be automatically created if it does not exist.",
    "downloads[].url":
      "- Type: `string`; Required: Yes; Default: None\n- Function: `http` / `https` URL of the download item.",
    "downloads[].dir":
      "- Type: `path`; Required: Yes; Default: None\n- Function: The target directory for downloading items.",
    "downloads[].filename":
      "- Type: `string`; Required: No; Default: Deduced from URL path\n- Function: The target file name of the download item.",
    timeout:
      "- Type: `duration`; Required: No; Default value: `30s`\n- Function: Download timeout.",
    socks5:
      '- Type: `string`; Required: No; Default: None\n- Function: All download connections will be initiated through this SOCKS5 proxy.\n- Supported formats: `host:port`, `username:password@host:port`, IPv6 needs to be written as `"[::1]:1080"`.',
    startup_if_missing:
      "- Type: `boolean`; required: no; default value: `true`\n- Function: Check the target file at startup. Missing items will be automatically downloaded before other plug-ins are initialized.\n- Note: Only missing files will be filled in, and existing files will not be forced to be overwritten every time it is started.",
  },
  reload_provider: {
    args: '- Type: `array[string]`; Required: Yes; Default: None\n- Function: Execute targeted provider reload one by one in the order declared in `args`.\n- Support element: provider reference, for example `"$geoip_cn"`.\n- Operational impact: Only the internal data of the provider is refreshed, and tags, dependencies or other plug-in configurations are not modified.',
  },
  reload: {
    args: "No independent configuration fields. When executed, an application-level full reload that is the same as the management API `POST /reload` will be triggered.",
  },
  cron: {
    jobs: "- Type: `array`; Required: Yes; Default: None\n- Function: Define one or more background tasks.\n- Operational impact:\n  - The array cannot be empty.\n  - Each task independently maintains its own scheduling status and overlap protection.",
    timezone:
      "- Type: `string`; Required: No; Default: System local time zone\n- Function: Specify the time zone for all `schedule` tasks under the current `cron` plugin.\n- Operational impact:\n  - Only takes effect on `schedule`.\n  - If not configured, the system's local time zone will be used; if it cannot be obtained, it will fall back to `UTC`.\n  - The IANA time zone name should be filled in, such as `Asia/Shanghai`, `UTC`, `America/Los_Angeles`.",
    "jobs[].name":
      "- Type: `string`; Required: Yes; Default: None\n- Function: task name, used for logs and runtime identification.\n- Operational impact:\n  - Must be unique within the same `cron` plugin.",
    "jobs[].schedule":
      "- Type: `string`; required: choose one from `interval`; default value: none\n- Function: Schedule tasks using standard 5-field cron expressions.\n- Rule description:\n  - Only `minute hour day month day-of-week` is supported.\n  - Second-level cron is not supported.\n  - Calculate next trigger time in `args.timezone` or system local time zone.",
    "jobs[].interval":
      "- Type: `string`; required: choose one from `schedule`; default value: none\n- Function: Schedule tasks with simple fixed intervals.\n- Supported formats:\n  - `5m`\n  - `1h`\n  - `1d`\n- Operational impact:\n  - Minimum granularity is `1m`.\n  - After startup, it will wait for a full interval before triggering for the first time.",
    "jobs[].executors":
      "- Type: `array`; Required: Yes; Default: None\n- Function: Define the executor list to be executed sequentially when the task is triggered.\n- Supported forms:\n  - `$tag`: explicitly refers to an existing executor\n  - `tag`: bare tag reference\n  - Shortcut expressions, such as `debug_print cron refresh`\n- Operational impact:\n  - The array cannot be empty.\n  - Even if an executor returns `Stop`, sets a response, or reports an execution error, subsequent executors will continue to execute.",
  },
  any_match: {
    args: '`args` of `any_match` is a list of matcher expressions.\n\n- Type: `array[string]`; Required: Yes; Default: None\n- Support elements:\n  - matcher tag reference (such as `"$match_tag"`)\n  - Shortcut matcher expressions (such as `"qname domain:example.com"`)\n  - Negate matcher expressions (e.g. `"!$has_resp"`)\n- Operational impact:\n  - Judge in order of configuration, and short-circuit and return `true` immediately after hitting any one.\n  - Returns `false` if all misses.',
  },
  qname: {
    args: "The `args` of `qname` takes the form of a list of rules, and each element in the list takes effect independently.\n\n- Type: `array`; Required: Yes; Default: None\n- Function: Define the source of domain name matching rules.\n- Support elements:\n  - Domain name expression (supports `full:`, `domain:`, `keyword:`, `regexp:`, if there is no prefix, it will be processed as `domain:`)\n  - Provider references with domain name matching capabilities, such as `domain_set`, `geosite`\n  - File reference\n- Operational impact:\n  - When any question domain name in the current request matches any rule, matcher returns `true`.",
  },
  question: {
    args: '- `args`\n  - Type: `array[string]`; Required: Yes; Default: None\n  - Function: Use the `"$provider_tag"` form to reference the provider that implements `contains_question`.',
  },
  qtype: {
    args: "`args` of `qtype` is a list of types.\n\n- Type: `array`; Required: Yes; Default: None\n- Function: Define the set of query types that are allowed to hit.\n- Supports both enum text and decimal values, such as `A` / `AAAA` / `PTR` or `1` / `28` / `12`; both formats can be mixed in the same list.\n- Unknown or future extension types can continue to use numeric matching.\n- Operational impact:\n  - Returns `true` when any question type in the request hits the configuration collection.",
  },
  qclass: {
    args: "`args` of `qclass` is a list of categories.\n\n- Type: `array`; Required: Yes; Default: None\n- Function: Define the set of query categories that are allowed to hit.\n- Supports both enum literals and decimal values, such as `IN` / `CH` / `HS` or `1` / `3` / `4`; both formats can be mixed in the same list.\n- Unknown or future expanded categories can continue to use numeric matching.\n- Operational impact:\n  - Returns `true` if any issue category in the request hits the configuration collection.",
  },
  client_ip: {
    args: "The `args` of `client_ip` takes the form of a list of rules.\n\n- Type: `array`; Required: Yes; Default: None\n- Function: Define client source address matching conditions.\n- Support elements:\n  - Single IP\n  - CIDR\n  - `ip_set` reference\n- Operational impact:\n  - As long as the client source address matches any rule, matcher returns `true`.",
  },
  resp_ip: {
    args: "The `args` of `resp_ip` takes the form of a list of rules.\n\n- Type: `array`; Required: Yes; Default: None\n- Function: Define response address matching conditions.\n- Support elements:\n  - Single IP\n  - CIDR\n  - `ip_set` reference\n- Operational impact:\n  - Only check A/AAAA addresses in the response answer area.\n  - Returns `true` if any answer address is hit.",
  },
  ptr_ip: {
    args: "The `args` of `ptr_ip` takes the form of a list of rules.\n\n- Type: `array`; Required: Yes; Default: None\n- Function: Define the address matching conditions for PTR request name resolution.\n- Support elements:\n  - Single IP\n  - CIDR\n  - `ip_set` reference\n- Operational impact:\n  - Valid only for PTR queries.\n  - Returns `true` when the address resolved from the PTR request name matches any rule.",
  },
  cname: {
    args: "The `args` of `cname` take the form of a list of rules.\n\n- Type: `array`; Required: Yes; Default: None\n- Function: Define CNAME target name matching conditions.\n- Support elements:\n  - Domain name expression (supports `full:`, `domain:`, `keyword:`, `regexp:`, if there is no prefix, it will be processed as `domain:`)\n  - Provider references with domain name matching capabilities, such as `domain_set`, `geosite`\n  - File reference\n- Operational impact:\n  - Only check the CNAME target in the response.\n  - Returns `true` when any CNAME target is hit.",
  },
  rcode: {
    args: "`args` of `rcode` is a list of rcodes.\n\n- Type: `array`; Required: Yes; Default: None\n- Function: Define the set of response codes that can be hit.\n- Supports both enum literals and decimal values, such as `NOERROR` / `SERVFAIL` / `NXDOMAIN` or `0` / `2` / `3`; both formats can be mixed in the same list.\n- Unknown or future extended response codes can continue to be matched using numeric values.\n- Operational impact:\n  - Return `true` only if there is already a response in the context and rcode hits the configuration collection.",
  },
  has_resp: {
    args: "No independent configuration fields.",
  },
  has_wanted_ans: {
    args: "No independent configuration fields.",
  },
  mark: {
    args: "`args` of `mark` is a list of marks.\n\n- Type: `array`; Required: Yes; Default: None\n- Function: Define the set of context tags that can be hit.\n-Support value:\n  - mark value as an unsigned integer\n- Operational impact:\n  - Returns `true` whenever context marks intersect with configuration marks.",
  },
  env: {
    args: "`args` of `env` is a list of environment variable conditions.\n\n- Type: `array`; Required: Yes; Default: None\n- Supported forms:\n  - `KEY=VALUE`: The variable exists and the value matches exactly. It is recommended for environment variable equal value matching.\n  - `KEY:VALUE`: Equivalent to `KEY=VALUE`, reserved as an alias for regular expression style\n  - `KEY`, `KEY:` or `KEY=`: hit if the variable exists\n- Note: Each string in the array is a complete expression and will not be split by commas or blanks; two naked parameters such as `PROFILE` and `prod` represent two existence checks and do not mean `PROFILE == prod`.\n- Running impact: Return `true` only when all conditions are met; variable values ​​are cached when the plug-in is initialized.",
  },
  random: {
    args: "`args` of `random` only accepts a probability value.\n\n- Type: `array`; Required: Yes; Default: None\n- Value range: `0.0` to `1.0`\n- Function: Define the probability of returning `true` for this match.\n- Operational impact:\n  - `0.0` means always miss.\n  - `1.0` means always hit.",
  },
  rate_limiter: {
    qps: "- Type: `number`; Required: No; Default: `20`\n- Function: Define the token replenishment rate per second.\n- Operational impact:\n  - The larger the value, the more requests allowed to pass per unit time.",
    burst:
      "- Type: `integer`; required: no; default value: `40`\n- Function: Define the upper limit of token bucket capacity.\n- Operational impact:\n  - The larger the value, the more burst requests allowed in a short period of time.",
    mask4:
      "- Type: `integer`; required: no; default value: `32`\n- Function: Define IPv4 client aggregation granularity.\n- Operational impact:\n  - The smaller the value, the easier it is for multiple IPv4 clients to share the same throttling bucket.",
    mask6:
      "- Type: `integer`; required: no; default value: `48`\n- Function: Define IPv6 client aggregation granularity.\n- Operational impact:\n  - The smaller the value, the easier it is for multiple IPv6 clients to share the same throttling bucket.",
  },
  string_exp: {
    args: "The `args` of `string_exp` can be a string or an array of strings.\n\n- Type: `string` or `array`\n- Required: Yes\n- Default: None\n- Function: Define a complete string expression.\n- Expression composition:\n  - Data source `source`\n  - Matching operation `op`\n  - one or more parameters\n- Operational impact:\n  - Get values from context by expression and perform string matching.",
  },
  _true: {
    args: "No independent configuration fields.",
  },
  _false: {
    args: "No independent configuration fields.",
  },
  domain_set: {
    exps: "- Type: `array`; Required: No; Default: empty array\n- Function: Define a list of inline domain name expressions.\n- Support content:\n  - `full:`\n  - `domain:`\n  - `keyword:`\n  - `regexp:`\n  - Unprefixed domain name (processed by `domain:`)\n- Operational impact:\n  - Compiled during the initialization phase into a set of rules that can be directly matched.",
    files:
      "- Type: `array`; Required: No; Default: empty array\n- Function: Specify the path list of external rule files.\n- Documentation requirements:\n  - One rule per line.\n  - Blank lines and comment lines are ignored.\n- Operational impact:\n  - The file contents will be re-read during initialization or `reload_provider`, and compiled into the current provider's local matcher.",
    sets: "- Type: `array`; Required: No; Default: empty array\n- Function: Reference other providers with domain name matching capabilities.\n- Constraints:\n  - Allows to reference any provider with domain name matching capabilities, such as `domain_set`, `geosite`, `adguard_rule`.\n- Operational impact:\n  - The current provider only saves the stable handle of the referenced provider and does not copy its rules.\n  - After the downstream provider is reloaded separately, the current `domain_set` can see the new results without reloading.",
  },
  dynamic_domain_set: {
    path: "- Type: `string`; Required: Yes; Default: None\n- Function: Specify the local rule file managed by this provider.\n- File requirements:\n  - machine-managed: maintained by the plugin; handwritten comments are not preserved.\n  - One rule per line. Supports `full:`, `domain:`, `keyword:`, `regexp:`, and domains without a prefix (parsed as `domain:`).\n  - Empty lines and comment lines starting with `#` are ignored when loading.\n- Operational impact:\n  - The file is created automatically if it does not exist.\n  - Existing files are re-read on startup and `reload_provider`.\n  - Manual edits are not detected automatically; reload the provider manually after editing.",
    bootstrap_rules:
      "- Type: `array`; Required: No; Default: empty array\n- Function: Initial rules written when the `path` file does not exist.\n- Supported content: `full:`, `domain:`, `keyword:`, `regexp:`, and domains without a prefix.\n- Operational impact:\n  - This field has no effect once the file already exists.\n  - It is only used for the first bootstrap. Use `learn_domain` or the management API for later additions.",
    queue_size:
      "- Type: `integer`; required: no; default value: `1024`\n- Configuration requirement: must be greater than 0.\n- Function: Define the bounded queue size from the request path to the background writer for automatic learning.\n- Operational impact: When the queue is full, new append requests may block or be dropped, depending on the caller.",
    batch_size:
      "- Type: `integer`; required: no; default value: `256`\n- Configuration requirement: must be greater than 0.\n- Function: Define the batch flush threshold for background appends.\n- Operational impact: Larger values write more rules per flush and concentrate CPU/disk work into fewer flushes.",
    flush_interval_ms:
      "- Type: `integer`; required: no; default value: `200`\n- Unit: milliseconds\n- Configuration requirement: must be greater than 0.\n- Function: Define the scheduled flush interval for background appends.\n- Operational impact: Together with `batch_size`, this controls write cadence. Smaller values make new rules visible sooner but flush to disk more often.",
  },
  geosite: {
    file: "- Type: `string`; Required: Yes\n- Function: Specify the `geosite.dat` file path.",
    selectors:
      "- Type: `array`; Required: No; Default: empty array\n- Function: Extract some rules by code, and also support `code@attribute` syntax to further filter by attribute.\n- Behavior:\n  - Case-insensitive exact matches.\n  - Union of multiple selectors.\n  - When not set or empty array, loads the union of all rules for the entire dat file.\n  - For example, `category-games@cn` means to extract only the rules with `cn` attribute in `category-games`.",
  },
  adguard_rule: {
    rules:
      "- Type: `array`; Required: No; Default: empty array\n- Function: Provides a subset of inline AdGuard Home DNS rules.\n- Supported content: basic domain name rules, `@@`, `important`, `badfilter`, `denyallow`, request side `dnstype`.",
    files:
      "- Type: `array`; Required: No; Default: empty array\n- Function: Load a subset of AdGuard Home DNS rules from an external rules file.\n- Runtime impact: File contents will be reread during initialization or `reload_provider`.",
  },
  ip_set: {
    ips: "- Type: `array`; Required: No; Default: empty array\n- Function: Define inline IP or CIDR rule list.\n- Support content:\n  - Single IPv4 address\n  - Single IPv6 address\n  - IPv4 CIDR\n  - IPv6 CIDR\n- Operational impact:\n  - Rules are compiled into address matching structures during the initialization phase.",
    files:
      "- Type: `array`; Required: No; Default: empty array\n- Function: Specify the external IP rule file path list.\n- Documentation requirements:\n  - One IP or CIDR rule per line.\n  - Blank lines and comment lines are ignored.\n- Operational impact:\n  - The file contents will be re-read during initialization or `reload_provider`, and compiled into the current provider's local matcher.",
    sets: "- Type: `array`; Required: No; Default: empty array\n- Function: Reference other `ip_set` instances.\n- Constraints:\n  - Allows to reference any provider with IP matching capabilities, such as `ip_set`, `geoip`.\n- Operational impact:\n  - The current provider only saves the stable handle of the referenced provider and does not copy its rules.\n  - After the downstream provider is reloaded separately, the current `ip_set` can see the new results without reloading.",
  },
  geoip: {
    file: "- Type: `string`; Required: Yes\n- Function: Specify the `geoip.dat` file path.",
    selectors:
      "- Type: `array`; Required: No; Default: empty array\n- Function: Extract some rules by code.\n- Behavior:\n  - Case-insensitive exact matching.\n  - Union of multiple selectors.\n  - When not set or empty array, loads the union of all CIDRs for the entire dat file.",
  },
} as const satisfies LocaleResourceShape<typeof zhCNDocs>;
