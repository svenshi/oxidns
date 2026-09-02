# WebUI Guidelines

Paths in this guide are relative to `webui/` unless stated otherwise.

## Structure & Commands

- `webui/` contains the Next.js-based management console for OxiDNS. Treat it as a separate frontend workspace that mirrors the plugin model exposed by the Rust server.
- `app/` uses the Next App Router. The `(console)` route group owns the console shell, dashboard, plugin center, settings page, and full-screen config editor mode.
- `components/` contains feature components, while `components/ui/` contains shadcn/Radix-style primitives. Prefer composing existing primitives before adding new low-level UI.
- `components/plugins/` contains plugin-center rendering. Generic card/detail templates live there, and per-plugin overrides live under `components/plugins/kinds/`.
- `lib/plugin-definitions/` is the source of truth for WebUI plugin kinds, labels, icons, descriptions, and config schemas. Each category has its own file (`executor.ts`, `matcher.ts`, `provider.ts`, `server.ts`); `lib/plugin-definitions.ts` aggregates and exports them all as `pluginKindDefinitions`.
- `lib/plugin-definitions/docs.ts` holds fallback field-level documentation keyed by plugin kind; it is merged automatically via `withFieldDocs()`. Localized user-facing docs live under `lib/i18n/locales/*/docs.ts`.
- `lib/i18n/` contains locale state, translation keys, localized WebUI copy, localized plugin definitions, and localized plugin field docs. Keep it aligned whenever adding user-facing UI text or plugin metadata.
- `lib/store.ts` contains the current client state model with Zustand. Backend API wiring should replace mock actions behind this store shape where possible instead of scattering fetch logic through views.
- `package.json`, the lockfile, and the repository-root
  `.github/workflows/webui-ci.yml` define the supported package manager, scripts,
  build mode, and CI checks. Invoke scripts from the manifest instead of copying
  their command bodies into this guide.

## Coding Style

- WebUI code is TypeScript + React. Use `PascalCase` for components, `camelCase` for props/functions, and colocate feature-only helpers near the feature.
- Prefer named exports for shared WebUI components and helpers.
- Use the `@/` path alias for WebUI imports instead of deep relative paths.
- Keep WebUI files client/server explicit: add `"use client"` only for components that need hooks, browser state, event handlers, Zustand, or theme APIs.
- Do not hard-code user-facing copy in components, stores, API helpers, or plugin views. Add translation keys and locale resources instead, while canonical protocol/config tokens such as plugin kinds, YAML keys, DNS record types, metric names, and plugin type labels like `Server`, `Executor`, `Matcher`, and `Provider` may remain literal when they are part of the domain model.
- Use `lucide-react` icons for toolbar actions, navigation, and plugin visuals when an icon exists.

## Internationalization

- The locale registry and resources under `lib/i18n/` define supported locales
  and fallback behavior. When adding or changing WebUI text, update the key and
  every maintained locale implementation discovered there.
- Use `useI18n()` inside React components and read `t`, `locale`, `formatNumber`, and `formatDateTime` from that hook. Use `tClient()` only in non-component code that cannot access the provider, such as Zustand store actions, API helpers, or pure utility paths.
- Keep interpolation placeholders stable across locales (`{name}`, `{count}`, `{version}`, etc.) and pass values through `t(key, params)`. Do not build translated sentences by concatenating fragments in JSX.
- Use `formatNumber()` and `formatDateTime()` for user-visible numbers and timestamps when the active locale matters. Avoid direct `toLocaleString()` calls unless the locale is explicitly supplied.
- Keep plugin config schemas in `lib/plugin-definitions/` domain-focused and language-neutral where possible. Put localized plugin names, descriptions, field labels, placeholders, option labels, metric labels/help, derived metric labels, and quick-setup placeholders in `lib/i18n/locales/*/plugin-defined.ts`.
- Put localized field-level documentation in `lib/i18n/locales/*/docs.ts`. When a plugin kind, config field, metric, or docs entry is added/renamed, update both `zh-CN` and `en-US` resources together.
- Use locale-aware plugin helpers such as `getLocalizedPluginKindDefinitions()`, `getLocalizedPluginKindDefinition()`, `pluginTypeLabel()`, `pluginStatusLabel()`, and `getPluginSearchText()` for catalogs, forms, search, CodeMirror completions, cards, and detail views.
- Search indexes, placeholders, empty states, validation messages, toast/dialog copy, accessibility labels, `sr-only` text, tooltips, document title, and meta description are all user-facing and should be localized.
- Leave machine-readable values unlocalized: YAML field names, plugin `kind` values, tags, API payload keys, route paths, enum values sent to the backend, Prometheus metric identifiers, DNS qtypes/rcodes, and configuration examples that users must paste verbatim.

