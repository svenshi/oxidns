export const zhCNPluginDefined = {
  pluginTypes: {
    labels: {
      server: "Server",
      executor: "Executor",
      matcher: "Matcher",
      provider: "Provider",
    },
    descriptions: {
      server: "入口服务",
      executor: "执行器",
      matcher: "匹配器",
      provider: "数据源",
    },
    statuses: {
      running: "运行中",
      stopped: "已停止",
      error: "异常",
    },
  },
  kinds: {
    udp_server: {
      name: "UDP Server",
      description: "标准 DNS UDP 入口，把请求交给指定执行器",
      fields: {
        entry: {
          label: "入口执行器",
          description:
            "指定处理该监听器全部请求的入口执行器，通常为sequence插件。",
        },
        listen: {
          label: "监听地址",
          description: "指定 UDP 监听地址。",
          placeholder: "0.0.0.0:53",
        },
      },
      metrics: {
        labels: {
          server_request_total: "请求总数",
          server_completed_total: "完成",
          server_controlled_total: "提前结束",
          server_failed_total: "失败(SERVFAIL)",
          server_inflight: "处理中",
          server_latency_count: "延迟样本",
          server_latency_sum_ms: "延迟累计(ms)",
        },
        help: {
          server_request_total: "服务器接收并处理的入站 DNS 请求总数。",
          server_completed_total: "执行器链正常执行完毕的请求总数。",
          server_controlled_total:
            "执行器主动停止（stop/return）而提前结束的请求总数。",
          server_failed_total: "因入口执行器失败而返回 SERVFAIL 的请求总数。",
          server_inflight: "当前服务器正在处理中的请求数量。",
          server_latency_count: "纳入服务器延迟统计的已完成请求数。",
          server_latency_sum_ms: "所有已完成请求的总处理延迟（毫秒）。",
        },
        derived: {
          "latency:server": "平均延迟",
          "percent:server_failed_total/server_request_total": "失败率",
        },
      },
    },
    tcp_server: {
      name: "TCP / DoT Server",
      description: "DNS over TCP；配置证书后作为 DoT 入口",
      fields: {
        entry: {
          label: "入口执行器",
          description: "指定 TCP 或 DoT 请求进入策略链时使用的入口执行器。",
        },
        listen: {
          label: "监听地址",
          description: "指定 TCP 监听地址。",
          placeholder: ":53",
        },
        cert: {
          label: "TLS 证书",
          description: "指定 TLS 证书文件路径。",
          placeholder: "/etc/oxidns/server.crt",
        },
        key: {
          label: "TLS 私钥",
          description: "指定 TLS 私钥文件路径。",
          placeholder: "/etc/oxidns/server.key",
        },
        idle_timeout: {
          label: "空闲超时(秒)",
          description: "指定连接空闲超时设置。",
        },
      },
      metrics: {
        labels: {
          server_request_total: "请求总数",
          server_completed_total: "完成",
          server_controlled_total: "提前结束",
          server_failed_total: "失败(SERVFAIL)",
          server_inflight: "处理中",
          server_latency_count: "延迟样本",
          server_latency_sum_ms: "延迟累计(ms)",
        },
        help: {
          server_request_total: "服务器接收并处理的入站 DNS 请求总数。",
          server_completed_total: "执行器链正常执行完毕的请求总数。",
          server_controlled_total:
            "执行器主动停止（stop/return）而提前结束的请求总数。",
          server_failed_total: "因入口执行器失败而返回 SERVFAIL 的请求总数。",
          server_inflight: "当前服务器正在处理中的请求数量。",
          server_latency_count: "纳入服务器延迟统计的已完成请求数。",
          server_latency_sum_ms: "所有已完成请求的总处理延迟（毫秒）。",
        },
        derived: {
          "latency:server": "平均延迟",
          "percent:server_failed_total/server_request_total": "失败率",
        },
      },
    },
    http_server: {
      name: "HTTP / DoH Server",
      description: "DNS over HTTPS，支持路径到执行器的多入口映射",
      fields: {
        entries: {
          label: "路径映射",
          description: "定义 HTTP 路径到执行器的映射关系。",
          placeholder: '[{"path":"/dns-query","exec":"seq_main"}]',
        },
        "entries[]": {
          label: "路径映射",
        },
        "entries[].path": {
          label: "路径",
          description: "指定 DoH 请求路径。",
          placeholder: "/dns-query",
        },
        "entries[].exec": {
          label: "执行器",
          description: "指定处理该路径请求的执行器。",
          placeholder: "seq_main",
        },
        "entries[].json_api": {
          label: "JSON DNS API",
          description:
            "开启后，该路径的 GET 请求可使用 JSON DNS API 参数；RFC 8484 GET/POST 始终可用。",
        },
        listen: {
          label: "监听地址",
          description: "指定 HTTP/HTTPS 监听地址。",
          placeholder: ":443",
        },
        src_ip_header: {
          label: "来源 IP Header",
          description: "指定从请求头中读取真实客户端来源地址的字段名。",
          placeholder: "X-Forwarded-For",
        },
        cert: {
          label: "HTTPS 证书",
          description: "指定 HTTPS 证书文件路径。",
          placeholder: "/etc/oxidns/server.crt",
        },
        key: {
          label: "HTTPS 私钥",
          description: "指定 HTTPS 私钥文件路径。",
          placeholder: "/etc/oxidns/server.key",
        },
        idle_timeout: {
          label: "空闲超时(秒)",
          description: "指定 HTTP 连接空闲超时。",
        },
        enable_http3: {
          label: "启用 HTTP/3",
          description: "指定是否同时启用 HTTP/3。",
        },
      },
      metrics: {
        labels: {
          server_request_total: "请求总数",
          server_completed_total: "完成",
          server_controlled_total: "提前结束",
          server_failed_total: "失败(SERVFAIL)",
          server_inflight: "处理中",
          server_latency_count: "延迟样本",
          server_latency_sum_ms: "延迟累计(ms)",
        },
        help: {
          server_request_total: "服务器接收并处理的入站 DNS 请求总数。",
          server_completed_total: "执行器链正常执行完毕的请求总数。",
          server_controlled_total:
            "执行器主动停止（stop/return）而提前结束的请求总数。",
          server_failed_total: "因入口执行器失败而返回 SERVFAIL 的请求总数。",
          server_inflight: "当前服务器正在处理中的请求数量。",
          server_latency_count: "纳入服务器延迟统计的已完成请求数。",
          server_latency_sum_ms: "所有已完成请求的总处理延迟（毫秒）。",
        },
        derived: {
          "latency:server": "平均延迟",
          "percent:server_failed_total/server_request_total": "失败率",
        },
      },
    },
    quic_server: {
      name: "QUIC / DoQ Server",
      description: "DNS over QUIC 入口",
      fields: {
        entry: {
          label: "入口执行器",
          description: "指定 DoQ 请求进入策略链时使用的入口执行器。",
        },
        listen: {
          label: "监听地址",
          description: "指定 QUIC 监听地址。",
          placeholder: ":853",
        },
        cert: {
          label: "TLS 证书",
          description: "指定 DoQ 所需 TLS 证书文件。",
          placeholder: "/etc/oxidns/server.crt",
        },
        key: {
          label: "TLS 私钥",
          description: "指定 DoQ 所需 TLS 私钥文件。",
          placeholder: "/etc/oxidns/server.key",
        },
        idle_timeout: {
          label: "空闲超时(秒)",
          description: "指定 QUIC transport 的空闲超时。",
        },
      },
      metrics: {
        labels: {
          server_request_total: "请求总数",
          server_completed_total: "完成",
          server_controlled_total: "提前结束",
          server_failed_total: "失败(SERVFAIL)",
          server_inflight: "处理中",
          server_latency_count: "延迟样本",
          server_latency_sum_ms: "延迟累计(ms)",
        },
        help: {
          server_request_total: "服务器接收并处理的入站 DNS 请求总数。",
          server_completed_total: "执行器链正常执行完毕的请求总数。",
          server_controlled_total:
            "执行器主动停止（stop/return）而提前结束的请求总数。",
          server_failed_total: "因入口执行器失败而返回 SERVFAIL 的请求总数。",
          server_inflight: "当前服务器正在处理中的请求数量。",
          server_latency_count: "纳入服务器延迟统计的已完成请求数。",
          server_latency_sum_ms: "所有已完成请求的总处理延迟（毫秒）。",
        },
        derived: {
          "latency:server": "平均延迟",
          "percent:server_failed_total/server_request_total": "失败率",
        },
      },
    },
    sequence: {
      name: "Sequence",
      description: "按顺序编排 matcher 与 executor，是最常用入口执行器",
      fields: {
        args: {
          label: "规则链",
          description: "定义 sequence 的规则链。",
          placeholder: "$cache_main\nmatches: !$has_resp, exec: $forward_main",
        },
        "args[]": {
          label: "规则",
        },
        "args[].matches": {
          label: "匹配条件",
          description: "定义当前规则的匹配条件。",
          placeholder: "$has_resp\nqname domain:example.com\n!$blocked",
        },
        "args[].matches.$matcher_ref": {
          label: "引用 matcher",
          placeholder: "has_resp",
        },
        "args[].matches.$input": {
          label: "输入值",
          placeholder: "qname domain:example.com",
        },
        "args[].exec": {
          label: "执行动作",
          description:
            "定义规则命中后要执行的动作，可引用执行器或使用 accept、return、reject、jump、goto、mark 等内置动作；reject 支持大小写不敏感的 RCODE 名称和数字。",
          placeholder:
            "$forward_main / accept / reject SERVFAIL / reject NOERROR / reject 3 / jump seq_tag",
        },
      },
    },
    forward: {
      name: "Forward",
      description: "向一个或多个上游 DNS 发起查询",
      fields: {
        concurrent: {
          label: "并发上游数",
          description: "定义多上游模式下的并发查询扇出数。",
        },
        response_selection: {
          label: "结果选择",
          description: "定义多上游并发返回不一致时的结果选择策略。",
          options: {
            fastest: "最快响应",
            balanced: "平衡",
            prefer_positive: "优先正向答案",
            consensus: "负向共识",
          },
        },
        upstreams: {
          label: "上游列表",
          description: "定义一个或多个上游目标。",
          placeholder: "udp://1.1.1.1:53",
        },
        "upstreams[].tag": {
          label: "上游标识",
          description: "为单个上游提供日志标识，便于排查多上游竞争结果。",
          placeholder: "cf_udp",
        },
        "upstreams[].addr": {
          label: "上游地址",
          description: "定义上游地址、协议类型以及目标主机。",
          placeholder: "udp://1.1.1.1:53",
        },
        "upstreams[].outbound": {
          label: "出站配置",
          description:
            "引用 network.outbound.profiles 中的出站配置，为该上游注入 resolver 和 proxy；本地 dial_addr、bootstrap、socks5 优先生效。",
          placeholder: "profile-1",
        },
        "upstreams[].dial_addr": {
          label: "拨号 IP",
          description:
            "指定实际连接 IP，同时保留 addr 中的主机名用于 SNI、Host 和证书校验；与 bootstrap 同时配置时本字段优先生效。",
          placeholder: "203.0.113.53",
        },
        "upstreams[].port": {
          label: "端口覆盖",
          description: "覆盖协议默认端口。",
          placeholder: "443",
        },
        "upstreams[].bootstrap": {
          label: "Bootstrap",
          description:
            "为域名型上游提供引导解析服务器，必须写为 IP:port；未配置时会在首次建连时使用系统解析；与 dial_addr 同时配置时会被忽略。",
          placeholder: "8.8.8.8:53",
        },
        "upstreams[].bootstrap_version": {
          label: "Bootstrap IP 版本",
          description: "指定 bootstrap 优先使用 IPv4 或 IPv6。",
          options: {
            "4": "IPv4",
            "6": "IPv6",
          },
        },
        "upstreams[].socks5": {
          label: "SOCKS5 代理",
          description: "为上游连接指定 SOCKS5 代理。",
          placeholder: "user:pass@127.0.0.1:1080",
        },
        "upstreams[].idle_timeout": {
          label: "连接空闲超时(秒)",
          description: "定义连接池空闲连接保留时间。",
          placeholder: "30",
        },
        "upstreams[].max_conns": {
          label: "最大连接数",
          description: "定义连接池连接上限，范围 1..4096。",
          placeholder: "256",
        },
        "upstreams[].min_conns": {
          label: "最小连接数",
          description:
            "定义连接池最小预热连接数，默认 0，范围 0..4096，且不能大于 max_conns。",
          placeholder: "0",
        },
        "upstreams[].insecure_skip_verify": {
          label: "跳过 TLS 校验",
          description: "控制是否跳过 TLS 证书校验。",
        },
        "upstreams[].timeout": {
          label: "查询超时",
          description: "定义单次上游查询超时。",
          placeholder: "3s",
        },
        "upstreams[].enable_pipeline": {
          label: "启用 Pipeline",
          description: "控制 TCP 或 DoT 流水线。",
        },
        "upstreams[].enable_http3": {
          label: "启用 HTTP/3",
          description: "控制 DoH 是否使用 HTTP/3。",
        },
        "upstreams[].so_mark": {
          label: "SO_MARK",
          description: "设置 Linux SO_MARK。",
          placeholder: "100",
        },
        "upstreams[].bind_to_device": {
          label: "绑定网卡",
          description: "设置 Linux SO_BINDTODEVICE。",
          placeholder: "eth0",
        },
        short_circuit: {
          label: "成功后停止后续执行",
          description:
            "控制在拿到成功上游响应后，是否立即停止后续 executor 链。",
        },
      },
      quickSetup: {
        paramPlaceholder: "1.1.1.1 short_circuit=true",
      },
      metrics: {
        labels: {
          forward_query_total: "转发查询",
          forward_success_total: "成功",
          forward_error_total: "失败",
          forward_timeout_total: "超时",
          forward_latency_count: "延迟样本",
          forward_latency_sum_ms: "延迟累计(ms)",
          forward_upstream_query_total: "上游查询",
          forward_upstream_success_total: "上游成功",
          forward_upstream_error_total: "上游失败",
          forward_upstream_timeout_total: "上游超时",
          forward_upstream_latency_count: "上游延迟样本",
          forward_upstream_latency_sum_ms: "上游延迟累计(ms)",
        },
        help: {
          forward_query_total: "转发执行器发起的查询总数。",
          forward_success_total: "成功获得上游响应的查询总数。",
          forward_error_total: "上游返回错误或无法获得响应的查询总数。",
          forward_timeout_total: "因超时未得到上游响应的查询总数。",
          forward_latency_count: "纳入延迟统计的已完成查询数。",
          forward_latency_sum_ms: "所有已完成转发查询的总延迟（毫秒）。",
          forward_upstream_query_total: "向该上游发起的请求总数。",
          forward_upstream_success_total: "该上游返回成功响应的次数。",
          forward_upstream_error_total: "该上游请求失败的次数。",
          forward_upstream_timeout_total: "该上游请求超时的次数。",
          forward_upstream_latency_count: "纳入该上游延迟统计的请求数。",
          forward_upstream_latency_sum_ms: "该上游所有请求的总延迟（毫秒）。",
        },
        derived: {
          "percent:forward_success_total/forward_query_total": "成功率",
          "latency:forward": "平均延迟",
        },
      },
    },
    cache: {
      name: "Cache",
      description: "TTL 感知缓存，支持负缓存、lazy cache 与持久化",
      fields: {
        size: {
          label: "最大条目数",
          description: "定义缓存最大条目数。",
        },
        lazy_cache_ttl: {
          label: "Lazy Cache TTL(秒)",
          description: "为正向成功响应启用 lazy cache。",
        },
        dump_file: {
          label: "持久化文件",
          description: "指定缓存持久化文件路径。",
          placeholder: "./dns_cache.dump",
        },
        dump_interval: {
          label: "落盘周期(秒)",
          description: "定义缓存定期落盘周期。",
        },
        short_circuit: {
          label: "命中后停止后续执行",
          description: "控制缓存命中后是否立即结束后续执行。",
        },
        cache_negative: {
          label: "缓存负响应",
          description: "控制是否缓存 NXDOMAIN 与 NODATA。",
        },
        max_negative_ttl: {
          label: "负缓存 TTL 上限",
          description: "定义负缓存 TTL 上限。",
        },
        negative_ttl_without_soa: {
          label: "无 SOA 负缓存 TTL",
          description: "定义无 SOA 负响应的回退 TTL。",
        },
        max_positive_ttl: {
          label: "正响应 TTL 上限",
          description: "定义正响应 TTL 上限。",
        },
        ecs_in_key: {
          label: "ECS 参与缓存键",
          description: "控制 ECS scope 是否参与缓存键计算。",
        },
      },
      quickSetup: {
        paramPlaceholder: "short_circuit=true",
      },
      metrics: {
        labels: {
          cache_lookup_total: "缓存查询",
          cache_hit_total: "命中",
          cache_miss_total: "未命中",
          cache_expired_total: "过期",
          cache_insert_total: "写入",
          cache_skip_total: "跳过",
          cache_lazy_refresh_total: "懒刷新",
          cache_entry_count: "条目数",
        },
        help: {
          cache_lookup_total: "带有可缓存请求键的缓存查询总数。",
          cache_hit_total:
            "按新鲜度分类的缓存命中总数（fresh = 直接命中，stale = 过期可用）。",
          cache_miss_total: "缓存未命中的查询总数。",
          cache_expired_total: "查找时发现并移除过期条目的次数。",
          cache_insert_total: "缓存条目插入或更新的总次数。",
          cache_skip_total:
            "因写入策略（截断响应、无 TTL）而跳过缓存的响应总数。",
          cache_lazy_refresh_total:
            "Lazy Cache 后台刷新尝试总数（按结果：started / success / failed）。",
          cache_entry_count: "当前缓存中的条目数量。",
        },
        derived: {
          "percent:cache_hit_total/cache_lookup_total": "命中率",
        },
      },
    },
    fallback: {
      name: "Fallback",
      description: "主路径失败或过慢时切换到备用执行器",
      fields: {
        primary: {
          label: "主执行器",
          description: "指定主执行器。",
        },
        secondary: {
          label: "备用执行器",
          description: "指定备用执行器。",
        },
        threshold: {
          label: "接管阈值(ms)",
          description: "定义主路径超时或延迟判定阈值。",
        },
        always_standby: {
          label: "备用路径并行待命",
          description: "控制备用路径是否与主路径同时待命。",
        },
        short_circuit: {
          label: "成功后停止后续执行",
          description:
            "控制在主/备路径选出最终响应后，是否立即停止后续 executor 链。",
        },
      },
      metrics: {
        labels: {
          fallback_primary_total: "主链",
          fallback_primary_error_total: "主链失败",
          fallback_secondary_total: "降级",
        },
        help: {
          fallback_primary_total: "主执行器被调用的总次数。",
          fallback_primary_error_total: "主执行器未能产生响应的总次数。",
          fallback_secondary_total: "备用执行器被调用的总次数。",
        },
        derived: {
          "percent:fallback_secondary_total/fallback_primary_total": "降级率",
        },
      },
    },
    hosts: {
      name: "Hosts",
      description: "按域名规则直接返回静态 A / AAAA",
      fields: {
        entries: {
          label: "内联 hosts 规则",
          description: "定义内联 hosts 规则。",
          placeholder:
            "router.local 192.168.1.1\nfull:gateway.local 192.168.1.2\ndomain:svc.local 10.0.0.10\nkeyword:nas 192.168.1.20\nregexp:^api[0-9]+\\.corp\\.local$ 10.10.0.5",
        },
        "entries[]": {
          label: "输入值",
          placeholder: "router.local 192.168.1.1",
        },
        files: {
          label: "hosts 文件",
          description: "指定外部 hosts 规则文件列表。",
          placeholder: "/etc/oxidns/hosts.txt",
        },
        "files[]": {
          label: "输入值",
          placeholder: "/etc/oxidns/hosts.txt",
        },
        short_circuit: {
          label: "命中后停止后续执行",
          description: "命中并生成本地应答后，是否立即停止后续 executor 链。",
        },
      },
      metrics: {
        labels: {
          hosts_hit_total: "命中",
          hosts_miss_total: "未命中",
        },
        help: {
          hosts_hit_total: "命中 hosts 规则并生成本地响应的总次数。",
          hosts_miss_total: "未命中任何 hosts 规则的查询总次数。",
        },
        derived: {
          "percent_of_sum:hosts_hit_total/hosts_hit_total+hosts_miss_total":
            "命中率",
        },
      },
    },
    arbitrary: {
      name: "Arbitrary",
      description: "加载静态 DNS 记录并在命中时构造应答",
      fields: {
        rules: {
          label: "静态记录",
          description: "定义内联静态记录列表。",
          placeholder:
            'example.com. 60 IN TXT "hello world"\nwww.example.com. 120 IN A 192.0.2.10',
        },
        "rules[]": {
          label: "输入值",
          placeholder: 'example.com. 60 IN TXT "hello world"',
        },
        files: {
          label: "记录文件",
          description: "指定静态记录文件列表。",
          placeholder: "/etc/oxidns/zone.txt",
        },
        "files[]": {
          label: "输入值",
          placeholder: "/etc/oxidns/zone.txt",
        },
        short_circuit: {
          label: "命中后停止后续执行",
          description: "命中并生成本地响应后，是否立即停止后续 executor 链。",
        },
      },
    },
    redirect: {
      name: "Redirect",
      description: "改写请求域名，配合 forward 生成目标响应",
      fields: {
        rules: {
          label: "重定向规则",
          description: "定义内联重定向规则。",
          placeholder:
            "full:old.example.com new.example.net\ndomain:legacy.example.com modern.example.net\nkeyword:staging staging-gateway.example.net\nregexp:^api[0-9]+\\.legacy\\.example\\.com$ api-gateway.example.net",
        },
        "rules[]": {
          label: "输入值",
          placeholder: "full:old.example.com new.example.net",
        },
        files: {
          label: "规则文件",
          description: "指定外部重定向规则文件列表。",
          placeholder: "/etc/oxidns/redirect.txt",
        },
        "files[]": {
          label: "输入值",
          placeholder: "/etc/oxidns/redirect.txt",
        },
      },
    },
    ecs_handler: {
      name: "ECS Handler",
      description: "处理 EDNS Client Subnet 的保留、注入和回程清理",
      fields: {
        forward: {
          label: "保留客户端 ECS",
          description: "控制是否保留客户端请求中已有的 ECS。",
        },
        send: {
          label: "缺失时发送 ECS",
          description: "控制在请求缺少 ECS 时，是否根据来源地址自动补充 ECS。",
        },
        preset: {
          label: "预设 ECS 地址",
          description: "指定固定的 ECS 来源地址。",
          placeholder: "203.0.113.10",
        },
        mask4: {
          label: "IPv4 前缀长度",
          description: "指定 IPv4 ECS 前缀长度。",
        },
        mask6: {
          label: "IPv6 前缀长度",
          description: "指定 IPv6 ECS 前缀长度。",
        },
      },
      quickSetup: {
        paramPlaceholder: "203.0.113.10/24",
      },
    },
    forward_edns0opt: {
      name: "Forward EDNS0 Opt",
      description: "把指定 EDNS0 option 从请求转发到响应",
      fields: {
        codes: {
          label: "Option Code",
          description: "定义允许从请求复制到响应中的 EDNS0 option code 集合。",
          placeholder: "10\n12",
        },
        "codes[]": {
          label: "输入值",
          placeholder: "10",
        },
      },
      quickSetup: {
        paramPlaceholder: "10,12",
      },
    },
    ttl: {
      name: "TTL",
      description: "固定、抬高或限制响应 TTL",
      fields: {
        fix: {
          label: "固定 TTL",
          description: "将所有响应 TTL 固定为同一个值。",
        },
        min: {
          label: "TTL 下限",
          description: "定义 TTL 下限。",
        },
        max: {
          label: "TTL 上限",
          description: "定义 TTL 上限。",
        },
      },
      quickSetup: {
        paramPlaceholder: "300 / 60-600",
      },
    },
    ip_selector: {
      name: "IP Selector",
      description: "对响应中的 A / AAAA 地址进行测速排序与筛选",
      fields: {
        selection_mode: {
          label: "优选模式",
          description: "定义响应 IP 优选策略。",
          options: {
            first_success: "First success",
            best_within_budget: "Best within budget",
            background: "Background",
          },
        },
        probe_methods: {
          label: "测速方式",
          description:
            "定义用于评分响应 IP 的探测方式，支持 tcp:<port>、ping、none。",
          placeholder: "tcp:443\ntcp:80",
        },
        "probe_methods[]": {
          label: "输入值",
          placeholder: "tcp:443",
        },
        outbound: {
          label: "出站配置",
          description:
            "引用 network.outbound.profiles 中的出站配置，为 TCP 探测复用 profile proxy。",
          placeholder: "profile-1",
        },
        socks5: {
          label: "SOCKS5 代理",
          description:
            "为 TCP 探测指定局部 SOCKS5 代理，优先于 outbound profile proxy。",
          placeholder: "127.0.0.1:1080",
        },
        probe_stagger: {
          label: "测速错峰(ms)",
          description: "多种测速方式之间的错峰启动间隔。",
        },
        probe_timeout: {
          label: "单次超时(ms)",
          description: "单次 IP 探测超时时间。",
        },
        max_wait: {
          label: "最大等待(ms)",
          description: "本次响应优选最多等待多久。",
        },
        top_n: {
          label: "保留地址数",
          description: "保留排序后的前 N 个地址；0 表示只重排不删除。",
        },
        dnssec_policy: {
          label: "DNSSEC 策略",
          description: "DNSSEC 敏感响应的处理策略。",
          options: {
            reorder_only: "Reorder only",
            skip: "Skip",
          },
        },
        max_parallel_probes: {
          label: "最大并发探测",
          description: "插件级并发探测数量上限。",
        },
        cache: {
          label: "评分缓存",
          description: "配置 IP 探测评分缓存。",
        },
        "cache.enabled": {
          label: "启用缓存",
          description: "是否启用探测评分缓存。",
        },
        "cache.size": {
          label: "缓存容量",
          description: "缓存容量目标。",
        },
        "cache.ttl": {
          label: "成功 TTL(秒)",
          description: "成功评分保留时间。",
        },
        "cache.failure_ttl": {
          label: "失败 TTL(秒)",
          description: "失败评分保留时间。",
        },
      },
      quickSetup: {
        paramPlaceholder: "best_within_budget tcp:443,tcp:80,ping",
      },
      metrics: {
        labels: {
          ip_selector_probe_total: "探测",
          ip_selector_probe_latency_count: "延迟样本",
          ip_selector_probe_latency_sum_ms: "延迟累计(ms)",
          ip_selector_selected_total: "优选结果",
          ip_selector_cache_entries: "缓存条目",
          ip_selector_dropped_probe_total: "跳过探测",
        },
        help: {
          ip_selector_probe_total: "按测速方式和结果统计的 IP 探测次数。",
          ip_selector_probe_latency_count: "成功测速的延迟样本数。",
          ip_selector_probe_latency_sum_ms: "成功测速延迟累计值（毫秒）。",
          ip_selector_selected_total:
            "按 probe/cache/fallback 来源统计的优选次数。",
          ip_selector_cache_entries: "当前 IP 探测评分缓存条目数量。",
          ip_selector_dropped_probe_total:
            "由于并发限制或已有 in-flight 探测而未新启动的探测次数。",
        },
      },
    },
    prefer_ipv4: {
      name: "Prefer IPv4",
      description: "双栈优选器，偏好 A 记录并抑制可替代的 AAAA 请求",
      fields: {
        cache: {
          label: "缓存偏好状态",
          description: "控制是否缓存 preferred 类型存在状态。",
        },
        cache_ttl: {
          label: "偏好状态缓存 TTL(秒)",
          description: "定义 preferred 状态缓存时长。",
        },
      },
    },
    prefer_ipv6: {
      name: "Prefer IPv6",
      description: "双栈优选器，偏好 AAAA 记录并抑制可替代的 A 请求",
      fields: {
        cache: {
          label: "缓存偏好状态",
          description: "控制是否缓存 preferred 类型存在状态。",
        },
        cache_ttl: {
          label: "偏好状态缓存 TTL(秒)",
          description: "定义 preferred 状态缓存时长。",
        },
      },
    },
    black_hole: {
      name: "Black Hole",
      description: "按模式生成全 qtype 本地拦截响应",
      fields: {
        mode: {
          label: "拦截模式",
          description:
            "定义拦截响应类型；未配置 ips 时默认 nxdomain，配置 ips 时默认 custom。",
          options: {
            nxdomain: "NXDOMAIN",
            nodata: "NODATA",
            null: "Null 地址",
            custom: "自定义地址",
            refused: "REFUSED",
          },
        },
        ips: {
          label: "自定义返回地址",
          description: "定义 custom 模式使用的本地合成返回地址集合。",
          placeholder: "0.0.0.0\n::",
        },
        "ips[]": {
          label: "输入值",
          placeholder: "0.0.0.0",
        },
        short_circuit: {
          label: "命中后停止后续执行",
          description: "生成拦截响应后，是否立即停止后续 executor 链。",
        },
      },
      quickSetup: {
        paramPlaceholder: "nxdomain short_circuit=true",
      },
      metrics: {
        labels: {
          blackhole_block_total: "拦截",
        },
        help: {
          blackhole_block_total: "black_hole 生成拦截响应的总次数。",
        },
      },
    },
    drop_resp: {
      name: "Drop Response",
      description: "清空当前上下文中的响应",
      fields: {},
    },
    reverse_lookup: {
      name: "Reverse Lookup",
      description: "缓存 IP 到域名关系，并可选直接处理 PTR 查询",
      fields: {
        size: {
          label: "反查缓存容量",
          description: "定义反查缓存容量上限。",
        },
        handle_ptr: {
          label: "直接响应 PTR",
          description: "控制是否直接用反查缓存响应 PTR 请求。",
        },
        ttl: {
          label: "映射 TTL(秒)",
          description: "定义 IP 到域名映射的缓存 TTL。",
        },
      },
      metrics: {
        labels: {
          reverse_lookup_ptr_hit_total: "PTR 命中",
          reverse_lookup_ptr_miss_total: "PTR 未命中",
          reverse_lookup_cache_insert_total: "缓存写入",
          reverse_lookup_cache_entries: "缓存条目",
        },
        help: {
          reverse_lookup_ptr_hit_total: "从反查缓存成功响应 PTR 查询的总次数。",
          reverse_lookup_ptr_miss_total: "PTR 查询未命中反查缓存的总次数。",
          reverse_lookup_cache_insert_total: "写入 IP → 域名映射条目的总次数。",
          reverse_lookup_cache_entries: "当前反查缓存中的条目数量。",
        },
      },
    },
    query_summary: {
      name: "Query Summary",
      description: "在后续链路执行完后输出紧凑查询摘要",
      fields: {
        msg: {
          label: "日志标题",
          description: "定义摘要日志标题。",
          placeholder: "main pipeline",
        },
      },
      quickSetup: {
        paramPlaceholder: "main pipeline",
      },
    },
    learn_domain: {
      name: "Learn Domain",
      description: "把请求域名写入 dynamic_domain_set，用于动态规则学习",
      fields: {
        provider: {
          label: "目标 Provider",
          description: "引用目标 dynamic_domain_set provider。",
          placeholder: "learned_allow",
        },
        phase: {
          label: "学习阶段",
          description: "控制在下游 executor 之前学习，还是响应返回后学习。",
          options: {
            before: "Before",
            after: "After",
          },
        },
        questions: {
          label: "Questions",
          description: "控制学习第一个 question 或所有 question。",
          options: {
            first: "First",
            all: "All",
          },
        },
        qtypes: {
          label: "查询类型",
          description: "只学习指定 DNS 查询类型。",
          placeholder: "A\nAAAA",
        },
        "qtypes[]": {
          label: "输入值",
          placeholder: "A",
        },
        success_only: {
          label: "仅成功响应",
          description: "仅响应为 NOERROR 时学习；只在 after 阶段生效。",
        },
        answer_required: {
          label: "需要 Answer",
          description: "仅响应包含 answer 时学习；只在 after 阶段生效。",
        },
        rule_kind: {
          label: "规则类型",
          description: "控制写入 dynamic_domain_set 的规则类型。",
          options: {
            full: "Full",
            domain: "Domain",
          },
        },
        async: {
          label: "异步写入",
          description: "控制是否只入队后继续执行；关闭后会等待写入完成。",
        },
        error_mode: {
          label: "错误处理",
          description: "控制学习失败后的执行行为。",
          options: {
            continue: "Continue",
            stop: "Stop",
            fail: "Fail",
          },
        },
        timeout: {
          label: "同步超时",
          description: "async 关闭时等待 provider 写入完成的最长时间。",
        },
      },
    },
    query_recorder: {
      name: "Query Recorder",
      description: "将请求、响应和 sequence 路径事件持久化到 SQLite",
      fields: {
        path: {
          label: "SQLite 文件",
          description: "指定当前 recorder 的 SQLite 文件路径。",
          placeholder: "./data/query-recorder-main.sqlite",
        },
        queue_size: {
          label: "队列大小",
          description: "定义热路径到后台写线程的有界队列大小。",
        },
        batch_size: {
          label: "批量写入条数",
          description: "定义后台批量写入 SQLite 的单批记录数。",
        },
        flush_interval_ms: {
          label: "Flush 间隔(ms)",
          description: "定义后台写线程的批量 flush 间隔。",
        },
        memory_tail: {
          label: "内存 Tail 长度",
          description:
            "定义最近记录的内存 tail 长度，用于 stream?tail=n 回放。",
        },
        retention_days: {
          label: "保留天数",
          description: "定义日志保留天数；过期数据会被定时实际删除。",
        },
        cleanup_interval_hours: {
          label: "清理周期(小时)",
          description: "定义过期清理任务的执行周期。",
        },
        reader_concurrency: {
          label: "读取并发数",
          description:
            "限制 query_recorder API/统计读取侧同时运行的 SQLite reader 数量，避免 WebUI 或 API 突发请求占用过多阻塞线程和内存。",
        },
      },
    },
    metrics_collector: {
      name: "Metrics Collector",
      description: "收集轻量级请求计数与延时指标并导出 Prometheus 格式",
      fields: {
        name: {
          label: "指标名称",
          description: "定义当前指标收集器的名称标签。",
        },
      },
      quickSetup: {
        paramPlaceholder: "main",
      },
      metrics: {
        labels: {
          query_total: "总查询",
          query_error_total: "查询错误",
          query_inflight: "处理中",
          query_latency_count: "延迟样本",
          query_latency_sum_ms: "延迟累计(ms)",
        },
        help: {
          query_total: "metrics_collector 观测到的 DNS 查询总数。",
          query_error_total: "未产生响应的查询总数（错误或无响应）。",
          query_inflight: "当前正在处理中的 DNS 查询数量。",
          query_latency_count: "纳入延迟统计的已完成查询数。",
          query_latency_sum_ms: "所有已完成查询的总延迟（毫秒）。",
        },
        derived: {
          "latency:query": "平均延迟",
          "percent:query_error_total/query_total": "错误率",
        },
      },
    },
    debug_print: {
      name: "Debug Print",
      description: "打印请求与响应对象，便于排查问题",
      fields: {
        msg: {
          label: "日志标题",
          description: "定义日志输出标题。",
        },
      },
      quickSetup: {
        paramPlaceholder: "cron refresh",
      },
    },
    sleep: {
      name: "Sleep",
      description: "为策略链加入可控异步延迟",
      fields: {
        duration: {
          label: "延迟(ms)",
          description: "定义当前请求在该执行器上的额外异步等待时间。",
        },
      },
      quickSetup: {
        paramPlaceholder: "250ms / 2s",
      },
    },
    http_request: {
      name: "HTTP Request",
      description: "向外部 HTTP/HTTPS 服务发送 webhook、审计或联动请求",
      fields: {
        method: {
          label: "HTTP 方法",
          description: "指定 HTTP 方法，例如 GET、POST、PUT、PATCH、DELETE。",
          options: {
            GET: "GET",
            POST: "POST",
            PUT: "PUT",
            PATCH: "PATCH",
            DELETE: "DELETE",
          },
        },
        url: {
          label: "目标 URL",
          description: "目标 URL。",
          placeholder: "https://hooks.example.com/dns",
        },
        phase: {
          label: "触发阶段",
          description:
            "控制请求在下游 executor 之前发送，还是在下游执行完成后发送。",
          options: {
            before: "Before",
            after: "After",
          },
        },
        async: {
          label: "异步发送",
          description:
            "控制使用异步后台队列发送，还是在当前请求路径同步等待 HTTP 完成。",
        },
        timeout: {
          label: "超时",
          description: "限制单次 HTTP 调用的总超时时间。",
        },
        error_mode: {
          label: "错误处理",
          description: "控制 HTTP 调用失败后的处理方式。",
          options: {
            continue: "Continue",
            stop: "Stop",
            fail: "Fail",
          },
        },
        headers: {
          label: "请求头",
          description: "附加 HTTP 请求头。",
          keyPlaceholder: "X-Qname",
          valuePlaceholder: "${qname}",
        },
        query_params: {
          label: "Query 参数",
          description: "把额外参数追加到 URL query 上。",
          keyPlaceholder: "qname",
          valuePlaceholder: "${qname}",
        },
        body: {
          label: "原始 Body",
          description: "原始字符串请求体。",
          placeholder: "qname=${qname}",
        },
        json: {
          label: "JSON Body",
          description: "以 JSON 方式发送请求体。",
          placeholder: '{"qname":"${qname}","client_ip":"${client_ip}"}',
        },
        form: {
          label: "表单 Body",
          description: "以 application/x-www-form-urlencoded 方式发送表单。",
          keyPlaceholder: "qname",
          valuePlaceholder: "${qname}",
        },
        content_type: {
          label: "Content-Type",
          description: "为原始 args.body 指定 Content-Type。",
        },
        outbound: {
          label: "出站配置",
          description:
            "引用 network.outbound.profiles 中的出站配置，用于统一控制解析器和代理。",
          placeholder: "profile-1",
        },
        socks5: {
          label: "SOCKS5 代理",
          description: "指定 SOCKS5 代理。",
          placeholder: "127.0.0.1:1080",
        },
        insecure_skip_verify: {
          label: "跳过 HTTPS 证书校验",
          description: "是否跳过 HTTPS 证书校验。",
        },
        max_redirects: {
          label: "最大重定向次数",
          description: "限制最多跟随多少次重定向。",
        },
        queue_size: {
          label: "异步队列大小",
          description: "异步模式下后台发送队列的容量。",
        },
      },
      metrics: {
        labels: {
          http_request_dispatch_total: "请求发起",
          http_request_error_total: "请求失败",
          http_request_dropped_total: "队列丢弃",
        },
        help: {
          http_request_dispatch_total:
            "http_request 执行器发起 HTTP 请求的总次数。",
          http_request_error_total:
            "HTTP 请求失败的总次数（渲染失败、发送失败或异步投递失败）。",
          http_request_dropped_total:
            "因异步队列已满或已关闭而丢弃的 HTTP 请求总数。",
        },
      },
    },
    script: {
      name: "Script",
      description: "执行外部命令，并注入 DnsContext 中的稳定字段",
      fields: {
        command: {
          label: "命令",
          description: "要执行的命令路径或命令名。",
          placeholder: "bash",
        },
        args: {
          label: "命令参数",
          description: "传给命令的参数数组。",
          placeholder: "/etc/oxidns/notify.sh\n${qname}",
        },
        "args[]": {
          label: "输入值",
          placeholder: "/etc/oxidns/notify.sh",
        },
        env: {
          label: "环境变量",
          description: "追加到子进程环境变量中的键值对。",
          keyPlaceholder: "FDNS_QNAME",
          valuePlaceholder: "${qname}",
        },
        cwd: {
          label: "工作目录",
          description: "指定脚本运行时的工作目录。",
          placeholder: "/etc/oxidns",
        },
        timeout: {
          label: "超时",
          description: "限制单次脚本执行时长。",
        },
        error_mode: {
          label: "错误处理",
          description: "控制脚本失败或超时后的处理方式。",
          options: {
            continue: "Continue",
            stop: "Stop",
            fail: "Fail",
          },
        },
        max_output_bytes: {
          label: "最大输出捕获字节",
          description:
            "限制 stdout / stderr 的捕获长度，超过部分只做截断标记。",
        },
      },
      metrics: {
        labels: {
          script_run_total: "执行",
          script_success_total: "成功",
          script_error_total: "失败",
          script_timeout_total: "超时",
        },
        help: {
          script_run_total: "脚本执行器启动的外部命令总次数。",
          script_success_total: "外部命令正常退出（exit code 0）的总次数。",
          script_error_total:
            "外部命令执行失败（非零退出或运行时错误）的总次数。",
          script_timeout_total: "外部命令因超时被终止的总次数。",
        },
      },
    },
    ipset: {
      name: "IPSet",
      description: "把响应中的 IP 写入 Linux ipset",
      fields: {
        set_name4: {
          label: "IPv4 ipset 名称",
          description: "指定写入 IPv4 地址的 ipset 名称。",
        },
        set_name6: {
          label: "IPv6 ipset 名称",
          description: "指定写入 IPv6 地址的 ipset 名称。",
        },
        mask4: {
          label: "IPv4 前缀长度",
          description: "指定 IPv4 地址写入 ipset 时使用的前缀长度。",
        },
        mask6: {
          label: "IPv6 前缀长度",
          description: "指定 IPv6 地址写入 ipset 时使用的前缀长度。",
        },
      },
      quickSetup: {
        paramPlaceholder: "oxidns_v4,inet,24 oxidns_v6,inet6,64",
      },
      metrics: {
        labels: {
          ipset_entries_total: "入队条目",
          ipset_dropped_total: "丢弃批次",
          ipset_write_total: "写入条目",
          ipset_write_error_total: "写入失败",
        },
        help: {
          ipset_entries_total: "排队等待写入 ipset 的 IP 条目总数。",
          ipset_dropped_total: "因写入队列已满而丢弃的批次总数。",
          ipset_write_total: "通过 netlink 成功写入 ipset 的 IP 条目总数。",
          ipset_write_error_total: "ipset netlink 写入失败的总次数。",
        },
      },
    },
    nftset: {
      name: "NFTSet",
      description: "把响应 IP 写入 Linux nftables set",
      fields: {
        ipv4: {
          label: "IPv4 目标",
          description: "定义 IPv4 目标 nftables set。",
        },
        "ipv4.table_family": {
          label: "表 Family",
          placeholder: "ip",
        },
        "ipv4.table_name": {
          label: "表名",
          placeholder: "mangle",
        },
        "ipv4.set_name": {
          label: "Set 名称",
          placeholder: "dns_v4",
        },
        "ipv4.mask": {
          label: "前缀长度",
          placeholder: "24",
        },
        ipv6: {
          label: "IPv6 目标",
          description: "定义 IPv6 目标 nftables set。",
        },
        "ipv6.table_family": {
          label: "表 Family",
          placeholder: "ip6",
        },
        "ipv6.table_name": {
          label: "表名",
          placeholder: "mangle",
        },
        "ipv6.set_name": {
          label: "Set 名称",
          placeholder: "dns_v6",
        },
        "ipv6.mask": {
          label: "前缀长度",
          placeholder: "24",
        },
        table_family4: {
          label: "IPv4 表 family",
          description: "兼容写法下定义 IPv4 的 nftables 表 family。",
        },
        table_name4: {
          label: "IPv4 表名",
          description: "兼容写法下定义 IPv4 的 nftables 表名。",
        },
        set_name4: {
          label: "IPv4 set 名称",
          description: "兼容写法下定义 IPv4 的 set 名称。",
        },
        mask4: {
          label: "IPv4 前缀长度",
          description: "兼容写法下定义 IPv4 前缀长度。",
        },
        table_family6: {
          label: "IPv6 表 family",
          description: "兼容写法下定义 IPv6 的 nftables 表 family。",
        },
        table_name6: {
          label: "IPv6 表名",
          description: "兼容写法下定义 IPv6 的 nftables 表名。",
        },
        set_name6: {
          label: "IPv6 set 名称",
          description: "兼容写法下定义 IPv6 的 set 名称。",
        },
        mask6: {
          label: "IPv6 前缀长度",
          description: "兼容写法下定义 IPv6 前缀长度。",
        },
      },
      quickSetup: {
        paramPlaceholder: "ip,mangle,dns_v4,ipv4_addr,24",
      },
      metrics: {
        labels: {
          nftset_entries_total: "入队前缀",
          nftset_dropped_total: "丢弃批次",
          nftset_write_total: "写入前缀",
          nftset_write_error_total: "写入失败",
        },
        help: {
          nftset_entries_total: "排队等待写入 nftset 的 IP 前缀总数。",
          nftset_dropped_total: "因写入队列已满而丢弃的批次总数。",
          nftset_write_total: "通过 netlink 成功写入 nftables 的 IP 前缀总数。",
          nftset_write_error_total: "nftset netlink 写入失败的总次数。",
        },
      },
    },
    ros_address_list: {
      name: "RouterOS Address List",
      description: "把应答 IP 同步到 RouterOS address-list",
      fields: {
        address: {
          label: "RouterOS API 地址",
          description: "指定 RouterOS API 服务地址，通常写为 host:port。",
          placeholder: "172.16.1.1:8728",
        },
        username: {
          label: "用户名",
          description: "指定 RouterOS API 登录用户名。",
        },
        password: {
          label: "密码",
          description: "指定 RouterOS API 登录密码。",
        },
        connect_timeout: {
          label: "连接超时",
          description: "建立 RouterOS API 连接时的等待上限，单位秒。",
        },
        send_timeout: {
          label: "发送超时",
          description: "发送单个 RouterOS API 命令时的等待上限，单位秒。",
        },
        receive_timeout: {
          label: "接收超时",
          description: "等待下一段 RouterOS API 响应数据的上限，单位秒。",
        },
        async: {
          label: "异步提交",
          description: "控制地址写入行为是否采用异步方式。",
        },
        address_list4: {
          label: "IPv4 Address List",
          description: "指定 IPv4 地址写入的目标 address-list 名称。",
        },
        address_list6: {
          label: "IPv6 Address List",
          description: "指定 IPv6 地址写入的目标 address-list 名称。",
        },
        comment_prefix: {
          label: "注释前缀",
          description: "指定插件写入 RouterOS 条目时使用的注释前缀。",
        },
        persistent: {
          label: "常驻地址",
          description: "定义需要长期保留的静态地址集合。",
        },
        "persistent.ips": {
          label: "IP / CIDR",
          description: "以内联方式声明常驻 IP 或 CIDR 网段。",
          placeholder: "1.1.1.1\n100.64.1.0/24",
        },
        "persistent.ips[]": {
          label: "输入值",
          placeholder: "1.1.1.1",
        },
        "persistent.files": {
          label: "文件",
          description: "从外部文件加载常驻地址集合。",
          placeholder: "/etc/oxidns/persistent_ips.txt",
        },
        "persistent.files[]": {
          label: "输入值",
          placeholder: "/etc/oxidns/persistent_ips.txt",
        },
        min_ttl: {
          label: "动态项最小 TTL",
          description: "定义动态地址项允许使用的最小 TTL。",
        },
        max_ttl: {
          label: "动态项最大 TTL",
          description: "定义动态地址项允许使用的最大 TTL。",
        },
        fixed_ttl: {
          label: "动态项固定 TTL",
          description: "为所有动态写入项指定固定 TTL。",
        },
        cleanup_on_shutdown: {
          label: "关闭时清理",
          description: "控制插件退出时是否清理由其管理的条目。",
        },
      },
      metrics: {
        labels: {
          ros_address_list_observe_total: "观测域名",
          ros_address_list_dropped_total: "异步丢弃",
          ros_address_list_sync_error_total: "同步失败",
          ros_address_list_sync_timeout_total: "同步超时",
        },
        help: {
          ros_address_list_observe_total:
            "提交给 RouterOS address-list 管理器的域名观测总数。",
          ros_address_list_dropped_total:
            "异步模式下因队列已满或通道关闭而丢弃的观测总数。",
          ros_address_list_sync_error_total:
            "同步模式下在 RouterOS 管理器侧失败的观测总数。",
          ros_address_list_sync_timeout_total:
            "同步模式下入队或等待超时的观测总数。",
        },
      },
    },
    upgrade: {
      name: "Upgrade",
      description: "执行 OxiDNS 升级流程",
      fields: {
        force: {
          label: "强制升级",
          description:
            "即使目标 release 不比当前版本更新，也继续下载、校验并替换。",
        },
        cleanup: {
          label: "升级后清理缓存",
          description: "升级成功后清理 cache_dir 和 backup_dir。",
        },
        repository: {
          label: "GitHub 仓库",
          description: "GitHub 仓库。",
        },
        asset: {
          label: "Release Asset",
          description:
            "Release asset 名称；auto 会根据当前平台和编译版本选择 archive。",
        },
        bundle: {
          label: "编译版本",
          description:
            "asset 为 auto 时使用的编译版本；auto 会跟随当前二进制的编译版本。",
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
            "GitHub 个人访问令牌，用于提高 API 速率限制或访问私有仓库。",
        },
        cache_dir: {
          label: "下载缓存目录",
          description: "下载缓存目录。",
          placeholder: "./upgrade/cache",
        },
        backup_dir: {
          label: "备份目录",
          description: "替换前备份目录。",
          placeholder: "./upgrade/backups",
        },
        webui_dir: {
          label: "WebUI 目录",
          description: "升级时安装 WebUI 静态资源的目录。",
          placeholder: "./webui",
        },
        skip_webui: {
          label: "跳过 WebUI 升级",
          description: "apply 时跳过 WebUI 目录升级，仅替换二进制文件。",
        },
        no_restart: {
          label: "跳过自动重启",
          description: "启用后升级成功也不会触发自动重启。",
        },
        timeout: {
          label: "超时",
          description: "限制升级过程的总等待时间。",
        },
        outbound: {
          label: "出站配置",
          description:
            "引用 network.outbound.profiles 中的出站配置，用于升级下载。",
          placeholder: "profile-1",
        },
        socks5: {
          label: "SOCKS5 代理",
          description: "升级下载时使用的 SOCKS5 代理。",
          placeholder: "127.0.0.1:1080",
        },
        insecure_skip_verify: {
          label: "跳过 HTTPS 证书校验",
          description: "升级下载时跳过 HTTPS 证书校验。",
        },
      },
      quickSetup: {
        paramPlaceholder: "force=true",
      },
    },
    download: {
      name: "Download",
      description: "下载 HTTP/HTTPS 文件到本地目录并原子覆盖",
      fields: {
        downloads: {
          label: "下载项",
          description:
            "下载一个或多个 http/https 文件到本地目录，并在新内容完整写入后覆盖目标文件。",
          placeholder:
            '[{"url":"https://example.com/geosite.dat","dir":"/etc/oxidns","filename":"geosite.dat"}]',
        },
        "downloads[]": {
          label: "下载项",
        },
        "downloads[].url": {
          label: "URL",
          description: "下载项的 http/https URL。",
          placeholder: "https://example.com/geosite.dat",
        },
        "downloads[].dir": {
          label: "目录",
          description: "下载项的目标目录。",
          placeholder: "/etc/oxidns",
        },
        "downloads[].filename": {
          label: "文件名",
          description: "下载项的目标文件名。",
          placeholder: "geosite.dat",
        },
        timeout: {
          label: "超时",
          description: "下载超时时间。",
        },
        outbound: {
          label: "出站配置",
          description:
            "引用 network.outbound.profiles 中的出站配置，用于统一控制下载解析器和代理。",
          placeholder: "profile-1",
        },
        socks5: {
          label: "SOCKS5 代理",
          description: "所有下载连接都会通过该 SOCKS5 代理发起。",
          placeholder: "127.0.0.1:1080",
        },
        startup_if_missing: {
          label: "启动时补齐缺失文件",
          description:
            "启动时检查目标文件，缺失项会在其它插件初始化前自动下载。",
        },
      },
      quickSetup: {
        paramPlaceholder: "https://example.com/rules.txt /etc/oxidns",
      },
      metrics: {
        labels: {
          download_success_total: "下载成功",
          download_failure_total: "下载失败",
          download_timeout_total: "下载超时",
        },
        help: {
          download_success_total: "成功完成的文件下载总次数。",
          download_failure_total: "下载失败的总次数（不含超时）。",
          download_timeout_total: "因超时中断的文件下载总次数。",
        },
      },
    },
    reload_provider: {
      name: "Reload Provider",
      description: "按 tag 定向刷新一个或多个 provider",
      fields: {
        args: {
          label: "Provider 引用",
          description: "按 args 中声明顺序逐个执行 targeted provider reload。",
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
          reload_provider_reload_total: "数据源重载",
          reload_provider_reload_error_total: "重载失败",
        },
        help: {
          reload_provider_reload_total:
            "该执行器触发的 provider 重载尝试总次数。",
          reload_provider_reload_error_total: "provider 重载失败的总次数。",
        },
      },
    },
    reload: {
      name: "Reload",
      description: "触发一次应用级全量 reload",
      fields: {},
      metrics: {
        labels: {
          reload_trigger_total: "重载触发",
          reload_error_total: "重载失败",
        },
        help: {
          reload_trigger_total: "该执行器请求应用级全量 reload 的总次数。",
          reload_error_total: "重载请求调度失败的总次数。",
        },
      },
    },
    cron: {
      name: "Cron",
      description: "后台按 cron 或固定间隔调度一组 executor",
      fields: {
        jobs: {
          label: "任务列表",
          description: "定义一个或多个后台任务。",
          placeholder:
            '[{"name":"refresh_sets","interval":"5m","executors":["$seq_refresh"]}]',
        },
        "jobs[]": {
          label: "任务",
        },
        "jobs[].name": {
          label: "任务名称",
          description: "任务名称，用于日志与运行时标识。",
          placeholder: "refresh_sets",
        },
        "jobs[].schedule": {
          label: "Cron 表达式",
          description: "使用标准 5 字段 cron 表达式调度任务。",
          placeholder: "0 */6 * * *",
        },
        "jobs[].interval": {
          label: "固定间隔",
          description: "用简单固定间隔调度任务。",
          placeholder: "5m",
        },
        "jobs[].executors": {
          label: "执行器",
          description: "定义任务触发时顺序执行的 executor 列表。",
          placeholder: "$seq_refresh\ndebug_print cron refresh",
        },
        "jobs[].executors.$executor_ref": {
          label: "引用 executor",
          placeholder: "seq_refresh",
        },
        "jobs[].executors.$input": {
          label: "输入值",
          placeholder: "debug_print cron refresh",
        },
        timezone: {
          label: "时区",
          description: "为当前 cron 插件下的所有 schedule 任务指定时区。",
          placeholder: "Asia/Shanghai",
        },
      },
      metrics: {
        labels: {
          cron_job_run_total: "任务运行",
          cron_job_skipped_total: "重叠跳过",
          cron_executor_error_total: "执行器失败",
        },
        help: {
          cron_job_run_total: "Cron 任务被启动的总次数。",
          cron_job_skipped_total: "因上次运行尚未结束而跳过本次触发的总次数。",
          cron_executor_error_total: "Cron 任务运行中各执行器失败的总次数。",
        },
      },
    },
    any_match: {
      name: "Any Match",
      description: "组合多个 matcher，任意一个命中即返回 true",
      fields: {
        args: {
          label: "匹配表达式",
          description:
            "每行一个 matcher 表达式，支持 $tag、快捷表达式和 ! 取反",
          placeholder: "$match_tag\nqname domain:example.com\n!$blocked",
        },
        "args.$matcher_ref": {
          label: "引用 matcher",
          placeholder: "match_tag",
        },
        "args.$input": {
          label: "输入值",
          placeholder: "qname domain:example.com",
        },
      },
    },
    qname: {
      name: "QName",
      description: "匹配请求中的查询域名",
      fields: {
        args: {
          label: "域名规则",
          description: "定义域名匹配规则来源。",
          placeholder:
            "full:login.example.com\ndomain:example.com\nkeyword:cdn\nregexp:^api[0-9]+\\.example\\.net$\n$core_domains\n&/etc/oxidns/domains.txt",
        },
        "args.$provider_ref": {
          label: "引用 provider",
          placeholder: "core_domains",
        },
        "args.$input": {
          label: "输入值",
          placeholder: "regexp:^api[0-9]+\\.example\\.net$",
        },
      },
      quickSetup: {
        paramPlaceholder: "$domain_set",
      },
    },
    question: {
      name: "Question",
      description: "按 provider 的 contains_question 语义匹配请求 question",
      fields: {
        args: {
          label: "Provider 引用",
          description:
            "使用 $provider_tag 形式引用实现了 contains_question 的 provider。",
          placeholder: "$ad_rules\n$shared_domains",
        },
        "args[]": {
          label: "引用 provider",
          placeholder: "ad_rules",
        },
      },
      quickSetup: {
        paramPlaceholder: "$ad_rules",
      },
    },
    qtype: {
      name: "QType",
      description: "匹配请求 qtype",
      fields: {
        args: {
          label: "QType 文本或数值",
          description:
            "定义允许命中的查询类型集合，同时支持 A/AAAA 等文本和对应数值。",
          placeholder: "A\nAAAA\n1\n28",
        },
        "args[]": {
          label: "输入值",
          placeholder: "A",
        },
      },
      quickSetup: {
        paramPlaceholder: "A,AAAA 或 1,28",
      },
    },
    qclass: {
      name: "QClass",
      description: "匹配请求 qclass",
      fields: {
        args: {
          label: "QClass 文本或数值",
          description:
            "定义允许命中的查询类别集合，同时支持 IN/CH 等文本和对应数值。",
          placeholder: "IN\n1",
        },
        "args[]": {
          label: "输入值",
          placeholder: "IN",
        },
      },
      quickSetup: {
        paramPlaceholder: "IN 或 1",
      },
    },
    client_ip: {
      name: "Client IP",
      description: "匹配客户端来源 IP",
      fields: {
        args: {
          label: "IP / CIDR / ip_set",
          description: "定义客户端来源地址匹配条件。",
          placeholder: "192.168.0.0/16\n$lan_ip_set",
        },
        "args.$provider_ref": {
          label: "引用 provider",
          placeholder: "lan_ip_set",
        },
        "args.$input": {
          label: "输入值",
          placeholder: "192.168.0.0/16",
        },
      },
      quickSetup: {
        paramPlaceholder: "$lan_ip_set",
      },
    },
    resp_ip: {
      name: "Response IP",
      description: "匹配响应 answers 中的 A/AAAA IP",
      fields: {
        args: {
          label: "IP / CIDR / ip_set",
          description: "定义应答地址匹配条件。",
          placeholder: "100.64.0.0/10\n$special_targets",
        },
        "args.$provider_ref": {
          label: "引用 provider",
          placeholder: "special_targets",
        },
        "args.$input": {
          label: "输入值",
          placeholder: "100.64.0.0/10",
        },
      },
      quickSetup: {
        paramPlaceholder: "$ip_set",
      },
    },
    ptr_ip: {
      name: "PTR IP",
      description: "从 PTR 请求名解析 IP 后匹配",
      fields: {
        args: {
          label: "IP / CIDR / ip_set",
          description: "定义 PTR 反查地址匹配条件。",
          placeholder: "192.168.0.0/16\n$lan_ip_set",
        },
        "args.$provider_ref": {
          label: "引用 provider",
          placeholder: "lan_ip_set",
        },
        "args.$input": {
          label: "输入值",
          placeholder: "192.168.0.0/16",
        },
      },
      quickSetup: {
        paramPlaceholder: "$lan_ip_set",
      },
    },
    cname: {
      name: "CNAME",
      description: "匹配响应中的 CNAME 目标域名",
      fields: {
        args: {
          label: "CNAME 规则",
          description: "定义响应 CNAME 目标域名匹配规则来源。",
          placeholder:
            "full:alias.example.com\ndomain:example.com\nkeyword:cdn\nregexp:^edge[0-9]+\\.example\\.net$\n$core_domains\n&/etc/oxidns/cnames.txt",
        },
        "args.$provider_ref": {
          label: "引用 provider",
          placeholder: "core_domains",
        },
        "args.$input": {
          label: "输入值",
          placeholder: "regexp:^edge[0-9]+\\.example\\.net$",
        },
      },
      quickSetup: {
        paramPlaceholder: "$domain_set",
      },
    },
    rcode: {
      name: "RCode",
      description: "匹配当前响应 rcode",
      fields: {
        args: {
          label: "RCode 文本或数值",
          description:
            "定义允许命中的响应码集合，同时支持 SERVFAIL/NXDOMAIN 等文本和对应数值。",
          placeholder: "NOERROR\nSERVFAIL\nNXDOMAIN\n0\n2\n3",
        },
        "args[]": {
          label: "输入值",
          placeholder: "NOERROR",
        },
      },
      quickSetup: {
        paramPlaceholder: "SERVFAIL,NXDOMAIN 或 2,3",
      },
    },
    has_resp: {
      name: "Has Response",
      description: "上下文中已有响应时命中",
      fields: {},
    },
    has_wanted_ans: {
      name: "Has Wanted Answer",
      description: "响应 answers 中包含请求 qtype 对应记录时命中",
      fields: {},
    },
    mark: {
      name: "Mark",
      description: "匹配上下文中的 mark 集合",
      fields: {
        args: {
          label: "Mark",
          description: "定义允许命中的 mark 集合。",
          placeholder: "100\n200",
        },
        "args[]": {
          label: "输入值",
          placeholder: "100",
        },
      },
      quickSetup: {
        paramPlaceholder: "100,200",
      },
    },
    env: {
      name: "Env",
      description: "匹配进程环境变量",
      fields: {
        args: {
          label: "环境变量条件",
          description: "定义需要同时满足的环境变量条件。",
          placeholder: "PROFILE=prod\nFEATURE_X",
        },
        "args[]": {
          label: "输入值",
          placeholder: "PROFILE=prod",
        },
      },
      quickSetup: {
        paramPlaceholder: "PROFILE=prod FEATURE_X",
      },
    },
    random: {
      name: "Random",
      description: "按概率命中",
      fields: {
        args: {
          label: "概率",
          description: "定义 matcher 命中概率。",
          placeholder: "0.1",
        },
        "args[]": {
          label: "输入值",
          placeholder: "0.1",
        },
      },
      quickSetup: {
        paramPlaceholder: "0.1",
      },
    },
    rate_limiter: {
      name: "Rate Limiter",
      description: "基于客户端 IP 的令牌桶限流",
      fields: {
        qps: {
          label: "QPS",
          description: "定义每秒令牌补充速率。",
        },
        burst: {
          label: "突发容量",
          description: "定义令牌桶容量上限。",
        },
        mask4: {
          label: "IPv4 聚合前缀",
          description: "定义 IPv4 客户端聚合粒度。",
        },
        mask6: {
          label: "IPv6 聚合前缀",
          description: "定义 IPv6 客户端聚合粒度。",
        },
      },
      quickSetup: {
        paramPlaceholder: "20 40",
      },
      metrics: {
        labels: {
          ratelimit_allowed_total: "放行",
          ratelimit_rejected_total: "限流拒绝",
        },
        help: {
          ratelimit_allowed_total: "通过限流检查（令牌充足）的匹配总次数。",
          ratelimit_rejected_total: "因令牌耗尽而被限流拒绝的匹配总次数。",
        },
        derived: {
          "percent_of_sum:ratelimit_rejected_total/ratelimit_rejected_total+ratelimit_allowed_total":
            "拒绝率",
        },
      },
    },
    string_exp: {
      name: "String Expression",
      description: "通用字符串表达式匹配器",
      fields: {
        args: {
          label: "表达式",
          description: "通用字符串表达式匹配器表达式。",
          placeholder: "url_path prefix /dns-",
        },
      },
      quickSetup: {
        paramPlaceholder: "url_path prefix /dns-",
      },
    },
    _true: {
      name: "Always True",
      description: "恒为真，可作为保底命中条件",
      fields: {},
    },
    _false: {
      name: "Always False",
      description: "恒为假，可用于临时禁用某条规则",
      fields: {},
    },
    domain_set: {
      name: "Domain Set",
      description: "高性能域名规则集合，可被 qname、cname 等引用",
      fields: {
        exps: {
          label: "内联域名规则",
          description: "定义内联域名表达式列表。",
          placeholder:
            "full:login.example.com\ndomain:example.com\nkeyword:cdn\nregexp:^api[0-9]+\\.example\\.net$",
        },
        "exps[]": {
          label: "输入值",
          placeholder: "full:login.example.com",
        },
        files: {
          label: "域名规则文件",
          description: "指定外部规则文件路径列表。",
          placeholder: "/etc/oxidns/domains.txt",
        },
        "files[]": {
          label: "输入值",
          placeholder: "/etc/oxidns/domains.txt",
        },
        sets: {
          label: "下游 Provider",
          description: "引用其它具备域名匹配能力的 provider。",
          placeholder: "shared_domains\nshared_geosite",
        },
        "sets[]": {
          label: "引用 provider",
          placeholder: "shared_domains",
        },
      },
    },
    dynamic_domain_set: {
      name: "Dynamic Domain Set",
      description: "可写的本地域名规则文件，支持自动学习与 API 管理",
      fields: {
        path: {
          label: "规则文件",
          description: "指定该动态 provider 管理的本地规则文件路径。",
          placeholder: "/etc/oxidns/learned-allow.txt",
        },
        bootstrap_rules: {
          label: "初始规则",
          description: "仅当规则文件不存在时写入初始规则。",
          placeholder: "full:login.example.com\ndomain:example.com",
        },
        "bootstrap_rules[]": {
          label: "输入值",
          placeholder: "full:login.example.com",
        },
        queue_size: {
          label: "队列大小",
          description: "定义自动学习写入队列大小。",
        },
        batch_size: {
          label: "批量写入条数",
          description: "定义后台 append 的批量 flush 阈值。",
        },
        flush_interval_ms: {
          label: "Flush 间隔(ms)",
          description: "定义后台 append 的定时 flush 间隔。",
        },
      },
    },
    geosite: {
      name: "Geosite",
      description: "从 geosite.dat 提取域名规则集合",
      fields: {
        file: {
          label: "geosite.dat",
          description: "指定 geosite.dat 文件路径。",
          placeholder: "/etc/oxidns/geosite.dat",
        },
        selectors: {
          label: "Selector",
          description:
            "按 code 提取部分规则，也支持 code@attribute 语法按 attribute 进一步过滤。",
          placeholder: "cn\ngeolocation-!cn",
        },
        "selectors[]": {
          label: "输入值",
          placeholder: "cn",
        },
      },
    },
    adguard_rule: {
      name: "AdGuard Rule",
      description: "提供 AdGuard Home DNS 规则子集",
      fields: {
        rules: {
          label: "内联规则",
          description: "提供 AdGuard Home DNS 规则子集。",
          placeholder: "||ads.example.com^\n@@||safe.ads.example.com^",
        },
        "rules[]": {
          label: "输入值",
          placeholder: "||ads.example.com^",
        },
        files: {
          label: "规则文件",
          description: "从外部规则文件加载。",
          placeholder: "/etc/oxidns/adguard.txt",
        },
        "files[]": {
          label: "输入值",
          placeholder: "/etc/oxidns/adguard.txt",
        },
      },
    },
    ip_set: {
      name: "IP Set",
      description: "IP / CIDR 规则集合，可被 client_ip、resp_ip、ptr_ip 引用",
      fields: {
        ips: {
          label: "IP / CIDR",
          description: "定义内联 IP 或 CIDR 规则列表。",
          placeholder: "192.168.0.0/16\nfd00::/8",
        },
        "ips[]": {
          label: "输入值",
          placeholder: "192.168.0.0/16",
        },
        files: {
          label: "IP 规则文件",
          description: "指定外部 IP 规则文件路径列表。",
          placeholder: "/etc/oxidns/ips.txt",
        },
        "files[]": {
          label: "输入值",
          placeholder: "/etc/oxidns/ips.txt",
        },
        sets: {
          label: "下游 Provider",
          description: "引用其它 ip_set 实例。",
          placeholder: "shared_ip_set\nshared_geoip",
        },
        "sets[]": {
          label: "引用 provider",
          placeholder: "shared_ip_set",
        },
      },
    },
    geoip: {
      name: "GeoIP",
      description: "从 geoip.dat 提取 IP / CIDR 集合",
      fields: {
        file: {
          label: "geoip.dat",
          description: "指定 geoip.dat 文件路径。",
          placeholder: "/etc/oxidns/geoip.dat",
        },
        selectors: {
          label: "Selector",
          description: "按 code 提取 IP / CIDR 集合。",
          placeholder: "cn",
        },
        "selectors[]": {
          label: "输入值",
          placeholder: "cn",
        },
      },
    },
  },
} as const;
