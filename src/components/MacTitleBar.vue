<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue';

import { os, window as tauriWindow } from '../tauri-api';

/**
 * macOS 覆盖式标题栏（titleBarStyle: Overlay）的拖动命中条。
 *
 * 内容视图铺满整个窗口后，顶部一条（约 32px）就是原生红绿灯所在的原标题栏区域。
 * 这条透明的固定层只负责窗口拖动：空白处按住拖动与原生标题栏一致；
 * 红绿灯在原生层，不受这层影响。
 *
 * 双击不在这里处理：隐藏标题（hiddenTitle）后 AppKit 不再处理标题栏双击，
 * 改由 Rust 侧的原生事件监听按系统偏好执行（见 window.rs 的 install_titlebar_monitor），
 * 这样窗口未激活、或网页收不到该区域事件时也能生效。
 */
function detectMacOS(): boolean {
    try {
        return os.type() === 'macos';
    } catch {
        return false; // 浏览器调试环境
    }
}

const isMacOS = detectMacOS();
// 同步打标记：标题栏内边距靠 html.macos 提供，避免首帧内容顶到红绿灯下面
if (isMacOS) {
    document.documentElement.classList.add('macos');
}

const fullscreen = ref(false);
let unlistenResize: (() => void) | null = null;
let resizeTimer: number | null = null;

onMounted(async () => {
    if (!isMacOS) {
        return;
    }
    const appWindow = tauriWindow.getCurrentWindow();
    try {
        const syncFullscreen = async (): Promise<void> => {
            const value = await appWindow.isFullscreen();
            if (value === fullscreen.value) {
                return;
            }
            fullscreen.value = value;
            document.documentElement.classList.toggle('is-fullscreen', value);
        };
        await syncFullscreen();
        // 进入/退出全屏时尺寸会连续变化；防抖后再查询，避免拖动缩放时频繁 IPC
        unlistenResize = await appWindow.onResized(() => {
            if (resizeTimer != null) {
                window.clearTimeout(resizeTimer);
            }
            resizeTimer = window.setTimeout(() => {
                resizeTimer = null;
                void syncFullscreen();
            }, 120);
        });
    } catch {
        // 非 Tauri 环境（浏览器调试）
    }
});

onBeforeUnmount(() => {
    unlistenResize?.();
    if (resizeTimer != null) {
        window.clearTimeout(resizeTimer);
    }
    document.documentElement.classList.remove('macos', 'is-fullscreen');
});

/** 只在单击时启动拖动；双击的第二下由 Rust 侧监听吞掉 */
function handleMouseDown(event: MouseEvent): void {
    if (event.button !== 0 || event.detail !== 1) {
        return;
    }
    void tauriWindow.getCurrentWindow().startDragging().catch(() => { /* 非 Tauri 环境 */ });
}
</script>

<template>
    <div v-if="isMacOS && !fullscreen" class="mac-titlebar" aria-hidden="true" @mousedown="handleMouseDown"></div>
</template>

<style scoped>
/* 覆盖顶部标题栏高度：整条都是拖动区（红绿灯在原生层，点击照常生效） */
.mac-titlebar {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    height: var(--titlebar-inset, 32px);
    z-index: 3000;
    user-select: none;
    -webkit-user-select: none;
}
</style>
