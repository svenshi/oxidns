const sidebars = {
  docsSidebar: [
    'intro',
    {
      type: 'category',
      label: '入门',
      collapsed: false,
      items: ['quickstart', 'openwrt', 'scenarios', 'migrate-from-mosdns'],
    },
    {
      type: 'category',
      label: '配置参考',
      items: ['configuration', 'dns-codes', 'cli', 'custom-build'],
    },
    {
      type: 'category',
      label: '插件',
      items: [
        'plugin-reference/overview',
        'plugin-reference/server',
        'plugin-reference/executor',
        'plugin-reference/matcher',
        'plugin-reference/provider',
      ],
    },
    {
      type: 'category',
      label: '管理与集成',
      items: ['webui', 'api', 'mikrotik-policy-routing'],
    },
    {
      type: 'category',
      label: '架构与性能',
      items: ['architecture-and-design', 'benchmarks'],
    },
    {
      type: 'category',
      label: '项目',
      items: ['roadmap', 'releases'],
    },
  ],
};

export default sidebars;
