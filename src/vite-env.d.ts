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

/** vite-svg-loader：带 ?component 查询的 SVG 导入为 Vue 组件（默认 *.svg 仍是 string） */
declare module "*.svg?component" {
  import type { FunctionalComponent, SVGAttributes } from "vue";
  const component: FunctionalComponent<SVGAttributes>;
  export default component;
}
