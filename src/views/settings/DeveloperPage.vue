<script setup lang="ts">
import { computed } from 'vue';
import { useRouter } from 'vue-router';

import { currentDevEnvironment } from '../../dev/environment';
import {
    DEV_GROUP_TITLES, DEV_SETTING_DEFS, DEV_SETTING_KEYS, coerceDevSetting,
    type DevSettingGroup, type DevSettingKey,
} from '../../dev/schema';
import { devOverridden, devSettings } from '../../dev/state';
import { devMockStatus } from '../../dev/status';
import { resetAllDevSettings, resetDevSetting, setDevSetting } from '../../dev/store';
import { FluentButton, FluentInput, FluentSelect, FluentSettingCard, FluentToggleSwitch, dialog } from '../../fluent-controls';
import { ResetButton } from '../../components';

/**
 * 开发者设置页：只在 dev 功能总开关（VITE_DEV_FEATURES）打开时存在。
 * 页面完全由 src/dev/schema.ts 的注册表生成；修改写入项目根目录的 dev-settings.local.json，
 * 并立即生效。取值与 URL 参数/`window.__dev` 的关系见 docs/dev-features.md。
 */

const groups = (Object.keys(DEV_GROUP_TITLES) as DevSettingGroup[]).map(group => ({
    group,
    title: DEV_GROUP_TITLES[group],
    keys: DEV_SETTING_KEYS.filter(key => DEV_SETTING_DEFS[key].group === group),
}));

const router = useRouter();

/** 当前运行环境；页面生命周期内不变 */
const environment = currentDevEnvironment();

/** 当前环境 + 当前设置下的完整状态：哪些生效、哪些无效、哪些模拟正在跑 */
const status = computed(() => devMockStatus(devSettings, environment));

/** 有已修改（偏离默认值）的开发者设置时才允许“全部恢复” */
const modifiedCount = computed(() => DEV_SETTING_KEYS.filter(isModified).length);

function isModified(key: DevSettingKey): boolean {
    return devSettings[key] !== DEV_SETTING_DEFS[key].default;
}

function optionLabel(key: DevSettingKey, value: unknown): string {
    const def = DEV_SETTING_DEFS[key];
    if (def.type === 'enum') {
        return def.options.find(option => option.value === value)?.label ?? String(value);
    }
    if (def.type === 'boolean') {
        return value === true ? '开启' : '关闭';
    }
    return String(value);
}

function describe(key: DevSettingKey): string {
    const parts = [DEV_SETTING_DEFS[key].description];
    if (devOverridden[key]) {
        parts.push(`当前被 URL 参数或控制台临时覆盖为“${optionLabel(key, devSettings[key])}”，刷新页面后失效；在此修改会解除覆盖`);
    }
    const item = status.value.settings[key];
    if (item.effectiveValue != null) {
        parts.push(`当前有效值：${item.effectiveValue}`);
    }
    if (item.disabledReason != null) {
        parts.push(item.disabledReason);
    }
    if (item.warning != null) {
        parts.push(item.warning);
    }
    if (key === 'llmScenario' && status.value.effective.llmScenario !== 'real' && !devSettings.mockLlmConfig) {
        parts.push('若 AI 尚未启用或未配置，查询结果里不会出现 AI 卡；打开“免 Key 预览 AI”即可零配置预览');
    }
    return parts.join('。');
}

/** 卡片是否需要用警示色说明：禁用原因用弱化说明，只有值无效/被临时覆盖才使用 WinUI warning 色 */
function isWarning(key: DevSettingKey): boolean {
    const item = status.value.settings[key];
    return devOverridden[key] === true || item.warning != null;
}

/** 当前模式下这一项是否可用 */
function isEnabled(key: DevSettingKey): boolean {
    return status.value.settings[key].enabled;
}

function resetTooltip(key: DevSettingKey): string {
    const def = DEV_SETTING_DEFS[key];
    return `重置为默认值：${def.type === 'boolean' ? (def.default ? '开启' : '关闭') : optionLabel(key, def.default)}`;
}

function onChange<K extends DevSettingKey>(key: K, value: unknown): void {
    const valid = coerceDevSetting(key, value);
    if (valid !== undefined) {
        setDevSetting(key, valid);
    }
}

/** 恢复全部开发者设置：确认后写回默认值并清除临时覆盖 */
async function handleResetAllClick() {
    // 受影响的项 = 与默认值不同的项 ∪ 被临时覆盖的项（两者常常是同一批，不能相加）
    const affected = DEV_SETTING_KEYS.filter(key => isModified(key) || devOverridden[key]).length;
    const confirmed = await dialog.confirm(
        `将把 ${affected} 项开发者设置恢复为默认值，并清除 URL 参数或控制台的临时覆盖。立即生效，不可撤销。`,
        {
            title: '恢复开发者设置',
            kind: 'warning',
            okLabel: '恢复默认',
            cancelLabel: '取消',
        },
    );
    if (confirmed) {
        resetAllDevSettings();
    }
}

function onNumberChange(key: DevSettingKey, text: string): void {
    onChange(key, text.trim() === '' ? undefined : Number(text));
}

/** 清空 AI 结果缓存与参数兼容缓存，让下一次请求按当前场景重新走 */
function handleResetMock(): void {
    window.__dev?.resetMock?.();
}
</script>

