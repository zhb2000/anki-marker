<script setup lang="ts">
import { ref, watch } from 'vue';
import { useRoute } from 'vue-router';
import { ElConfigProvider } from 'element-plus';

const currentRoute = useRoute();
const transitionName = ref<string | undefined>(undefined);

watch(() => currentRoute.path, (newPath, _oldPath) => {
    const isBack = newPath === '/'; // 简单判断是否是“后退”操作
    transitionName.value = isBack ? 'slide-right' : 'slide-left';
});
</script>

<template>
    <ElConfigProvider :message="{ showClose: true }">
        <RouterView v-slot="{ Component, route }">
            <Transition :name="transitionName">
                <KeepAlive>
                    <component :is="Component" :key="route.path" />
                </KeepAlive>
            </Transition>
        </RouterView>
    </ElConfigProvider>
</template>

<style scoped>
/* #region 仿移动端页面切换动画 */
/* https://juejin.cn/post/7092789643401232398 */
.slide-right-enter-active,
.slide-left-enter-active,
.slide-right-leave-active,
.slide-left-leave-active {
    transition: all 0.3s ease-out;
    will-change: transform;
    position: absolute !important;
    background-color: white;
    left: 0;
    right: 0;
    top: 0;
    bottom: 0;
    box-shadow: -16px 0 16px 0px rgba(0, 0, 0, 0.1);
}

.slide-left-enter-from {
    z-index: 100;
    transform: translateX(100%);
    box-shadow: -16px 0 16px 0px rgba(0, 0, 0, 0.1);
}

.slide-left-leave-to {
    /* opacity: 0.4; */
    filter: brightness(80%);
    transform: translateX(-50%);
}

.slide-right-enter-from {
    /* opacity: 0.4; */
    filter: brightness(80%);
    transform: translateX(-50%);
}

.slide-right-leave-from {
    box-shadow: -16px 0 16px 0px rgba(0, 0, 0, 0.1);
}

.slide-right-leave-to {
    z-index: 100;
    transform: translateX(100%);
}

/* #endregion */

/* #region 页面平移切换动画 */
/* https://zhuanlan.zhihu.com/p/424219196 */
/* .slide-left-enter-active,
.slide-left-leave-active,
.slide-right-enter-active,
.slide-right-leave-active {
    transition: all 0.2s ease-out;
    will-change: transform;
    position: absolute !important;
    background-color: white;
    left: 0;
    right: 0;
    top: 0;
    bottom: 0;
    z-index: 1;
}

.slide-left-enter-from {
    opacity: 1;
    transform: translateX(100%);
}

.slide-left-leave-to {
    opacity: 0.4;
    transform: translateX(-100%);
}

.slide-right-enter-from {
    opacity: 1;
    transform: translateX(-100%);
}

.slide-right-leave-to {
    opacity: 0.4;
    transform: translateX(100%);
} */

/* #endregion */
</style>

<style>
* {
    box-sizing: border-box;
}

html,
body {
    /* 去除 Edge 自带的回弹效果 */
    overscroll-behavior: none;
}

html,
body,
#app {
    /* 避免页面切换动画时出现滚动条，内容滚动由页面自己负责 */
    overflow: hidden;
    height: 100vh;
    background-color: var(--window-background);
}
</style>
