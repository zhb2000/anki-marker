import { ref, watch, computed } from 'vue';
import { useRoute } from 'vue-router';

/** 用 JavaScript 实现 hover 以同时支持鼠标和触摸 */
export function useHover() {
    const hovered = ref(false);
    const route = useRoute();

    /** 在触摸事件触发后的短时间内忽略鼠标事件，避免控件仍然处于 hover 状态 */
    let ignoreMouse = false;
    const TIMEOUT = 500;

    const listeners = {
        touchstart() {
            hovered.value = true;
            ignoreMouse = true;
            setTimeout(() => ignoreMouse = false, TIMEOUT);
        },
        touchend() {
            hovered.value = false;
            ignoreMouse = true;
            setTimeout(() => ignoreMouse = false, TIMEOUT);
        },
        touchcancel() {
            hovered.value = false;
            ignoreMouse = true;
            setTimeout(() => ignoreMouse = false, TIMEOUT);
        },
        mouseenter() {
            if (!ignoreMouse) {
                hovered.value = true;
            }
        },
        mouseleave() {
            if (!ignoreMouse) {
                hovered.value = false;
            }
        }
    };

    watch(() => route.name, () => {
        hovered.value = false; // reset hover state when changing pages
    });

    const classes = computed(() => ({ hover: hovered.value }));

    return { hovered, listeners, classes };
}
