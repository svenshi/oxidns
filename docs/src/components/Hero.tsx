import React, { useEffect, useRef, useState } from 'react';
import Link from '@docusaurus/Link';

const ICONS = {
  gauge: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M12 14 4 6" />
      <path d="M12 2v2" />
      <path d="M5 5 7 7" />
      <path d="M2 12h2" />
      <path d="M19 5 17 7" />
      <path d="M22 12h-2" />
      <circle cx="12" cy="14" r="8" />
    </svg>
  ),
  route: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="6" cy="19" r="3" />
      <path d="M9 19h8.5a3.5 3.5 0 0 0 0-7h-11a3.5 3.5 0 0 1 0-7H15" />
      <circle cx="18" cy="5" r="3" />
    </svg>
  ),
  lock: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <rect x="3" y="11" width="18" height="11" rx="2" />
      <path d="M7 11V7a5 5 0 0 1 10 0v4" />
    </svg>
  ),
  plug: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M12 22v-5" />
      <path d="M9 8V2" />
      <path d="M15 8V2" />
      <path d="M18 8v4a4 4 0 0 1-4 4h-4a4 4 0 0 1-4-4V8Z" />
    </svg>
  ),
};

type Locale = 'zh' | 'en';
type InstallTab = { label: string; prompt: string; code: string };
type Feature = { icon: keyof typeof ICONS; title: string; desc: string; href: string };
type NextStep = { num: number; label: string; href: string };
type Community = { title: string; desc: string; cta: string; qrAlt: string };
type HeroProps = { locale?: Locale };

const TELEGRAM_URL = 'https://t.me/oxidns';
const TELEGRAM_QR = '/img/telegram-qr.png';

const INSTALL_TABS: InstallTab[] = [
  { label: 'Linux / macOS', prompt: '$', code: 'curl -fsSL https://oxidns.org/install.sh | sudo sh' },
  { label: 'Windows', prompt: 'PS>', code: 'irm https://oxidns.org/install.ps1 | iex' },
  { label: 'OpenWrt', prompt: '#', code: 'curl -fsSL https://oxidns.org/install.sh | sh' },
  { label: 'Docker', prompt: '$', code: 'docker run -d \\\n' +
          '  --name oxidns \\\n' +
          '  --restart unless-stopped \\\n' +
          '  -p 53:53/udp \\\n' +
          '  -p 53:53/tcp \\\n' +
          '  -p 9199:9199/tcp \\\n' +
          '  -v "$(pwd)/config.yaml:/etc/oxidns/config.yaml:ro" \\\n' +
          '  svenshi/oxidns:latest' },
  { label: 'Cargo', prompt: '$', code: 'cargo install oxidns' },
];

