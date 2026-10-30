"use client";

import * as React from "react";
import { ThemeProvider as NextThemesProvider, useTheme } from "next-themes";

// Suppress React 19 false-positive warning for next-themes inline script tag in development
if (typeof window !== "undefined") {
  const originalError = console.error;
  console.error = (...args: unknown[]) => {
    if (
      typeof args[0] === "string" &&
      (args[0].includes("Encountered a script tag while rendering React component") ||
        args[0].includes("Scripts inside React components are never executed"))
    ) {
      return;
    }
    originalError.apply(console, args);
  };
}

function ThemeProvider({
  children,
  ...props
}: React.ComponentProps<typeof NextThemesProvider>) {
  return (
    <NextThemesProvider
      attribute="class"
      defaultTheme="system"
      enableSystem
      disableTransitionOnChange
      enableColorScheme={false}
      {...props}
    >
      <ThemeFavicon />
      {children}
    </NextThemesProvider>
  );
}

function ThemeFavicon() {
  const { resolvedTheme } = useTheme();

  React.useEffect(() => {
    if (!resolvedTheme) {
      return;
    }

    const href =
      resolvedTheme === "dark" ? "/logo-dark.png" : "/logo-light.png";
    let icon = document.querySelector<HTMLLinkElement>(
      "link[data-oxidns-theme-icon]",
    );

    if (!icon) {
      icon = document.createElement("link");
      icon.rel = "icon";
      icon.dataset.oxidnsThemeIcon = "true";
      document.head.appendChild(icon);
    }

    icon.type = "image/png";
    icon.media = "";
    icon.href = href;
  }, [resolvedTheme]);

  return null;
}

export { ThemeProvider };
