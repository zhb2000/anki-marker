<script setup lang="ts">
import { onDeactivated, onActivated, ref, watch, nextTick } from 'vue';

const props = defineProps({
    /** 控制组件的 `v-show` 属性 */
    show: {
        type: Boolean,
        default: true,
    },
    /** 容器元素使用的标签名 */
    tag: {
        type: String,
        default: 'div',
    }
});

const container = ref<HTMLElement>(null as any);
let savedScrollTop: number | null = null;

onDeactivated(() => {
    // display 属性为 none 时 scrollTop 总是为 0，不要保存
    if (container.value.style.display !== 'none' && savedScrollTop === null) {
        savedScrollTop = container.value.scrollTop;
    }
});

onActivated(() => {
    if (container.value.style.display !== 'none' && savedScrollTop !== null) {
        container.value.scrollTop = savedScrollTop;
        savedScrollTop = null;
    }
});

watch(() => props.show, async () => {
    if (props.show) {
        if (savedScrollTop !== null) {
            // watch 的回调被调用时，DOM 元素的 display 属性还是 none，
            // 需要等待 display 属性变为非 none 之后再设置 scrollTop 才有效。
            await nextTick();
            container.value.scrollTop = savedScrollTop;
            savedScrollTop = null;
        }
    } else {
        // 在 DOM 元素的 display 属性变为 none 之前保存 scrollTop
        if (savedScrollTop === null) {
            savedScrollTop = container.value.scrollTop;
        }
    }
});
</script>

<template>
    <component v-show="show" :is="props.tag" ref="container">
        <slot></slot>
    </component>
</template>