const COPY = {
  zh: {
    eyebrow: 'Rust · DNS Engine · v1.x',
    titleAccent: '面向复杂网络的高性能 DNS 策略编排引擎',
    tagline: '用 Rust 重写的高性能 DNS 服务，灵感来自 MosDNS，为复杂策略、加密上游和系统联动而设计。',
    quickstart: '快速开始',
    github: 'GitHub',
    copy: 'Copy',
    copied: '✓ 已复制',
    nextTitle: '推荐阅读路径',
    features: [
      {
        icon: 'gauge',
        title: '性能优先',
        desc: '自研 DNS 消息层和 wire 编解码，缩短热路径，缓存与上游连接全程复用。',
        href: '/architecture-and-design',
      },
      {
        icon: 'route',
        title: '可编排策略',
        desc: 'sequence + matcher + executor + provider 四层插件，把所有能力收敛到策略层。',
        href: '/configuration',
      },
      {
        icon: 'lock',
        title: '完整加密上游',
        desc: '原生支持 UDP / TCP / DoT / DoH / DoH3 / DoQ，并提供 fallback 与并发竞争。',
        href: '/plugin-reference/overview',
      },
      {
        icon: 'plug',
        title: '系统联动',
        desc: 'ipset / nftset / MikroTik address-list / Prometheus，DNS 解析驱动网络行为。',
        href: '/mikrotik-policy-routing',
      },
    ] satisfies Feature[],
    nextSteps: [
      { num: 1, label: '阅读《快速开始》，完成首次启动', href: '/quickstart' },
      { num: 2, label: '从《常见策略场景》挑选最接近你的部署目标的配置', href: '/scenarios' },
      { num: 3, label: '阅读《配置总览》，理解 YAML 顶层结构与 sequence 编排', href: '/configuration' },
      { num: 4, label: '需要 Web 控制台？查看《WebUI 部署》', href: '/webui' },
      { num: 5, label: '想理解设计取舍？阅读《架构与设计》', href: '/architecture-and-design' },
    ] satisfies NextStep[],
    community: {
      title: '加入社区',
      desc: '欢迎进入 Telegram 群与作者和其他用户交流配置、反馈问题或讨论新特性。',
      cta: 'Telegram · @OXIDNS',
      qrAlt: 'OxiDNS Telegram 群二维码',
    } satisfies Community,
  },
  en: {
    eyebrow: 'Rust · DNS Engine · v1.x',
    titleAccent: 'A high-performance DNS policy orchestration engine for complex networks',
    tagline: 'A high-performance DNS service built with Rust, inspired by MosDNS, and designed for complex policy, encrypted upstreams, and system integrations.',
    quickstart: 'Quick Start',
    github: 'GitHub',
    copy: 'Copy',
    copied: '✓ Copied',
    nextTitle: 'Recommended path',
    features: [
      {
        icon: 'gauge',
        title: 'Performance first',
        desc: 'A dedicated DNS message layer and wire codec keep the hot path short while cache and upstream connections are reused.',
        href: '/architecture-and-design',
      },
      {
        icon: 'route',
        title: 'Composable policy',
        desc: 'The sequence + matcher + executor + provider plugin layers keep DNS behavior in one policy pipeline.',
        href: '/configuration',
      },
      {
        icon: 'lock',
        title: 'Encrypted upstreams',
        desc: 'Native UDP / TCP / DoT / DoH / DoH3 / DoQ support with fallback and upstream racing.',
        href: '/plugin-reference/overview',
      },
      {
        icon: 'plug',
        title: 'System integration',
        desc: 'ipset, nftset, MikroTik address lists, and Prometheus let DNS results drive network behavior.',
        href: '/mikrotik-policy-routing',
      },
    ] satisfies Feature[],
    nextSteps: [
      { num: 1, label: 'Read Quick Start and complete the first successful launch', href: '/quickstart' },
      { num: 2, label: 'Choose the closest deployment from Common Scenarios', href: '/scenarios' },
      { num: 3, label: 'Read Configuration to understand YAML and sequence orchestration', href: '/configuration' },
      { num: 4, label: 'Need the console? Open WebUI Deployment', href: '/webui' },
      { num: 5, label: 'Want the design trade-offs? Read Architecture and Design', href: '/architecture-and-design' },
    ] satisfies NextStep[],
    community: {
      title: 'Join the community',
      desc: 'Hop into the Telegram group to chat with the author and other users about configuration, feedback, or upcoming features.',
      cta: 'Telegram · @OXIDNS',
      qrAlt: 'OxiDNS Telegram group QR code',
    } satisfies Community,
  },
} as const;

