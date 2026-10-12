// SPDX-FileCopyrightText: 2026 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

import {createRequire} from 'node:module';
import {themes as prismThemes} from 'prism-react-renderer';

const require = createRequire(import.meta.url);

// Enable Algolia DocSearch after approval by setting these env vars.
// Without them, the docs keep the existing local search plugin as a fallback.
const docSearchAppId = process.env.DOCSEARCH_APP_ID ?? process.env.ALGOLIA_APP_ID;
const docSearchApiKey = process.env.DOCSEARCH_API_KEY ?? process.env.ALGOLIA_API_KEY;
const docSearchIndexName = process.env.DOCSEARCH_INDEX_NAME ?? process.env.ALGOLIA_INDEX_NAME;

const algoliaConfig = docSearchAppId && docSearchApiKey && docSearchIndexName
  ? {
      appId: docSearchAppId,
      apiKey: docSearchApiKey,
      indexName: docSearchIndexName,
      contextualSearch: true,
      searchParameters: {},
    }
  : undefined;

/** @type {import('@docusaurus/types').Config} */
const config = {
  title: 'OxiDNS',
  tagline: 'A Rust-powered DNS engine inspired by MosDNS, designed for performance and complete configurability.',
  favicon: 'img/logo-light.png',

  scripts: [
    {
      src: '/js/theme-favicon.js',
      defer: true,
    },
  ],

  future: {
    v4: true,
  },

  url: 'https://oxidns.org',
  baseUrl: '/',

  organizationName: 'svenshi',
  projectName: 'oxidns',

  onBrokenLinks: 'throw',
  onBrokenAnchors: 'throw',

  i18n: {
    defaultLocale: 'zh-Hans',
    locales: ['zh-Hans', 'en'],
    localeConfigs: {
      'zh-Hans': {
        label: '中文',
      },
      en: {
        label: 'English',
      },
    },
  },

  markdown: {
    mermaid: true,
    hooks: {
      onBrokenMarkdownLinks: 'throw',
    },
  },

  themes: ['@docusaurus/theme-mermaid'],

  plugins: [
    !algoliaConfig && [
      require.resolve('@easyops-cn/docusaurus-search-local'),
      {
        hashed: true,
        docsRouteBasePath: '/',
        indexDocs: true,
        indexBlog: false,
        indexPages: false,
        language: ['zh', 'en'],
        highlightSearchTermsOnTargetPage: true,
        searchBarShortcut: true,
        searchBarShortcutHint: true,
        searchResultLimits: 8,
        explicitSearchResultPath: true,
      },
    ],
  ].filter(Boolean),

  presets: [
    [
      '@docusaurus/preset-classic',
      ({
        docs: {
          path: './docs',
          routeBasePath: '/',
          sidebarPath: './sidebars.js',
          editUrl: ({locale, docPath}) => {
            const sourcePath = locale === 'en'
              ? 'i18n/en/docusaurus-plugin-content-docs/current'
              : 'docs';
            return `https://github.com/svenshi/oxidns/edit/main/docs/${sourcePath}/${docPath}`;
          },
          showLastUpdateTime: true,
        },
        blog: false,
        theme: {
          customCss: './src/css/custom.css',
        },
      }),
    ],
  ],

  themeConfig: ({
      colorMode: {
        defaultMode: 'light',
        disableSwitch: false,
        respectPrefersColorScheme: false,
      },
      navbar: {
        title: 'OxiDNS',
        logo: {
          alt: 'OxiDNS Logo',
          src: 'img/logo-light.png',
          srcDark: 'img/logo-dark.png',
          width: 32,
          height: 32,
        },
        items: [
          {
            to: '/quickstart',
            label: '快速开始',
            position: 'left',
          },
          {
            to: '/plugin-reference/overview',
            label: '插件参考',
            position: 'left',
          },
          {
            type: 'localeDropdown',
            position: 'right',
          },
          {
            href: 'https://github.com/svenshi/oxidns',
            'aria-label': 'GitHub repository',
            className: 'header-github-link',
            position: 'right',
          },
          {
            type: 'search',
            position: 'right',
          },
        ],
      },
      footer: {
        style: 'light',
        links: [
          {
            title: '文档',
            items: [
              {label: '快速开始', to: '/quickstart'},
              {label: '配置指南', to: '/configuration'},
              {label: '插件参考', to: '/plugin-reference/overview'},
              {label: '文档版本', to: '/documentation'},
            ],
          },
          {
            title: '运维',
            items: [
              {label: '运维与故障排查', to: '/operations'},
              {label: '安全加固', to: '/security'},
              {label: '管理 API', to: '/api'},
            ],
          },
          {
            title: '社区',
            items: [
              {label: '参与贡献', to: '/contributing'},
              {label: '支持项目开发', to: '/support-development'},
              {label: 'GitHub Discussions', href: 'https://github.com/svenshi/oxidns/discussions'},
              {label: 'Telegram', href: 'https://t.me/oxidns'},
            ],
          },
        ],
        copyright: `Copyright © ${new Date().getFullYear()} OxiDNS`,
      },
      prism: {
        theme: prismThemes.oneDark,
        darkTheme: prismThemes.oneDark,
        additionalLanguages: ['shell-session', 'powershell', 'bash'],
      },
      ...(algoliaConfig ? {algolia: algoliaConfig} : {}),
    }),
};

export default config;