## Architecture & Extension Principles

- Keep console-shell ownership centralized in `app/(console)/layout.tsx` and the
  shell components it imports; derive the current component flow from those
  imports instead of preserving a copied chain here.
- Keep global UI state in `useAppStore` until backend integration introduces a clearer API boundary. Avoid duplicating selected plugin, drawer state, editor mode, or restart/save flags in page-local stores.
- Treat `PluginInstance` in `lib/types.ts` as the UI model for live plugin instances. Keep its `type` aligned with OxiDNS plugin categories: `server`, `executor`, `matcher`, and `provider`.
- **Schema registration for a new plugin kind requires one definition-file change.** Add the definition to the category file selected by the current exports under `lib/plugin-definitions/`. The following behavior auto-derives from that definition without another schema registry:
  - Plugin catalog and type-filtered lists (`pluginCatalog`, `getPluginCatalogItemsByType`)
  - Create-plugin dialog (search, listing, schema-driven form)
  - Default card and detail drawer (`PluginCardTemplate`, `PluginDetailTemplate`)
  - Plugin index panel (kind-to-category mapping)
  - Sequence composer and quick-setup insertion
  - YAML editor completions and inline validation
- New or changed plugin definitions still need i18n resources for user-facing labels, descriptions, field text, metric text, and docs. Update `lib/i18n/locales/zh-CN/` and `lib/i18n/locales/en-US/` alongside the schema change.
- Two optional follow-up steps exist for richer UI: (1) create `components/plugins/kinds/<kind>.tsx` with custom `Card`/`Detail` components and register it in `components/plugins/registry.ts`; (2) add fallback field-level docs to `lib/plugin-definitions/docs.ts` when a non-localized fallback is useful. Both fall back gracefully if omitted.
- Use `ConfigField` schemas for plugin configuration instead of hand-built one-off forms whenever possible. This keeps create/edit behavior consistent and preserves YAML/plugin concepts like references, arrays, objects, records, durations, and JSON fields.
- Keep configuration guidance semantic: `default` is the runtime/schema default and remains omitted from generated YAML while inherited; `initialValue` is an intentional new-form value that is serialized; `example` is machine-readable input guidance; and `description` explains behavior or constraints. Do not encode defaults in examples. The generic editor keeps the legacy `placeholder` property as a compatibility fallback, but built-in definitions should use `example`.
- Present generic plugin configuration as a responsive settings list with normal-weight labels and short descriptions in the metadata column. Read-only views show plain values instead of disabled controls. In edit mode, inherited defaults use a quieter control treatment, while explicitly configured optional values expose only a subdued reset action on row hover, keyboard focus, or narrow screens; do not render persistent provenance badges. Restoring a default removes the explicit YAML key. Show inherited primitive arrays as compact values rather than raw JSON, and stack each row when its field-group container is narrow.
- Use neutral muted color for inherited defaults and the low-saturation `config-example` theme token for example placeholders and helper text. Keep explicit values at the normal foreground contrast, and reserve destructive, warning, and primary status colors for their actual semantics. Text such as `Example:` must remain present so color is never the only distinction.
- Use `ConfigField.advanced` for optional tuning, timeout, queue, cache, transport override, lifecycle, and legacy compatibility fields. Keep required fields, primary targets, rule data, chain-control fields such as `short_circuit`, and safety-significant behavior visible. The generic editor honors this metadata at the top level and inside nested object/array-object schemas; custom plugin editors must reuse `AdvancedSettingsSection` for the same behavior.
- New-plugin forms keep advanced sections collapsed and display schema defaults without materializing them for untouched advanced fields. Existing plugin forms automatically reveal a section when its serialized YAML explicitly contains any advanced field, including an explicit default, `false`, `0`, or an empty object. Collapsing a section must never remove or rewrite its values.
- Use `referenceTypes`, `referencePrefix`, and `allowInvert` for fields that point to other plugins or matcher expressions. Do not encode `$tag` and `!$tag` handling in individual plugin components unless the schema editor cannot represent the shape.
- Put optional custom plugin visuals in `components/plugins/kinds/<kind>.tsx` and register them in `components/plugins/registry.ts`. If a custom component does not add meaningful clarity, rely on `PluginCardTemplate` and `PluginDetailTemplate`.
- Keep plugin cards focused on scanability: name, category, kind, status/primary metric, and compact operational controls. Push detailed configuration, charts, and destructive actions into the detail sheet.
- In compact card configuration summaries, reserve a predictable share of each item for the field label and truncate long values first. Metric summaries may continue to prioritize their numeric values. Keep the full label and value available through their existing title affordances.
- Keep `CreatePluginDialog` catalog-driven. Search should cover kind, display name, description, type label, and config fields so operators can find plugins by the concept they remember.
- When replacing mock data with real APIs, keep network calls outside low-level UI primitives and preserve optimistic UI only where the backend operation is reversible or clearly reported.
- Structured plugin mutations must patch the source-backed YAML/CST instead of
  stringifying the complete configuration or `plugins` list. Preserve all
  untouched source bytes, keep schema-unknown fields in field mode, and stop
  for review or explicit confirmation when only a localized reconstruction is
  possible. The full YAML editor remains authoritative for intentional raw
  text changes.

