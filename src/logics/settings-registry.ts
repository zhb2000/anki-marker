/**
 * 设置页的导航与搜索元数据。
 *
 * - SETTINGS_PAGES：左侧导航的页面列表，顺序即导航顺序
 * - SETTING_ENTRIES：可搜索的设置项/动作条目；设置项 id 用 config 键名（与卡片 data-setting-id 一致），
 *   关于/高级页的动作条目用短横线 id
 * - 条目的 title/description 与 SettingsView.vue 中的文案保持一致（新增 theme 项除外）
 */

/** 设置页 id（与 /settings/<id> 路由路径段一致） */
export type SettingsPageId = 'general' | 'selection' | 'anki' | 'ai' | 'advanced' | 'about';

export interface SettingsPageMeta {
    id: SettingsPageId;
    title: string;
    /** Unicode 符号图标，由 shell 统一字号渲染 */
    icon: string;
    /** 仅 macOS 显示的导航项 */
    macOnly?: boolean;
}

/** 左侧导航的页面列表，顺序即导航顺序 */
export const SETTINGS_PAGES: SettingsPageMeta[] = [
    { id: 'general', title: '通用', icon: '⚙' },
    { id: 'selection', title: '划词', icon: '⌨', macOnly: true },
    { id: 'anki', title: 'Anki', icon: '📇' },
    { id: 'ai', title: 'AI 优选', icon: '✨' },
    { id: 'advanced', title: '高级', icon: '🔧' },
    { id: 'about', title: '关于', icon: 'ℹ' },
];

/** 可搜索的设置项/动作条目 */
export interface SettingEntry {
    /** 设置项用 config 键名，动作条目用短横线 id（与卡片 data-setting-id 高亮锚点一致） */
    id: string;
    /** 所属页面 */
    page: SettingsPageId;
    title: string;
    description?: string;
    /** 搜索别名：中文名、口语说法、英文、相关词 */
    keywords: string[];
}