export default function Hero({ locale = 'zh' }: HeroProps): JSX.Element {
  const [active, setActive] = useState(0);
  const [copied, setCopied] = useState(false);
  const codeRef = useRef<HTMLSpanElement>(null);
  const typedRef = useRef<{ destroy: () => void } | null>(null);
  const text = COPY[locale];

  useEffect(() => {
    let cancelled = false;

    typedRef.current?.destroy();
    typedRef.current = null;

    if (codeRef.current) {
      codeRef.current.textContent = '';
    }

    if (window.matchMedia?.('(prefers-reduced-motion: reduce)').matches) {
      if (codeRef.current) {
        codeRef.current.textContent = INSTALL_TABS[active].code;
      }
      return () => {
        cancelled = true;
      };
    }

    void import('typed.js').then(({ default: Typed }) => {
      if (cancelled || !codeRef.current) {
        return;
      }

      typedRef.current = new Typed(codeRef.current, {
        strings: [INSTALL_TABS[active].code],
        typeSpeed: 16,
        backSpeed: 0,
        startDelay: 120,
        showCursor: true,
        cursorChar: '▌',
        smartBackspace: false,
        loop: false,
      });
    });

    return () => {
      cancelled = true;
      typedRef.current?.destroy();
      typedRef.current = null;
    };
  }, [active]);

  const copy = async () => {
    await navigator.clipboard?.writeText(INSTALL_TABS[active].code);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1400);
  };

  const selectTab = (index: number) => {
    setActive(index);
    setCopied(false);
  };

  return (
    <section className="oxi-hero">
      <span className="oxi-hero__eyebrow">
        <span className="oxi-hero__eyebrow-dot" />
        {text.eyebrow}
      </span>

      <h1 className="oxi-hero__title">
        OxiDNS
        <span className="oxi-hero__title-accent">{text.titleAccent}</span>
      </h1>

      <div className="oxi-hero__tagline">{text.tagline}</div>

      <div className="oxi-hero__ctas">
        <Link className="oxi-hero__cta oxi-hero__cta--primary" to="/quickstart">
          {text.quickstart}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round" style={{ width: '1rem', height: '1rem' }}>
            <path d="M5 12h14" />
            <path d="m13 5 7 7-7 7" />
          </svg>
        </Link>
        <Link className="oxi-hero__cta oxi-hero__cta--ghost" to="https://github.com/svenshi/oxidns">{text.github}</Link>
      </div>

      <div className="oxi-hero__install">
        <div className="oxi-hero__install-tabs">
          {INSTALL_TABS.map((tab, index) => (
            <button
              key={tab.label}
              type="button"
              className={`oxi-hero__install-tab${index === active ? ' oxi-hero__install-tab--active' : ''}`}
              onClick={() => selectTab(index)}
            >
              {tab.label}
            </button>
          ))}
        </div>
        <div className="oxi-hero__install-body">
          <span className="oxi-hero__install-prompt">{INSTALL_TABS[active].prompt}</span>
          <div className="oxi-hero__install-code" aria-live="polite">
            <span ref={codeRef}>{INSTALL_TABS[active].code}</span>
          </div>
          <button
            type="button"
            className={`oxi-hero__install-copy${copied ? ' oxi-hero__install-copy--copied' : ''}`}
            onClick={copy}
          >
            {copied ? text.copied : text.copy}
          </button>
        </div>
      </div>

      <div className="oxi-hero__features">
        {text.features.map((feature) => (
          <Link key={feature.title} className="oxi-hero__feature" to={feature.href}>
            <div className="oxi-hero__feature-icon">{ICONS[feature.icon]}</div>
            <h3 className="oxi-hero__feature-title">{feature.title}</h3>
            <p className="oxi-hero__feature-desc">{feature.desc}</p>
          </Link>
        ))}
      </div>

      <div className="oxi-hero__next">
        <h2 className="oxi-hero__next-title">{text.nextTitle}</h2>
        <ol className="oxi-hero__next-list">
          {text.nextSteps.map((step) => (
            <li key={step.num}>
              <Link to={step.href}>
                <span className="oxi-hero__next-num">{step.num}</span>
                <span>{step.label}</span>
              </Link>
            </li>
          ))}
        </ol>
      </div>

      <div className="oxi-hero__community">
        <div className="oxi-hero__community-text">
          <h2 className="oxi-hero__community-title">{text.community.title}</h2>
          <p className="oxi-hero__community-desc">{text.community.desc}</p>
          <Link className="oxi-hero__community-cta" to={TELEGRAM_URL}>
            <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
              <path d="M21.198 3.105 2.43 10.42c-1.281.514-1.273 1.235-.234 1.553l4.815 1.502 11.144-7.03c.527-.32 1.008-.148.612.204l-9.03 8.155h-.002l.002.001-.332 4.964c.488 0 .703-.224.976-.49l2.347-2.282 4.864 3.593c.897.494 1.541.24 1.764-.831l3.193-15.04c.327-1.317-.503-1.914-1.351-1.614Z"/>
            </svg>
            {text.community.cta}
          </Link>
        </div>
        <a className="oxi-hero__community-qr" href={TELEGRAM_URL} target="_blank" rel="noopener noreferrer">
          <img src={TELEGRAM_QR} alt={text.community.qrAlt} loading="lazy" />
        </a>
      </div>
    </section>
  );
}
