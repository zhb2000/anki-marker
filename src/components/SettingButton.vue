<script setup lang="ts">
import { computed } from 'vue';
import { ElBadge } from 'element-plus';

import { useHover } from '../fluent-controls';

const props = defineProps({
    updateAvailable: {
        type: Boolean,
        required: true
    }
});

const hover = useHover();
const tooltipTitle = computed(() => props.updateAvailable ? '设置（更新可用）' : '设置');
</script>

<template>
    <ElBadge is-dot :offset="[-2, 2]" :hidden="!props.updateAvailable">
        <button class="setting-button" :class="hover.classes.value" :title="tooltipTitle" v-on="hover.listeners">
            <slot></slot>
        </button>
    </ElBadge>
</template>

<style scoped>
.setting-button {
    height: 32px;
    width: 32px;
    margin: 0;
    padding: 0;
    background-image: url('../assets/settings.svg');
    /* background-size: cover; */
    background-repeat: no-repeat;
    background-position: center center;
    background-color: var(--window-background);
    background-size: 60% 60%;
    border: none;
    border-radius: var(--border-radius);
}

.setting-button.hover {
    filter: brightness(90%);
}

.setting-button:active {
    filter: brightness(80%);
}
</style>
