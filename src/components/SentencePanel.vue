<script setup lang="ts">
import { PropType } from 'vue';

import TokenItem from "./TokenItem.vue";

defineProps({
    tokens: {
        type: Object as PropType<{
            token: string;
            marked: boolean;
        }[]>,
        required: true,
    }
});

</script>

<template>
    <div class="sentence-panel">
        <TokenItem v-for="(token, index) in tokens" :key="index" :token="token.token" v-model:marked="token.marked" />
    </div>
</template>

<style scoped>
.sentence-panel {
    width: 100%;
    height: 100%;
    padding: 8px 12px;
    user-select: none;
    cursor: default;
    background-color: var(--input-text-background-focus);
    border-style: solid;
    border-color: var(--border-color);
    border-bottom-color: var(--border-bottom-color);
    border-width: var(--border-width);
    border-radius: var(--border-radius);
    overflow-y: auto;
    overflow-wrap: break-word;
}
</style>