export const SETTING_ENTRIES: SettingEntry[] = [
    // #region 通用
    {
        id: 'theme',
        page: 'general',
        title: '主题',
        description: '跟随系统、浅色或深色',
        keywords: ['主题', '深色', '暗色', '浅色', '外观', 'dark', 'light', 'mode', '模式'],
    },
    {
        id: 'keepRunningOnClose',
        page: 'general',
        title: '关闭窗口后保持后台运行',
        description: '关闭窗口后应用将在后台继续运行，可通过 Dock 图标、菜单栏图标或全局快捷键再次打开',
        keywords: ['后台运行', '保持后台', '关闭窗口', '后台', '常驻', 'background'],
    },
    {
        id: 'backgroundIcon',
        page: 'general',
        title: '后台运行时显示图标',
        description: '选择窗口关闭后（后台运行期间）应用图标的显示位置；窗口打开时图标始终显示在 Dock 栏',
        keywords: ['图标', '后台图标', '显示位置', 'dock', '菜单栏', 'menu bar', '托盘'],
    },
    // #endregion

    // #region 划词（仅 macOS）
    {
        id: 'globalShortcut',
        page: 'selection',
        title: '全局快捷键（录入句子）',
        description: '在任意应用中选中一段文字后按下此快捷键，所选文字将录入划词面板并自动分词。',
        keywords: ['快捷键', '快捷方式', '全局快捷键', '录入句子', 'hotkey', 'shortcut', '划词'],
    },
    // #endregion

    // #region Anki
    {
        id: 'ankiConnectURL',
        page: 'anki',
        title: 'AnkiConnect 服务',
        keywords: ['ankiconnect', 'anki connect', '服务地址', '服务', '地址', 'url', '接口'],
    },
    {
        id: 'deckName',
        page: 'anki',
        title: '将划词结果添加到哪个牌组',
        keywords: ['牌组', '卡组', '划词结果', '添加', 'deck'],
    },
    {
        id: 'modelName',
        page: 'anki',
        title: '使用的笔记模板名称',
        keywords: ['笔记模板', '模板名称', '模板', '单词模板', 'model', 'note type'],
    },
    {
        id: 'autoLaunchAnki',
        page: 'anki',
        title: '自动启动 Anki',
        description: '添加笔记时若 Anki 未运行，将自动启动 Anki 并等待其就绪',
        keywords: ['自动启动', '启动', '拉起', 'anki', 'launch', 'autostart'],
    },
    {
        id: 'launchAnkiOnAppStart',
        page: 'anki',
        title: '应用启动时启动 Anki',
        description: '应用启动时自动启动 Anki，无需等到添加笔记',
        keywords: ['应用启动时启动', '启动时启动', '开机启动', '应用启动', 'anki', 'startup'],
    },
    {
        id: 'ankiExecutablePath',
        page: 'anki',
        title: 'Anki 可执行文件路径',
        description: '留空则自动检测 Anki 路径',
        keywords: ['可执行文件', '路径', '安装路径', 'anki', 'path', 'exe', 'executable'],
    },
    // #endregion

    // #region AI 优选
    {
        id: 'llmEnabled',
        page: 'ai',
        title: '启用 AI 优选释义',
        description: '按句子语境从多本词典中优选释义，需自行配置 LLM API',
        keywords: ['ai', 'llm', '优选释义', '优选', '释义', '启用', '大模型', '人工智能'],
    },
    {
        id: 'llmBaseUrl',
        page: 'ai',
        title: 'API 地址',
        keywords: ['api', '地址', '接口地址', 'base url', 'url', 'deepseek', 'llm'],
    },
    {
        id: 'llmApiKey',
        page: 'ai',
        title: 'API Key',
        keywords: ['api key', 'key', '密钥', '令牌', 'token', 'apikey'],
    },
    {
        id: 'llmModel',
        page: 'ai',
        title: '模型',
        keywords: ['模型', '大模型', 'model', 'deepseek', 'llm'],
    },
    {
        id: 'llmMaxTokens',
        page: 'ai',
        title: '最大生成 Token 数',
        description: '单次请求的生成上限；思考模型的思维链计入此配额，释义被截断或为空时可调大',
        keywords: ['最大', 'token', 'tokens', 'max tokens', '生成上限', '生成', '截断', '思维链', '思考', 'max_tokens'],
    },
    {
        id: 'llmReasoningEffort',
        page: 'ai',
        title: '思考强度',
        description: '常见取值 low / medium / high，实际支持因所配服务而异；留空则不传参',
        keywords: ['思考强度', '推理强度', '思考', 'reasoning', 'effort', 'low', 'medium', 'high'],
    },
    // #endregion

    // #region 高级（配置文件）
    {
        id: 'portable-mode',
        page: 'advanced',
        title: '安装/便携模式',
        keywords: ['便携模式', '安装模式', '模式', 'portable', '绿色版'],
    },
    {
        id: 'config-path',
        page: 'advanced',
        title: '配置文件路径',
        keywords: ['配置文件', '配置文件路径', '路径', '设置文件', 'config'],
    },
    {
        id: 'open-config-file',
        page: 'advanced',
        title: '打开文件',
        keywords: ['打开文件', '打开配置文件', '配置文件', '编辑配置', 'open', 'config'],
    },
    {
        id: 'open-config-dir',
        page: 'advanced',
        title: '打开目录',
        keywords: ['打开目录', '打开文件夹', '所在目录', '目录', 'open', 'folder'],
    },
    // #endregion

    // #region 关于
    {
        id: 'check-update',
        page: 'about',
        title: '检查更新',
        keywords: ['检查更新', '更新', '版本', '应用版本', '新版本', '升级', 'update'],
    },
    {
        id: 'update-template',
        page: 'about',
        title: 'Anki 内笔记模板版本',
        keywords: ['更新模板', '模板', '笔记模板', '模板版本', '刷新', 'template'],
    },
    {
        id: 'author',
        page: 'about',
        title: '作者',
        keywords: ['作者', '开发者', 'author', 'zhb'],
    },
    {
        id: 'project-url',
        page: 'about',
        title: '项目地址',
        keywords: ['项目地址', '项目', '开源', '仓库', '源码', 'github'],
    },
    // #endregion
];

/** 搜索设置条目：trim 后大小写不敏感地子串匹配 title/description/keywords，空 query 返回 [] */
export function searchSettings(query: string): SettingEntry[] {
    const q = query.trim().toLowerCase();
    if (q.length === 0) {
        return [];
    }
    return SETTING_ENTRIES.filter(entry =>
        entry.title.toLowerCase().includes(q) ||
        (entry.description?.toLowerCase().includes(q) ?? false) ||
        entry.keywords.some(keyword => keyword.toLowerCase().includes(q))
    );
}