<template>
    <div class="settings-page">
        <div v-if="status.mocks.length > 0" class="mock-banner">
            <div class="mock-banner-title">当前有 {{ status.mocks.length }} 项模拟生效</div>
            <ul class="mock-list">
                <li v-for="item in status.mocks" :key="item.key" class="mock-item">
                    <span class="mock-name">{{ item.title }}</span>
                    <span class="mock-value">{{ item.value }}</span>
                    <span class="mock-detail">{{ item.detail }}</span>
                </li>
            </ul>
            <div v-if="status.warnings.length > 0" class="mock-warning">
                另有 {{ status.warnings.length }} 项设置需要注意，见下方说明文字为警示色的卡片。
            </div>
        </div>
        <div v-else class="mock-banner mock-banner-quiet">
            <div class="mock-banner-title">当前没有模拟生效</div>
            <div class="mock-detail">所有请求按真实实现走；仍可通过下方设置开启场景。</div>
        </div>

        <div class="card-list">
            <FluentSettingCard header="运行环境" :description="status.environmentDescription">
                <span class="value-text">{{ status.environmentLabel }}</span>
            </FluentSettingCard>
            <FluentSettingCard header="设置文件" description="开发者设置保存在项目根目录，被 gitignore；也可直接编辑，外部修改会实时同步">
                <span class="value-text">dev-settings.local.json</span>
            </FluentSettingCard>
            <FluentSettingCard header="卡片模板预览"
                description="并排查看笔记模板的正面与背面，可切样本、宽度与夜间模式；改模板文件保存即刷新">
                <FluentButton @click="router.push('/dev/card')">打开预览</FluentButton>
            </FluentSettingCard>
        </div>

        <template v-for="section in groups" :key="section.group">
            <h2 class="group-title">{{ section.title }}</h2>
            <div class="card-list">
                <FluentSettingCard v-for="key in section.keys" :key="key" :header="DEV_SETTING_DEFS[key].title"
                    :description="describe(key)" :description-type="isWarning(key) ? 'warning' : 'normal'"
                    :disabled="!isEnabled(key)">
                    <template #header-extra>
                        <ResetButton :modified="isModified(key)" :tooltip="resetTooltip(key)"
                            @reset="resetDevSetting(key)" />
                    </template>
                    <FluentToggleSwitch v-if="DEV_SETTING_DEFS[key].type === 'boolean'"
                        :model-value="devSettings[key] as boolean" :disabled="!isEnabled(key)"
                        @update:model-value="onChange(key, $event)" />
                    <FluentSelect v-else-if="DEV_SETTING_DEFS[key].type === 'enum'"
                        :options="(DEV_SETTING_DEFS[key] as any).options"
                        :model-value="devSettings[key] as string" :disabled="!isEnabled(key)"
                        @update:model-value="onChange(key, $event)" />
                    <template v-else>
                        <FluentInput class="number-input" no-auto-correct
                            :model-value="String(devSettings[key])" :disabled="!isEnabled(key)"
                            @update:model-value="onNumberChange(key, String($event))" />
                        <span class="value-text">{{ (DEV_SETTING_DEFS[key] as any).unit }}</span>
                    </template>
                </FluentSettingCard>
            </div>
        </template>

        <h2 class="group-title">重置</h2>
        <div class="card-list">
            <FluentSettingCard header="恢复全部开发者设置"
                :description="modifiedCount > 0 ? `当前有 ${modifiedCount} 项与默认值不同；同时清除临时覆盖` : '所有开发者设置均为默认值'">
                <FluentButton :disabled="modifiedCount === 0 && Object.keys(devOverridden).length === 0"
                    @click="handleResetAllClick">恢复默认</FluentButton>
            </FluentSettingCard>
            <FluentSettingCard header="重置模拟状态"
                description="清空 AI 结果缓存与参数兼容缓存；下一次请求会按当前场景重新走（切换场景时也会自动清空）">
                <FluentButton @click="handleResetMock">重置模拟状态</FluentButton>
            </FluentSettingCard>
        </div>
    </div>
</template>

<style scoped>
.group-title {
    margin: 24px 0 6px 2px;
    padding: 0;
    font-size: 14px;
    font-weight: normal;
    opacity: 0.6;
    user-select: none;
    cursor: default;
}

.card-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
}

.card-list:first-child {
    margin-top: 0;
}

.value-text {
    user-select: none;
    cursor: default;
}

/* 当前模拟状态横幅：有模拟时用警示色，避免人类/AI 忘记自己在夹具里 */
.mock-banner {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 12px 16px;
    margin-bottom: 4px;
    border: 1px solid var(--warning-text-color);
    border-radius: 8px;
    background: color-mix(in srgb, var(--warning-text-color) 8%, var(--control-background));
    color: var(--control-text-color);
    user-select: none;
    cursor: default;
}

.mock-banner-quiet {
    border-color: var(--border-color);
    background: var(--control-background);
}

.mock-banner-title {
    font-size: 13px;
    font-weight: 600;
    color: var(--warning-text-color);
}

.mock-banner-quiet .mock-banner-title {
    color: var(--control-text-color);
    opacity: 0.7;
}

.mock-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin: 0;
    padding: 0;
    list-style: none;
}

.mock-item {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 6px;
    font-size: 12px;
}

.mock-name {
    font-weight: 600;
}

.mock-value {
    color: var(--warning-text-color);
}

.mock-detail {
    color: color-mix(in srgb, currentColor 60%, transparent);
}

.mock-warning {
    font-size: 12px;
    color: var(--warning-text-color);
}

.number-input {
    width: 100px;
    margin-right: 8px;
}
</style>
