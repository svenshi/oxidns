/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

import { describe, expect, it } from "vitest";

import {
  AUTO_UPDATE_CHECK_INTERVAL_MS,
  FAILED_UPDATE_CHECK_RETRY_INTERVAL_MS,
  isAutomaticUpdateCheckDue,
  updateCheckOptionsFingerprint,
  updateCheckRequestKey,
} from "./update-check-policy";

const requestKey = updateCheckRequestKey({
  backend: "/api",
  currentVersion: "0.9.0",
  repository: "svenshi/oxidns",
  bundle: "full",
  allowPrerelease: false,
  requestOptionsFingerprint: updateCheckOptionsFingerprint(["", "", ""]),
});

describe("automatic update-check policy", () => {
  it("checks immediately when no previous attempt exists", () => {
    expect(isAutomaticUpdateCheckDue(null, requestKey, 10_000)).toBe(true);
  });

  it("waits one day after a successful check", () => {
    const previous = { requestKey, checkedAt: 10_000, succeeded: true };

    expect(
      isAutomaticUpdateCheckDue(
        previous,
        requestKey,
        10_000 + AUTO_UPDATE_CHECK_INTERVAL_MS - 1,
      ),
    ).toBe(false);
    expect(
      isAutomaticUpdateCheckDue(
        previous,
        requestKey,
        10_000 + AUTO_UPDATE_CHECK_INTERVAL_MS,
      ),
    ).toBe(true);
  });

  it("retries a failed check after one hour", () => {
    const previous = { requestKey, checkedAt: 10_000, succeeded: false };

    expect(
      isAutomaticUpdateCheckDue(
        previous,
        requestKey,
        10_000 + FAILED_UPDATE_CHECK_RETRY_INTERVAL_MS - 1,
      ),
    ).toBe(false);
    expect(
      isAutomaticUpdateCheckDue(
        previous,
        requestKey,
        10_000 + FAILED_UPDATE_CHECK_RETRY_INTERVAL_MS,
      ),
    ).toBe(true);
  });

  it("checks immediately when the request context changes", () => {
    expect(
      isAutomaticUpdateCheckDue(
        { requestKey, checkedAt: 10_000, succeeded: true },
        updateCheckRequestKey({
          backend: "/api",
          currentVersion: "0.10.0",
          repository: "svenshi/oxidns",
          bundle: "full",
          allowPrerelease: false,
          requestOptionsFingerprint: updateCheckOptionsFingerprint([
            "",
            "",
            "",
          ]),
        }),
        10_001,
      ),
    ).toBe(true);
  });

  it("changes when backend or request transport credentials change", () => {
    const changedBackend = updateCheckRequestKey({
      backend: "https://dns.example/api",
      currentVersion: "0.9.0",
      repository: "svenshi/oxidns",
      bundle: "full",
      allowPrerelease: false,
      requestOptionsFingerprint: updateCheckOptionsFingerprint(["", "", ""]),
    });
    const changedToken = updateCheckRequestKey({
      backend: "/api",
      currentVersion: "0.9.0",
      repository: "svenshi/oxidns",
      bundle: "full",
      allowPrerelease: false,
      requestOptionsFingerprint: updateCheckOptionsFingerprint([
        "",
        "",
        "new-token",
      ]),
    });

    expect(changedBackend).not.toBe(requestKey);
    expect(changedToken).not.toBe(requestKey);
  });
});
