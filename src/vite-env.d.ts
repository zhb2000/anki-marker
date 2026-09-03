/// <reference types="vite/client" />

interface ImportMetaEnv {
    /** 应用更新检查的调试场景（可用值见 src/logics/debug.ts） */
    readonly VITE_APP_UPDATE_SCENARIO?: string;
    /** 笔记模板版本获取的调试场景（可用值见 src/logics/debug.ts） */
    readonly VITE_TEMPLATE_VERSION_SCENARIO?: string;
}

declare module "*.vue" {
  import type { DefineComponent } from "vue";
  const component: DefineComponent<{}, {}, any>;
  export default component;
}

/**
 * vite-svg-loader：不带 query 的 .svg 在运行时默认编译为 Vue 组件（defaultImport 未配置，
 * 与下方 vite/client 的 string 类型声明不一致——类型陷阱），显式加 ?component 才是组件、
 * 加 ?url 才是资源 URL（?url 不匹配 loader 正则，回落 Vite 默认资源处理）。
 */
declare module "*.svg?component" {
  import type { FunctionalComponent, SVGAttributes } from "vue";
  const component: FunctionalComponent<SVGAttributes>;
  export default component;
}

declare module "*.svg?url" {
  const src: string;
  export default src;
}
