/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

import { describe, expect, it } from "vitest";

import { selectCardConfigFields } from "./default-plugin-card";

const fields = [
  { key: "first", label: "First", type: "text" as const },
  { key: "second", label: "Second", type: "text" as const },
  { key: "third", label: "Third", type: "text" as const },
  { key: "fourth", label: "Fourth", type: "text" as const },
  {
    key: "advanced",
    label: "Advanced",
    type: "number" as const,
    advanced: true,
  },
];

describe("default plugin card config summary", () => {
  it("prioritizes configured primary fields while preserving schema order", () => {
    expect(
      selectCardConfigFields(fields, { third: "configured" }).map(
        (field) => field.key,
      ),
    ).toEqual(["third", "first", "second", "fourth"]);
  });

  it("does not let an advanced field displace primary configuration", () => {
    expect(
      selectCardConfigFields(fields, { advanced: true }).map(
        (field) => field.key,
      ),
    ).toEqual(["first", "second", "third", "fourth", "advanced"]);
  });

  it("uses configured advanced fields when no primary fields exist", () => {
    expect(
      selectCardConfigFields(
        [
          {
            key: "unused",
            label: "Unused",
            type: "number",
            advanced: true,
          },
          {
            key: "enabled",
            label: "Enabled",
            type: "switch",
            advanced: true,
          },
        ],
        { enabled: false },
      ).map((field) => field.key),
    ).toEqual(["enabled"]);
  });

  it("uses the expanded six-item card capacity", () => {
    const expanded = Array.from({ length: 8 }, (_, index) => ({
      key: `field_${index}`,
      label: `Field ${index}`,
      type: "text" as const,
    }));

    expect(
      selectCardConfigFields(expanded, {}).map((field) => field.key),
    ).toEqual([
      "field_0",
      "field_1",
      "field_2",
      "field_3",
      "field_4",
      "field_5",
    ]);
  });

  it("keeps required fields ahead of optional configured fields", () => {
    const prioritized = [
      { key: "optional", label: "Optional", type: "text" as const },
      {
        key: "required",
        label: "Required",
        type: "reference" as const,
        required: true,
      },
    ];

    expect(
      selectCardConfigFields(prioritized, { optional: "value" }).map(
        (field) => field.key,
      ),
    ).toEqual(["required", "optional"]);
  });

  it("excludes sensitive and high-entropy content fields", () => {
    const unsafe = [
      { key: "username", label: "Username", type: "text" as const },
      { key: "password", label: "Password", type: "password" as const },
      { key: "github_token", label: "Token", type: "text" as const },
      { key: "body", label: "Body", type: "textarea" as const },
      { key: "json", label: "JSON", type: "json" as const },
    ];

    expect(
      selectCardConfigFields(unsafe, {
        username: "admin",
        password: "secret",
        github_token: "token",
        body: "payload",
        json: { secret: true },
      }).map((field) => field.key),
    ).toEqual(["username"]);
  });
});
