// SPDX-FileCopyrightText: 2026 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

import React from 'react';
import Link from '@docusaurus/Link';
import useDocusaurusContext from '@docusaurus/useDocusaurusContext';
import './PluginAvailability.css';

const bundles = ['minimal', 'standard', 'full'] as const;
type Bundle = (typeof bundles)[number];

type PluginAvailabilityProps = {
  from: Bundle;
  compact?: boolean;
  note?: string;
};

export default function PluginAvailability({
  from,
  compact = false,
  note,
}: PluginAvailabilityProps) {
  const {i18n} = useDocusaurusContext();
  const isChinese = i18n.currentLocale === 'zh-Hans';
  const included = bundles.slice(bundles.indexOf(from));
  const label = isChinese ? '内置于' : 'Included in';
  const description = `${label} ${included.join(', ')}`;

  if (compact) {
    return (
      <span className={`plugin-bundle plugin-bundle--${from}`} title={description} aria-label={description}>
        {from}
        {included.length > 1 && <span className="plugin-bundle__plus" aria-hidden="true">+</span>}
      </span>
    );
  }

  return (
    <div className="plugin-availability">
      <div className="plugin-availability__row" role="group" aria-label={description}>
        <span className="plugin-availability__label">{label}</span>
        <span className="plugin-availability__bundles">
          {included.map((bundle) => (
            <span key={bundle} className={`plugin-bundle plugin-bundle--${bundle}`}>{bundle}</span>
          ))}
        </span>
      </div>
      {note && <p className="plugin-availability__note">{note}</p>}
    </div>
  );
}

export function BundleGuide() {
  const {i18n} = useDocusaurusContext();
  const isChinese = i18n.currentLocale === 'zh-Hans';
  const descriptions = isChinese
    ? ['精简核心', '常用扩展', '完整功能']
    : ['Essential core', 'Everyday extensions', 'All capabilities'];

  return (
    <aside className="bundle-guide" aria-label={isChinese ? '构建版本说明' : 'Build edition guide'}>
      <div className="bundle-guide__header">
        <span className="bundle-guide__title">{isChinese ? '找到插件所在的构建版本' : 'Find the right build for your plugins'}</span>
        <Link to="/custom-build" className="bundle-guide__link">
          {isChinese ? '自定义构建' : 'Custom builds'} <span aria-hidden="true">↗</span>
        </Link>
      </div>
      <div className="bundle-guide__editions">
        {bundles.map((bundle, index) => (
          <div className={`bundle-guide__edition bundle-guide__edition--${bundle}`} key={bundle}>
            <div className="bundle-guide__name">
              <span className={`plugin-bundle plugin-bundle--${bundle}`}>{bundle}</span>
              {bundle === 'full' && <span className="bundle-guide__default">{isChinese ? '默认构建' : 'Default build'}</span>}
            </div>
            <span className="bundle-guide__description">{descriptions[index]}</span>
          </div>
        ))}
      </div>
      <p className="bundle-guide__caption">
        {isChinese
          ? '版本逐级包含：minimal+ 表示三个版本均收录，standard+ 表示 standard 与 full 收录。'
          : 'Builds are cumulative: minimal+ includes all three; standard+ includes standard and full.'}
      </p>
      <p className="bundle-guide__footnote">
        {isChinese
          ? '标签表示构建中已包含该插件；使用时仍需在配置中启用。'
          : 'Labels indicate plugins included in a build. Enable them in your configuration to use them.'}
      </p>
    </aside>
  );
}
