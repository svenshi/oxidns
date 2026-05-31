---
title: 路线图
sidebar_position: 5
---

# 路线图

以下是 OxiDNS 近期规划中的开发方向，按开发顺序排列。

```mermaid
flowchart LR
  A["① 编译定制化"] --> B["② IP 优选"]
  B --> C["③ MikroTik 深度集成"]
  C --> D["④ OpenWrt 支持"]
  D --> E["⑤ WebUI 与指标增强"]

  style A fill:#f0fdfa,stroke:#0f766e,stroke-width:2px,color:#0f4c46
  style B fill:#f0fdfa,stroke:#0f766e,stroke-width:2px,color:#0f4c46
  style C fill:#f0fdfa,stroke:#0f766e,stroke-width:2px,color:#0f4c46
  style D fill:#f0fdfa,stroke:#0f766e,stroke-width:2px,color:#0f4c46
  style E fill:#f0fdfa,stroke:#0f766e,stroke-width:2px,color:#0f4c46
```

<div style={{display: 'flex', flexDirection: 'column', gap: '0.75rem', marginTop: '1.75rem'}}>

  <div className="doc-plugin-card" style={{display: 'flex', gap: '1.25rem', alignItems: 'flex-start'}}>
    <div style={{flexShrink: 0, width: '2.2rem', height: '2.2rem', borderRadius: '50%', background: 'var(--ifm-color-primary)', color: '#fff', display: 'flex', alignItems: 'center', justifyContent: 'center', fontWeight: 800, fontSize: '0.95rem', marginTop: '0.1rem'}}>1</div>
    <div>
      <div className="doc-plugin-card__eyebrow">第一阶段 · 已完成 ✓</div>
      <h3 className="doc-plugin-card__title">编译定制化</h3>
      <p style={{margin: '0.4rem 0 0', lineHeight: 1.7}}>按功能模块拆分编译，用户 fork 仓库后可自由组合所需插件，构建精简的定制版本，并通过自定义仓库地址实现自动更新。<br/><strong>已完成</strong>：<code>minimal</code> / <code>standard</code> / <code>full</code> 三档组合包；每个协议栈与管理面都已 feature 化 —— <code>api</code> / <code>webui</code> / <code>metrics</code>、<code>server-dot</code> / <code>server-doh</code> / <code>server-doq</code> / <code>server-doh3</code>、<code>upstream-dot</code> / <code>upstream-doh</code> / <code>upstream-doq</code> / <code>upstream-doh3</code>,以及 MikroTik、query_recorder、ipset/nftset、cron、script、upgrade、download、http_request、reverse_lookup、geo provider、adguard_rule 均可单独裁剪。<code>AppController</code> / <code>LogBuffer</code> 已下沉到 <code>src/core/</code>,因此 <code>minimal</code> 构建排除了 hyper / rustls / quinn,release 二进制约为 <code>full</code> 的 40%(≈ 8.9 MB vs 21 MB)。详见 <a href="/docs/custom-build">自定义编译</a>。</p>
    </div>
  </div>

  <div className="doc-plugin-card" style={{display: 'flex', gap: '1.25rem', alignItems: 'flex-start'}}>
    <div style={{flexShrink: 0, width: '2.2rem', height: '2.2rem', borderRadius: '50%', background: 'var(--ifm-color-primary)', color: '#fff', display: 'flex', alignItems: 'center', justifyContent: 'center', fontWeight: 800, fontSize: '0.95rem', marginTop: '0.1rem'}}>2</div>
    <div>
      <div className="doc-plugin-card__eyebrow">第二阶段</div>
      <h3 className="doc-plugin-card__title">IP 优选</h3>
      <p style={{margin: '0.4rem 0 0', lineHeight: 1.7}}>对 DNS 响应中的多个 A/AAAA 地址并行测速，自动选出延迟最低的 IP 返回给客户端，提升实际访问速度。</p>
    </div>
  </div>

  <div className="doc-plugin-card" style={{display: 'flex', gap: '1.25rem', alignItems: 'flex-start'}}>
    <div style={{flexShrink: 0, width: '2.2rem', height: '2.2rem', borderRadius: '50%', background: 'var(--ifm-color-primary)', color: '#fff', display: 'flex', alignItems: 'center', justifyContent: 'center', fontWeight: 800, fontSize: '0.95rem', marginTop: '0.1rem'}}>3</div>
    <div>
      <div className="doc-plugin-card__eyebrow">第三阶段</div>
      <h3 className="doc-plugin-card__title">MikroTik 深度集成</h3>
      <p style={{margin: '0.4rem 0 0', lineHeight: 1.7}}>在现有单向推送基础上，新增从 RouterOS 拉取地址列表作为 OxiDNS 数据源，以及将本地 IP 集主动推送到 RouterOS 的能力，实现 DNS 策略与 RouterOS 的双向数据联动。</p>
    </div>
  </div>

  <div className="doc-plugin-card" style={{display: 'flex', gap: '1.25rem', alignItems: 'flex-start'}}>
    <div style={{flexShrink: 0, width: '2.2rem', height: '2.2rem', borderRadius: '50%', background: 'var(--ifm-color-primary)', color: '#fff', display: 'flex', alignItems: 'center', justifyContent: 'center', fontWeight: 800, fontSize: '0.95rem', marginTop: '0.1rem'}}>4</div>
    <div>
      <div className="doc-plugin-card__eyebrow">第四阶段</div>
      <h3 className="doc-plugin-card__title">OpenWrt 支持</h3>
      <p style={{margin: '0.4rem 0 0', lineHeight: 1.7}}>为 OpenWrt 用户提供与 Debian 包同等级别的原生安装体验：通过 opkg 一键安装、服务自动托管、随系统更新，无需手动部署二进制文件。</p>
    </div>
  </div>

  <div className="doc-plugin-card" style={{display: 'flex', gap: '1.25rem', alignItems: 'flex-start'}}>
    <div style={{flexShrink: 0, width: '2.2rem', height: '2.2rem', borderRadius: '50%', background: 'var(--ifm-color-primary)', color: '#fff', display: 'flex', alignItems: 'center', justifyContent: 'center', fontWeight: 800, fontSize: '0.95rem', marginTop: '0.1rem'}}>5</div>
    <div>
      <div className="doc-plugin-card__eyebrow">第五阶段</div>
      <h3 className="doc-plugin-card__title">WebUI 与指标增强</h3>
      <p style={{margin: '0.4rem 0 0', lineHeight: 1.7}}>为各新增插件补充 WebUI 管理界面，扩展 Prometheus 指标覆盖范围，提升可观测性和运维体验。</p>
    </div>
  </div>

</div>

<div style={{borderLeft: '4px solid var(--ifm-color-primary)', background: 'rgba(15, 118, 110, 0.06)', borderRadius: '0 12px 12px 0', padding: '0.9rem 1.2rem', marginTop: '2rem'}}>
  <p style={{margin: 0, lineHeight: 1.75}}><strong>关于插件生态的长期方向</strong></p>
  <ul style={{margin: '0.5rem 0 0', paddingLeft: '1.25rem', lineHeight: 1.75}}>
    <li><strong>WebAssembly 插件</strong>：探索通过 WASM 支持第三方插件，允许开发者用任意语言按规范开发和分发插件，无需修改 OxiDNS 主体代码，并天然获得沙箱隔离保障。</li>
    <li><strong>动态链接库插件</strong>：探索通过动态库（.so / .dylib）加载机制支持原生插件，适合对性能要求极高的场景，开发者可独立编译和分发，OxiDNS 在运行时按需加载。</li>
  </ul>
</div>
