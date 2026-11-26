"use client";

import { create } from "zustand";
import { persist } from "zustand/middleware";

export type WallpaperPresetId =
  | "optics"
  | "aurora"
  | "sequoia"
  | "ocean"
  | "zen-mist"
  | "cyber"
  | "photo-mountain"
  | "photo-dunes"
  | "photo-nebula"
  | "custom"
  | "none";

export interface WallpaperPreset {
  id: WallpaperPresetId;
  name: string;
  category: "artistic" | "photography" | "minimal";
  description: string;
  previewGradient: string;
  darkCss?: string;
  lightCss?: string;
  imageUrl?: string;
}

export const WALLPAPER_PRESETS: WallpaperPreset[] = [
  {
    id: "optics",
    name: "光晕透镜",
    category: "artistic",
    description: "暖阳金芒与碧空流光，呈现最丰富的双色温光学折射质感，长时间阅读舒适自然",
    previewGradient: "linear-gradient(135deg, #0284c7 0%, #eab308 50%, #f43f5e 100%)",
    darkCss:
      "radial-gradient(circle at 72% 38%, rgba(245, 158, 11, 0.28) 0%, rgba(217, 119, 6, 0.12) 35%, transparent 60%), radial-gradient(circle at 18% 68%, rgba(14, 165, 233, 0.22) 0%, rgba(16, 185, 129, 0.12) 40%, transparent 65%), radial-gradient(circle at 85% 15%, rgba(244, 63, 94, 0.15) 0%, transparent 45%), linear-gradient(180deg, #070a14 0%, #0d1326 50%, #080c1a 100%)",
    lightCss:
      "radial-gradient(circle at 72% 38%, rgba(251, 191, 36, 0.38) 0%, rgba(245, 158, 11, 0.18) 35%, transparent 60%), radial-gradient(circle at 18% 68%, rgba(56, 189, 248, 0.32) 0%, rgba(52, 211, 153, 0.22) 40%, transparent 65%), radial-gradient(circle at 85% 15%, rgba(251, 113, 133, 0.22) 0%, transparent 45%), linear-gradient(180deg, #f0fdfa 0%, #f8fafc 50%, #fff7ed 100%)",
  },
  {
    id: "aurora",
    name: "深空极光",
    category: "artistic",
    description: "深邃暗夜靛蓝与翡翠流光，夜间阅读温和护眼，玻璃折射感极佳",
    previewGradient: "linear-gradient(135deg, #090d16 0%, #064e3b 50%, #1e1b4b 100%)",
    darkCss:
      "radial-gradient(circle at 15% 20%, rgba(16, 185, 129, 0.2) 0%, transparent 45%), radial-gradient(circle at 85% 75%, rgba(99, 102, 241, 0.2) 0%, transparent 50%), radial-gradient(circle at 50% 50%, rgba(14, 165, 233, 0.14) 0%, transparent 65%), linear-gradient(180deg, #070a13 0%, #0c1222 50%, #090e1a 100%)",
    lightCss:
      "radial-gradient(circle at 15% 20%, rgba(52, 211, 153, 0.18) 0%, transparent 45%), radial-gradient(circle at 85% 75%, rgba(129, 140, 248, 0.18) 0%, transparent 50%), linear-gradient(180deg, #f0fdf4 0%, #f8fafc 50%, #eef2ff 100%)",
  },
  {
    id: "sequoia",
    name: "晨曦红杉",
    category: "artistic",
    description: "macOS Sequoia 标志性自然有机山峦晨曦，层次分明高雅质感",
    previewGradient: "linear-gradient(135deg, #1c1917 0%, #9a3412 50%, #0c0a09 100%)",
    darkCss:
      "radial-gradient(circle at 20% 15%, rgba(249, 115, 22, 0.18) 0%, transparent 45%), radial-gradient(circle at 80% 80%, rgba(225, 29, 72, 0.15) 0%, transparent 55%), radial-gradient(circle at 50% 60%, rgba(120, 53, 15, 0.18) 0%, transparent 60%), linear-gradient(180deg, #0c0a09 0%, #171412 50%, #0c0a09 100%)",
    lightCss:
      "radial-gradient(circle at 20% 15%, rgba(251, 146, 60, 0.18) 0%, transparent 45%), radial-gradient(circle at 80% 80%, rgba(251, 113, 133, 0.15) 0%, transparent 55%), linear-gradient(180deg, #fff7ed 0%, #fafaf9 50%, #fdf2f8 100%)",
  },
  {
    id: "ocean",
    name: "琉璃深海",
    category: "artistic",
    description: "科技感深海蔚蓝与微光水波，如水下液态透镜般的清澈通透体验",
    previewGradient: "linear-gradient(135deg, #022c43 0%, #055170 50%, #021824 100%)",
    darkCss:
      "radial-gradient(circle at 25% 25%, rgba(6, 182, 212, 0.2) 0%, transparent 45%), radial-gradient(circle at 75% 75%, rgba(2, 132, 199, 0.18) 0%, transparent 50%), linear-gradient(180deg, #02121e 0%, #052136 50%, #02121e 100%)",
    lightCss:
      "radial-gradient(circle at 25% 25%, rgba(34, 211, 238, 0.16) 0%, transparent 45%), radial-gradient(circle at 75% 75%, rgba(56, 189, 248, 0.16) 0%, transparent 50%), linear-gradient(180deg, #ecfeff 0%, #f0f9ff 50%, #f8fafc 100%)",
  },
  {
    id: "zen-mist",
    name: "极简烟岚",
    category: "minimal",
    description: "超低对比度柔和烟岚石墨，极致减少视觉干扰，长时间阅读首选",
    previewGradient: "linear-gradient(135deg, #1e293b 0%, #334155 50%, #0f172a 100%)",
    darkCss:
      "radial-gradient(circle at 50% 30%, rgba(148, 163, 184, 0.1) 0%, transparent 60%), linear-gradient(180deg, #0b0f17 0%, #111827 50%, #0b0f17 100%)",
    lightCss:
      "radial-gradient(circle at 50% 30%, rgba(203, 213, 225, 0.35) 0%, transparent 60%), linear-gradient(180deg, #f8fafc 0%, #f1f5f9 50%, #f8fafc 100%)",
  },
  {
    id: "cyber",
    name: "赛博棱镜",
    category: "artistic",
    description: "流光溢彩的光学折射三棱镜光环，最大化展现物理液态玻璃色散",
    previewGradient: "linear-gradient(135deg, #180d2b 0%, #4c1d95 50%, #0f172a 100%)",
    darkCss:
      "radial-gradient(circle at 20% 30%, rgba(168, 85, 247, 0.2) 0%, transparent 45%), radial-gradient(circle at 80% 40%, rgba(236, 72, 153, 0.18) 0%, transparent 45%), radial-gradient(circle at 50% 85%, rgba(59, 130, 246, 0.18) 0%, transparent 50%), linear-gradient(180deg, #0c0817 0%, #130f24 50%, #0c0817 100%)",
    lightCss:
      "radial-gradient(circle at 20% 30%, rgba(192, 132, 252, 0.16) 0%, transparent 45%), radial-gradient(circle at 80% 40%, rgba(244, 114, 182, 0.16) 0%, transparent 45%), linear-gradient(180deg, #faf5ff 0%, #fdf2f8 50%, #f8fafc 100%)",
  },
  {
    id: "photo-mountain",
    name: "雪山晨雾",
    category: "photography",
    description: "4K 柔和雪山与轻雾，极度开阔宁静，长时间阅读减压护眼",
    previewGradient: "linear-gradient(135deg, #334d5c 0%, #45b29d 100%)",
    imageUrl:
      "https://images.unsplash.com/photo-1506744038136-46273834b3fb?auto=format&fit=crop&w=2560&q=80",
  },
  {
    id: "photo-dunes",
    name: "金色沙丘",
    category: "photography",
    description: "极简自然曲线沙丘，温暖柔润光影，玻璃透视质感绝美",
    previewGradient: "linear-gradient(135deg, #b07d62 0%, #e0a96d 100%)",
    imageUrl:
      "https://images.unsplash.com/photo-1509316975850-ff9c5deb0cd9?auto=format&fit=crop&w=2560&q=80",
  },
  {
    id: "photo-nebula",
    name: "深邃星云",
    category: "photography",
    description: "遥远深空深邃星系微光，沉浸感极强，暗色模式最佳伴侣",
    previewGradient: "linear-gradient(135deg, #0f2027 0%, #203a43 50%, #2c5364 100%)",
    imageUrl:
      "https://images.unsplash.com/photo-1518709268805-4e9042af9f23?auto=format&fit=crop&w=2560&q=80",
  },
  {
    id: "custom",
    name: "自定义壁纸",
    category: "minimal",
    description: "输入任意网络图片链接或本地图片地址",
    previewGradient: "linear-gradient(135deg, #6366f1 0%, #a855f7 100%)",
  },
  {
    id: "none",
    name: "原生极简",
    category: "minimal",
    description: "无背景图案，回归经典的低调纯净控制台质感",
    previewGradient: "linear-gradient(135deg, #18181b 0%, #27272a 100%)",
    darkCss: "none",
    lightCss: "none",
  },
];

interface WallpaperState {
  currentPresetId: WallpaperPresetId;
  customImageUrl: string;
  blurLevel: "low" | "medium" | "high";
  chromaticAberration: boolean;
  mouseLighting: boolean;
  setPreset: (id: WallpaperPresetId) => void;
  setCustomImageUrl: (url: string) => void;
  setBlurLevel: (level: "low" | "medium" | "high") => void;
  setChromaticAberration: (enabled: boolean) => void;
  setMouseLighting: (enabled: boolean) => void;
}

export const useWallpaperStore = create<WallpaperState>()(
  persist(
    (set) => ({
      currentPresetId: "optics",
      customImageUrl: "",
      blurLevel: "medium",
      chromaticAberration: true,
      mouseLighting: true,
      setPreset: (id) => set({ currentPresetId: id }),
      setCustomImageUrl: (url) => set({ customImageUrl: url }),
      setBlurLevel: (level) => set({ blurLevel: level }),
      setChromaticAberration: (chromaticAberration) => set({ chromaticAberration }),
      setMouseLighting: (mouseLighting) => set({ mouseLighting }),
    }),
    {
      name: "oxidns:wallpaper-settings",
    },
  ),
);
