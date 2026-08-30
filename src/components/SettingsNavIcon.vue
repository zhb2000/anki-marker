<script setup lang="ts">
import type { Component } from 'vue';

import iconGeneral from '../assets/settings-icons/general.svg?component';
import iconSelection from '../assets/settings-icons/selection.svg?component';
import iconAnki from '../assets/settings-icons/anki.svg?component';
import iconAI from '../assets/settings-icons/ai.svg?component';
import iconAdvanced from '../assets/settings-icons/advanced.svg?component';
import iconAbout from '../assets/settings-icons/about.svg?component';
import type { SettingsNavIconName } from '../logics/settings-registry';

/** 设置导航页 → SVG 图标组件（vite-svg-loader 的 ?component 导入） */
const icons: Record<SettingsNavIconName, Component> = {
    general: iconGeneral,
    selection: iconSelection,
    anki: iconAnki,
    ai: iconAI,
    advanced: iconAdvanced,
    about: iconAbout,
};

defineProps<{ name: SettingsNavIconName }>();
</script>

<template>
    <component :is="icons[name]" class="settings-nav-svg" />
</template>

<style scoped>
.settings-nav-svg {
    width: 16px;
    height: 16px;
    display: block;
}
</style>
