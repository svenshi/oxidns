"use client";

import { useRouter } from "next/navigation";
import { Code2, Gauge, SlidersHorizontal } from "lucide-react";
import { AppHeader } from "@/components/shell/app-header";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { useAppStore } from "@/lib/store";

export function ModeSelection() {
  const router = useRouter();
  const setWebUiMode = useAppStore((s) => s.setWebUiMode);

  return (
    <div className="flex h-full min-h-0 flex-col">
      <AppHeader title="选择工作模式" />
      <main className="flex min-h-0 flex-1 items-center justify-center overflow-auto p-6">
        <div className="w-full max-w-5xl space-y-6">
          <div className="space-y-2">
            <h1 className="text-2xl font-semibold tracking-tight">
              选择 OxiDNS WebUI 工作模式
            </h1>
            <p className="max-w-3xl text-sm text-muted-foreground">
              现有配置默认按专家模式打开。模式偏好会保存到后端 WebUI
              状态文件，标准模式保存时仍会生成完整 YAML 配置。
            </p>
          </div>
          <div className="grid gap-4 md:grid-cols-2">
            <Card className="border-primary/40 bg-primary/5">
              <CardHeader className="space-y-3">
                <div className="flex size-10 items-center justify-center rounded-md bg-primary text-primary-foreground">
                  <Code2 className="size-5" />
                </div>
                <CardTitle>专家模式</CardTitle>
              </CardHeader>
              <CardContent className="space-y-5">
                <p className="text-sm leading-6 text-muted-foreground">
                  保留现有 Dashboard、插件中心、日志、系统配置、YAML
                  编辑器和配置历史，适合直接管理完整插件编排。
                </p>
                <Button
                  className="w-full"
                  onClick={() => {
                    setWebUiMode("expert", { dismissSelection: true });
                    router.push("/");
                  }}
                >
                  进入专家模式
                </Button>
              </CardContent>
            </Card>
            <Card>
              <CardHeader className="space-y-3">
                <div className="flex size-10 items-center justify-center rounded-md bg-muted text-foreground">
                  <SlidersHorizontal className="size-5" />
                </div>
                <CardTitle>标准模式</CardTitle>
              </CardHeader>
              <CardContent className="space-y-5">
                <p className="text-sm leading-6 text-muted-foreground">
                  使用开关和表单配置 DNS，上手体验接近家庭网关类 DNS
                  管理面板，仍可随时切回专家模式。
                </p>
                <Button
                  variant="secondary"
                  className="w-full"
                  onClick={() => {
                    setWebUiMode("standard", { dismissSelection: true });
                    router.push("/standard");
                  }}
                >
                  <Gauge className="size-4" />
                  进入标准模式
                </Button>
              </CardContent>
            </Card>
          </div>
        </div>
      </main>
    </div>
  );
}
