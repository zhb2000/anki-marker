/**
 * 设置页文本输入框的“编辑生命周期”绑定器。
 *
 * 职责边界（与基础控件解耦）：
 * - FluentInput 等基础控件只负责如实上报输入与焦点事件，不感知“提交”概念
 * - 本绑定器（设置业务层）持有编辑中的草稿：聚焦开始编辑，期间键入只改草稿、不写入 store，
 *   blur / Enter 时 trim 并提交到 store（提交语义属于业务层，故收敛于此）
 * - 提交后由 store 既有链路（watch → 防抖 commit）自动落盘
 *
 * 使用方式（页面 setup 中创建一次，模板中逐键绑定）：
 * ```vue
 * const { bind } = createSettingInputBinder(store);
 * <FluentInput v-bind="bind('ankiConnectURL')" />
 * ```
 *
 * 实现说明：
 * - 草稿是响应式的单槽结构（同一时刻至多一个输入框处于编辑态）。
 *   modelValue 恒等于“应当显示的值”——编辑中取草稿、非编辑态取 store——
 *   即完全受控。这点至关重要：HoverWrapper 会克隆子节点并在 hover 变化时重渲染，
 *   槽内容可能以旧的 prop 重新 patch 控件；若绑定值不是响应式新鲜的，
 *   defineModel 的本地值/prop 协调会把输入框可见文本回退到旧 prop（表现为“输入中被清空”）。
 *   受控值始终保持新鲜后，任何来源的重渲染都不会再回退输入内容。
 * - 提交只发生在编辑结束（blur / Enter / 离开页面），半成品输入永不落盘。
 */

import { onActivated, onDeactivated, ref } from 'vue';

import type { ConfigModel } from './config';
import type { SettingsStore } from './settings-store';

/**
 * 值为纯字符串的配置项键（只有这些键可用于文本输入绑定器）。
 * `string extends ConfigModel[K]` 用于排除字面量联合键（theme/backgroundIcon）：
 * 它们虽可赋给 string，但写入类型并非宽泛的 string。
 */
export type TextSettingKey = {
    [K in keyof ConfigModel]: string extends ConfigModel[K] ? K : never;
}[keyof ConfigModel];

/** 传给 FluentInput / FluentPasswordInput 的 v-bind 绑定对象 */
export interface SettingInputBindings {
    modelValue: string;
    'onUpdate:modelValue': (value: string | undefined) => void;
    onFocus: (event: FocusEvent) => void;
    onBlur: (event: FocusEvent) => void;
    onKeydown: (event: KeyboardEvent) => void;
}

/** 创建绑定到指定设置仓库的文本输入绑定器（每个页面 setup 中调用一次） */
export function createSettingInputBinder(store: SettingsStore) {
    /** 当前编辑中的输入框：键与草稿值；null 表示没有输入框处于编辑态 */
    const editing = ref<{ key: TextSettingKey; value: string } | null>(null);

    /** 结束编辑：trim 后提交到 store（空值合法，“留空 = 交给程序”语义由消费端落地） */
    function commitEditing(): void {
        if (editing.value == null) {
            return;
        }
        store.state[editing.value.key] = editing.value.value.trim();
        editing.value = null;
    }

    /**
     * 生成指定设置项的控件绑定对象。每次渲染调用，受控值取当前草稿（编辑中）
     * 或 store 的值（非编辑态）。
     */
    function bind(key: TextSettingKey): SettingInputBindings {
        return {
            modelValue: editing.value?.key === key ? editing.value.value : store.state[key],
            'onUpdate:modelValue': value => {
                // 文本输入框不会产生 undefined/null（defineModel 的事件类型放宽所致），防御性忽略
                if (value == null) {
                    return;
                }
                if (editing.value == null) {
                    // 兜底：极少数情况下 focus 事件缺失时，首个键入也能开启编辑
                    editing.value = { key, value };
                } else if (editing.value.key === key) {
                    editing.value.value = value;
                }
            },
            onFocus: () => {
                if (editing.value == null) {
                    editing.value = { key, value: store.state[key] };
                }
            },
            onBlur: () => {
                if (editing.value?.key === key) {
                    commitEditing();
                }
            },
            onKeydown: event => {
                // 输入法组合期间的 Enter 是选词确认，不是提交
                if (event.isComposing || editing.value?.key !== key) {
                    return;
                }
                if (event.key === 'Enter') {
                    commitEditing();
                    (event.target as HTMLInputElement).blur();
                }
            },
        };
    }

    // KeepAlive 边界：离开设置页时提交未落盘的草稿（如程序化导航未触发 blur）；
    // 重新进入时清空遗留草稿，防止遮蔽 syncFromConfig 拉入的外部修改
    onDeactivated(() => commitEditing());
    onActivated(() => {
        editing.value = null;
    });

    return { bind };
}