### Shared Toasts and asynchronous runtime actions

- Use the shared toast provider and programmatic API implemented by the root
  providers for operational warnings and errors. Keep notices dismissible and
  avoid per-card blocks that change repeated-item height; presentation defaults
  belong in the shared implementation.
- For asynchronous runtime actions, an accepted mutation response is not completion. Correlate the returned operation ID with a lightweight status endpoint, keep the initiating control loading until the matching terminal result arrives, and reserve inline success styling for confirmed success.
- Follow the runtime-operation transport implemented by `lib/store.ts` and
  `lib/oxidns-api.ts`. Avoid per-card N+1 requests; when polling is used, batch
  visible state, serialize rounds, cancel stale sessions, and deduplicate
  consecutive failure feedback. A push transport may replace polling while
  preserving those lifecycle properties.
- On a new polling session, restore active loading states but treat existing terminal results as a baseline so reopening a view does not replay stale success or error feedback. Backend-active state takes visual priority over transient success feedback.

## Design Principles

- The WebUI is an operational DNS console, not a marketing site. Prioritize dense, calm, scan-friendly screens over decorative layouts.
- Preserve the current visual language: dark mode by default, light mode supported, OKLCH design tokens in `app/globals.css`, teal/green primary accents, restrained borders, muted surfaces, and compact spacing.
- Use shadcn/Radix primitives from `components/ui/` for buttons, dialogs, sheets, tabs, tables, inputs, tooltips, badges, sidebars, and forms. Extend primitives only when repeated product behavior needs it.
- Prefer full-width work surfaces and simple sections. Use cards for individual repeated items, metrics, dialogs, and framed editor/helper panels; avoid nesting cards inside cards.
- Keep navigation persistent and predictable: sidebar for main sections, header for breadcrumbs and global actions, sheets/dialogs for focused secondary workflows.
- Use icon buttons with tooltips for compact global actions such as theme switching, restart, view mode, and editor mode. Include `sr-only` text for icon-only buttons.
- Keep typography compact: page headings around `text-lg`, operational labels at `text-sm`/`text-xs`, plugin tags and config keys in mono where useful. Do not use oversized hero typography inside the console.
- Ensure responsive behavior for desktop and narrow screens with stable grids (`sm`, `lg`, `xl`) and fixed-width side panels only when there is enough viewport room. Avoid layouts where labels, buttons, or badges can overlap.
- Use semantic status color sparingly: primary for active/healthy emphasis, destructive for dangerous actions, yellow/amber only for unsaved or warning states, muted foreground for secondary metadata.
- Use the shared `warning` color token for `always_false`, and the `destructive` token for the higher-risk `always_true` mode. Represent fixed Boolean values with off/on toggle icons; use a neutral controls icon for `normal`. Both fixed modes require confirmation, while restoring `normal` does not. Confirmation copy must state how positive and negated references behave.
- Do not add gradient blobs, decorative illustrations, or broad one-color themes. The interface should feel like a precise control surface for OxiDNS.

## Testing & Documentation

- For WebUI behavior changes, select the applicable scripts from `package.json`;
  its script definitions and the repository-root
  `.github/workflows/webui-ci.yml` are authoritative for exact commands and CI
  coverage.
- For visual WebUI changes, verify the affected route in both light and dark themes, and check narrow and desktop widths for overflow, clipped labels, and broken grid/card layouts.
- If a Rust plugin is added, renamed, or its config shape changes, update the appropriate file in `lib/plugin-definitions/`, the matching i18n resources under `lib/i18n/locales/*/plugin-defined.ts` and `lib/i18n/locales/*/docs.ts`, and optionally `lib/plugin-definitions/docs.ts` in the same change so the console stays aligned with runtime behavior. Custom kind components under `components/plugins/kinds/` only need updating if they reference removed or renamed fields.
- If WebUI architecture, styling tokens, plugin schema conventions, or console workflows change, update this `ai/webui.md` file.
