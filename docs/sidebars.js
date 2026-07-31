const sidebars = {
  docsSidebar: [
    'intro',
    {
      type: 'category',
      label: '入门',
      collapsed: false,
      items: [
        'quickstart',
        {
          type: 'category',
          label: '安装与部署',
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
      items: [
        'plugin-reference/overview',
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
      items: ['webui', 'operations', 'security'],
    },
    {
      type: 'category',
      label: '接口参考',
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
            'api/standard-mode',
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
      items: ['architecture-and-design', 'custom-build', 'benchmarks'],
    },
    {
      type: 'category',
      label: '项目与社区',
      items: [
        'documentation',
        'contributing',
        'support-development',
        'standard-mode-plan',
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
