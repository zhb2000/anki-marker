// 注意：本项目 vite-svg-loader 对不带 query 的 .svg 默认编译为 Vue 组件（见 vite-env.d.ts 说明），
// 图标作为 <img src> 使用，必须带 ?url 强制走 Vite 默认的 URL 资源
import deepseekLogo from '../assets/provider-logos/deepseek.svg?url';
import kimiLogo from '../assets/provider-logos/kimi.svg?url';
import lmstudioLogo from '../assets/provider-logos/lmstudio.svg?url';
import ollamaLogo from '../assets/provider-logos/ollama.svg?url';
import openaiLogo from '../assets/provider-logos/openai.svg?url';
import openrouterLogo from '../assets/provider-logos/openrouter.svg?url';
import qwenLogo from '../assets/provider-logos/qwen.svg?url';
import siliconflowLogo from '../assets/provider-logos/siliconflow.png';
import zhipuLogo from '../assets/provider-logos/zhipu.png';

/**
 * 预设服务商表（弹窗展示用静态数据）。
 *
 * baseUrl 为各服务商官方文档给出的 OpenAI 兼容地址（2026-09 核实）：
 * 客户端会在其后自动拼接 /chat/completions 与 /models（见 llm.ts 的归一化逻辑），
 * 因此这里填写到“域名或版本段”为止的根地址即可。
 * 使用方（预设弹窗）只负责把选中项的 baseUrl 填进输入框，输入框仍是唯一事实来源，
 * 本表不参与持久化。
 */

/** 单个预设服务商 */
export interface LlmProviderPreset {
    /** 稳定标识（用于换源提醒时比较服务商是否变化） */
    id: string;
    /** 展示名 */
    name: string;
    /** 官方 OpenAI 兼容根地址（填入 API 地址输入框的值） */
    baseUrl: string;
    /** 品牌图标（打包后的资源 URL） */
    logo: string;
    /** 深色主题下品牌色近黑、需要反色显示 */
    invertInDark?: boolean;
    /** 开放平台/控制台地址（弹窗中的“获取 Key”链接）；本地服务为官网 */
    homepage?: string;
    /** 弹窗内补充说明（如本地服务的注意事项） */
    note?: string;
}

export const LLM_PROVIDER_PRESETS: LlmProviderPreset[] = [
    {
        id: 'deepseek',
        name: 'DeepSeek',
        baseUrl: 'https://api.deepseek.com',
        logo: deepseekLogo,
        homepage: 'https://platform.deepseek.com',
    },
    {
        id: 'zhipu',
        name: '智谱 GLM',
        baseUrl: 'https://open.bigmodel.cn/api/paas/v4',
        logo: zhipuLogo,
        homepage: 'https://open.bigmodel.cn',
    },
    {
        id: 'kimi',
        name: 'Kimi（月之暗面）',
        baseUrl: 'https://api.moonshot.cn/v1',
        logo: kimiLogo,
        invertInDark: true,
        homepage: 'https://platform.moonshot.cn',
    },
    {
        id: 'qwen',
        name: '通义千问（北京）',
        baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode/v1',
        logo: qwenLogo,
        homepage: 'https://bailian.console.aliyun.com',
    },
    {
        id: 'qwen-intl',
        name: '通义千问（国际版）',
        baseUrl: 'https://dashscope-intl.aliyuncs.com/compatible-mode/v1',
        logo: qwenLogo,
        homepage: 'https://bailian.console.alibabacloud.com',
    },
    {
        id: 'siliconflow',
        name: '硅基流动 SiliconFlow',
        baseUrl: 'https://api.siliconflow.cn/v1',
        logo: siliconflowLogo,
        homepage: 'https://cloud.siliconflow.cn',
    },
    {
        id: 'openrouter',
        name: 'OpenRouter',
        baseUrl: 'https://openrouter.ai/api/v1',
        logo: openrouterLogo,
        homepage: 'https://openrouter.ai/keys',
    },
    {
        id: 'openai',
        name: 'OpenAI',
        baseUrl: 'https://api.openai.com/v1',
        logo: openaiLogo,
        homepage: 'https://platform.openai.com',
    },
    {
        id: 'ollama',
        name: 'Ollama（本机）',
        baseUrl: 'http://localhost:11434/v1',
        logo: ollamaLogo,
        invertInDark: true,
        homepage: 'https://ollama.com',
        note: '本地推理服务，需在本机运行 Ollama 并已拉取模型；API Key 可随意填写',
    },
    {
        id: 'lmstudio',
        name: 'LM Studio（本机）',
        baseUrl: 'http://localhost:1234/v1',
        logo: lmstudioLogo,
        invertInDark: true,
        homepage: 'https://lmstudio.ai',
        note: '本地推理服务，需在 LM Studio 中开启本地服务端；API Key 可随意填写',
    },
];

/** 提取 URL 的 `协议://主机:端口` 作为归一化比较键；解析失败返回 null */
function normalizeOrigin(url: string): string | null {
    try {
        const parsed = new URL(url.trim());
        return `${parsed.protocol}//${parsed.host}`;
    } catch {
        return null;
    }
}

/**
 * 判断用户填写的 API 地址命中哪个预设。
 * 按协议+主机+端口比较（忽略路径差异），因此同一服务商带不带 /v1、自定义路径均能命中；
 * 两家本地服务的端口不同，也不会互相误匹配。未命中（含中转站/空值/非法输入）返回 null。
 */
export function matchProviderPreset(baseUrl: string): LlmProviderPreset | null {
    const origin = normalizeOrigin(baseUrl);
    if (origin == null) {
        return null;
    }
    return LLM_PROVIDER_PRESETS.find(preset => normalizeOrigin(preset.baseUrl) === origin) ?? null;
}
