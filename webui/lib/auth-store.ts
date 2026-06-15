"use client";

import { create } from "zustand";
import { persist } from "zustand/middleware";
import { WEBUI, tClient } from "./i18n";

export interface ServerConfig {
  url: string;
  requiresAuth: boolean;
  username: string;
  password: string;
}

export function normalizeServerUrl(url: string): string {
  const trimmed = url.trim();
  return trimmed ? trimmed.replace(/\/+$/, "") : "/api";
}

export function isSameServerIdentity(left: ServerConfig, right: ServerConfig) {
  return normalizeServerUrl(left.url) === normalizeServerUrl(right.url);
}

export interface AuthState {
  serverConfig: ServerConfig;
  isAuthenticated: boolean;
  isConnected: boolean;
  isConnecting: boolean;
  isHydrated: boolean;
  hasAttemptedAutoConnect: boolean;
  connectionError: string | null;
  needsCredentials: boolean;
  rememberLogin: boolean;

  setServerConfig: (config: ServerConfig) => void;
  connect: (config?: ServerConfig) => Promise<boolean>;
  attemptAutoConnect: () => Promise<void>;
  markHydrated: () => void;
  setRememberLogin: (remember: boolean) => void;
  logout: () => void;
}

export const useAuthStore = create<AuthState>()(
  persist(
    (set, get) => ({
      serverConfig: {
        url: "/api",
        requiresAuth: false,
        username: "",
        password: "",
      },
      isAuthenticated: false,
      isConnected: false,
      isConnecting: false,
      isHydrated: false,
      hasAttemptedAutoConnect: false,
      connectionError: null,
      needsCredentials: false,
      rememberLogin: true,

      setServerConfig: (config) =>
        set((state) => ({
          serverConfig: config,
          ...(isSameServerConfig(state.serverConfig, config)
            ? {}
            : {
                isAuthenticated: false,
                isConnected: false,
                connectionError: null,
                needsCredentials: false,
              }),
        })),

      setRememberLogin: (remember) => set({ rememberLogin: remember }),

      logout: () =>
        set((state) => ({
          isConnected: false,
          isAuthenticated: false,
          needsCredentials: true,
          connectionError: null,
          serverConfig: {
            ...state.serverConfig,
            username: "",
            password: "",
          },
        })),

      connect: async (config?: ServerConfig) => {
        set({ isConnecting: true, connectionError: null });

        const serverConfig = config ?? get().serverConfig;

        try {
          const url = serverConfig.url.trim();
          if (!url) {
            throw new Error(tClient(WEBUI.storeErrors.serviceUrlRequired));
          }
          const headers: Record<string, string> = {
            Accept: "application/json",
          };
          if (serverConfig.requiresAuth) {
            if (!serverConfig.username || !serverConfig.password) {
              // Credentials known to be incomplete (e.g. rememberLogin=false cleared
              // the password). Skip the network round-trip and show the login form.
              set({
                isConnecting: false,
                needsCredentials: true,
                connectionError: null,
              });
              return false;
            }
            headers.Authorization = `Basic ${btoa(`${serverConfig.username}:${serverConfig.password}`)}`;
          }
          const response = await fetch(`${url.replace(/\/$/, "")}/health`, {
            method: "GET",
            headers,
          });
          if (response.status === 401) {
            set({
              isConnected: false,
              isAuthenticated: false,
              isConnecting: false,
              needsCredentials: true,
              connectionError:
                serverConfig.requiresAuth &&
                serverConfig.username &&
                serverConfig.password
                  ? tClient(WEBUI.storeErrors.invalidCredentials)
                  : null,
              serverConfig: { ...serverConfig, requiresAuth: true },
            });
            return false;
          }
          if (!response.ok) {
            throw new Error(
              tClient(WEBUI.storeErrors.connectionHttpFailed, {
                status: response.status,
              }),
            );
          }
          set({
            serverConfig,
            isConnected: true,
            isAuthenticated: true,
            isConnecting: false,
            needsCredentials: false,
          });
          return true;
        } catch (error) {
          set({
            isConnected: false,
            isAuthenticated: false,
            isConnecting: false,
            needsCredentials: false,
            connectionError:
              error instanceof Error
                ? error.message
                : tClient(WEBUI.storeErrors.connectionFailed),
          });
          return false;
        }
      },

      attemptAutoConnect: async () => {
        if (get().hasAttemptedAutoConnect) return;
        set({ hasAttemptedAutoConnect: true });
        if (get().isConnecting) return;
        await get().connect();
      },

      markHydrated: () => set({ isHydrated: true }),
    }),
    {
      name: "oxidns-auth",
      // Don't persist live connection flags: every page load should
      // re-verify the backend before assuming we're online.
      // When rememberLogin is false, strip the password so the next
      // visit forces the user to re-enter it (username is kept for pre-fill).
      partialize: (state) => ({
        rememberLogin: state.rememberLogin,
        serverConfig: state.rememberLogin
          ? state.serverConfig
          : { ...state.serverConfig, password: "" },
      }),
      onRehydrateStorage: () => (state) => {
        state?.markHydrated();
      },
    },
  ),
);

function isSameServerConfig(left: ServerConfig, right: ServerConfig) {
  return (
    normalizeServerUrl(left.url) === normalizeServerUrl(right.url) &&
    left.requiresAuth === right.requiresAuth &&
    left.username === right.username &&
    left.password === right.password
  );
}
