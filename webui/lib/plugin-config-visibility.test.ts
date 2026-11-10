/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

import { describe, expect, it } from "vitest";

import {
  createPluginConfigFormValues,
  serializePluginConfigValues,
} from "@/components/plugins/plugin-config-fields-editor";
import { getVisiblePluginConfigFields } from "./plugin-config-visibility";
import { getLocalizedPluginKindDefinition } from "./plugin-definitions";

const cases = [
  {
    kind: "learn_domain",
    field: "success_only",
    control: "phase",
    active: "after",
    inactive: "before",
    defaultVisible: true,
    value: false,
  },
  {
    kind: "learn_domain",
    field: "answer_required",
    control: "phase",
    active: "after",
    inactive: "before",
    defaultVisible: true,
    value: false,
  },
  {
    kind: "learn_domain",
    field: "timeout",
    control: "async",
    active: false,
    inactive: true,
    defaultVisible: false,
    value: "4s",
  },
  {
    kind: "http_request",
    field: "queue_size",
    control: "async",
    active: true,
    inactive: false,
    defaultVisible: true,
    value: 128,
  },
  {
    kind: "ros_route",
    field: "wait_timeout",
    control: "async",
    active: false,
    inactive: true,
    defaultVisible: false,
    value: "4s",
  },
  {
    kind: "ros_address_list",
    field: "wait_timeout",
    control: "async",
    active: false,
    inactive: true,
    defaultVisible: false,
    value: "4s",
  },
  {
    kind: "upgrade",
    field: "bundle",
    control: "asset",
    active: "auto",
    inactive: "custom.tar.gz",
    defaultVisible: true,
    value: "minimal",
  },
  {
    kind: "upgrade",
    field: "webui_dir",
    control: "skip_webui",
    active: false,
    inactive: true,
    defaultVisible: true,
    value: "/srv/webui",
  },
];

describe("conditional plugin configuration", () => {
  it.each(cases)(
    "shows $kind.$field only when $control applies",
    (testCase) => {
      for (const locale of ["zh-CN", "en-US"] as const) {
        const fields = getLocalizedPluginKindDefinition(
          testCase.kind,
          locale,
        )!.configSchema;
        for (const [controlValue, expected] of [
          [undefined, testCase.defaultVisible],
          [testCase.active, true],
          [testCase.inactive, false],
        ] as const) {
          const values =
            controlValue === undefined
              ? {}
              : { [testCase.control]: controlValue };
          const visible = getVisiblePluginConfigFields(fields, values);
          expect(visible.some((field) => field.key === testCase.field)).toBe(
            expected,
          );
          expect(visible.some((field) => field.key === testCase.control)).toBe(
            true,
          );
        }
      }
    },
  );

  it.each(cases)(
    "retains $kind.$field when hidden and then revealed",
    (testCase) => {
      const fields = getLocalizedPluginKindDefinition(
        testCase.kind,
        "en-US",
      )!.configSchema;
      const values = createPluginConfigFormValues(fields, {
        [testCase.control]: testCase.inactive,
        [testCase.field]: testCase.value,
      });
      expect(
        getVisiblePluginConfigFields(fields, values).some(
          (field) => field.key === testCase.field,
        ),
      ).toBe(false);
      expect(serializePluginConfigValues(fields, values)[testCase.field]).toBe(
        testCase.value,
      );
      values[testCase.control] = testCase.active;
      expect(
        getVisiblePluginConfigFields(fields, values).some(
          (field) => field.key === testCase.field,
        ),
      ).toBe(true);
      expect(serializePluginConfigValues(fields, values)[testCase.field]).toBe(
        testCase.value,
      );
    },
  );
});
