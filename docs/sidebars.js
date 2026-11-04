// SPDX-FileCopyrightText: 2026 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

const sidebars = {
  docsSidebar: [
    'intro',
    {
      type: 'category',
      label: '入门',
      collapsed: false,
      link: {
        type: 'generated-index',
        slug: '/getting-started',
        title: '入门',
        description: '从快速开始、安装部署与常见场景入手，搭建第一条 DNS 策略流水线。',
      },
      items: [
        'quickstart',
        {
          type: 'category',
          label: '安装与部署',
          link: {
            type: 'generated-index',
            slug: '/installation',
            title: '安装与部署',
            description: '选择原生二进制、系统软件包、Docker 或 OpenWrt 的安装方式。',
          },
          items: [
            'installation/native',
            'installation/packages',
            'installation/docker',
            'openwrt',
          ],
        },
        'scenarios',
        'migrate-from-mosdns',
      ],
    },
    {
      type: 'category',
      label: '配置指南',
      link: {type: 'doc', id: 'configuration'},
      items: [
        'configuration/global',
        'configuration/sequence',
        'configuration/rules',
      ],
    },
    {
      type: 'category',
      label: '插件参考',
      link: {type: 'doc', id: 'plugin-reference/overview'},
      items: [
        {
          type: 'category',
          label: '服务端',
          link: {type: 'doc', id: 'plugin-reference/server'},
          items: [
            'plugin-reference/server/datagram-stream',
            'plugin-reference/server/encrypted-http',
          ],
        },
        {
          type: 'category',
          label: '执行器',
          link: {type: 'doc', id: 'plugin-reference/executor'},
          items: [
            'plugin-reference/executor/control-flow',
            'plugin-reference/executor/resolution',
            'plugin-reference/executor/response',
            'plugin-reference/executor/observability',
            'plugin-reference/executor/integrations',
            'plugin-reference/executor/maintenance',
          ],
        },
        {
          type: 'category',
          label: '匹配器',
          link: {type: 'doc', id: 'plugin-reference/matcher'},
          items: [
            'plugin-reference/matcher/request',
            'plugin-reference/matcher/response',
            'plugin-reference/matcher/context',
            'plugin-reference/matcher/composition',
          ],
        },
        {
          type: 'category',
          label: '数据提供器',
          link: {type: 'doc', id: 'plugin-reference/provider'},
          items: [
            'plugin-reference/provider/domain',
            'plugin-reference/provider/ip',
          ],
        },
      ],
    },
    {
      type: 'category',
      label: '部署与运维',
      link: {
        type: 'generated-index',
        slug: '/deployment-and-operations',
        title: '部署与运维',
        description: '了解 WebUI 管理、运行维护与安全部署。',
      },
      items: ['webui', 'operations', 'security'],
    },
    {
      type: 'category',
      label: '接口参考',
      link: {
        type: 'generated-index',
        slug: '/interfaces',
        title: '接口参考',
        description: '查阅命令行、管理 API 与 DNS 状态码的使用说明。',
      },
      items: [
        {
          type: 'category',
          label: '命令行',
          link: {type: 'doc', id: 'cli'},
          items: ['cli/runtime', 'cli/tools', 'cli/upgrade'],
        },
        {
          type: 'category',
          label: '管理 API',
          link: {type: 'doc', id: 'api'},
          items: [
            'api/conventions',
            'api/control',
            'api/configuration',
            'api/plugins',
            'api/metrics',
          ],
        },
        'dns-codes',
      ],
    },
    {
      type: 'category',
      label: '架构与开发',
      link: {
        type: 'generated-index',
        slug: '/development',
        title: '架构与开发',
        description: '理解架构设计、定制构建，并查看性能基准。',
      },
      items: ['architecture-and-design', 'custom-build', 'benchmarks'],
    },
    {
      type: 'category',
      label: '项目与社区',
      link: {
        type: 'generated-index',
        slug: '/community',
        title: '项目与社区',
        description: '了解文档维护、贡献方式、项目支持、路线图与版本更新。',
      },
      items: [
        'documentation',
        'contributing',
        'support-development',
        'roadmap',
        {
          type: 'category',
          label: '版本更新',
          link: {type: 'doc', id: 'releases'},
          items: [
            'releases/2026-06',
            'releases/2026-05',
            'releases/2026-04',
            'releases/2026-03',
          ],
        },
      ],
    },
  ],
};

export default sidebars;
