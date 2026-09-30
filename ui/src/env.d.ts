/// <reference types="vite/client" />

declare module "*.vue" {
  import type { DefineComponent } from "vue";
  const component: DefineComponent<{}, {}, any>;
  export default component;
}

declare global {
  interface Window {
    DotWallpaperNative?: {
      invoke(action: string, params?: Record<string, unknown>): void;
    };
  }
}

export {};
