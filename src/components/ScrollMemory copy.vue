<script setup lang="ts">
import { onDeactivated, onActivated, ref, onMounted, onBeforeUnmount } from 'vue';

const props = defineProps({
    show: {
        type: Boolean,
        default: true,
    },
    tag: {
        type: String,
        default: 'div',
    }
});

const container = ref<HTMLElement>(null as any);
let observer: MutationObserver;
let savedScrollTop: number | null = null;

onMounted(() => {
    observer = new MutationObserver(mutations => {
        for (const mutation of mutations) {
            if (mutation.attributeName === 'style') {
                const display = getComputedStyle(container.value).display;
                if (display !== 'none') {
                    console.log('组件显示了，', 'savedScrollTop:', savedScrollTop);
                    // 组件显示了
                    if (savedScrollTop !== null) {
                        container.value.scrollTop = savedScrollTop;
                        savedScrollTop = null;
                    }
                } else {
                    // 组件隐藏了
                    // console.log('组件隐藏了');
                }
            }
        }
    });
    // 监视容器元素的 style 属性变化
    observer.observe(container.value, {
        attributes: true,
        attributeFilter: ['style'],
    });
});

onBeforeUnmount(() => {
    // 组件销毁时停止监视
    observer?.disconnect();
});

onDeactivated(() => {
    savedScrollTop = container.value.scrollTop;
    console.log('onDeactivated', 'savedScrollTop:', savedScrollTop);
});

onActivated(() => {
    // console.log('onActivated');
    if (container.value.style.display !== 'none' && savedScrollTop !== null) {
        container.value.scrollTop = savedScrollTop;
        savedScrollTop = null;
    }
});
</script>

<template>
    <component :is="props.tag" ref="container">
        <slot></slot>
    </component>
</template>
