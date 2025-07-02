import { cloneVNode, useSlots, ref, defineComponent, watch } from 'vue';
import { useRoute } from 'vue-router';

export const HoverWrapper = defineComponent({
    name: 'HoverWrapper',
    setup() {
        const hovered = ref(false);
        const route = useRoute();
        const slots = useSlots();
        /** 在触摸事件触发后的短时间内忽略鼠标事件，避免控件仍然处于 hover 状态 */
        let ignoreMouse = false;
        const TIMEOUT = 500;
        const listeners = {
            onTouchstart() {
                console.log(`onTouchstart, current ignoreMouse: ${ignoreMouse}`);
                hovered.value = true;
                ignoreMouse = true;
                setTimeout(() => ignoreMouse = false, TIMEOUT);
            },
            onTouchend() {
                console.log(`onTouchend, current ignoreMouse: ${ignoreMouse}`);
                hovered.value = false;
                ignoreMouse = true;
                setTimeout(() => ignoreMouse = false, TIMEOUT);
            },
            onTouchcancel() {
                console.log(`onTouchcancel, current ignoreMouse: ${ignoreMouse}`);
                hovered.value = false;
                ignoreMouse = true;
                setTimeout(() => ignoreMouse = false, TIMEOUT);
            },
            onMouseenter() {
                console.log(`onMouseenter, current ignoreMouse: ${ignoreMouse}`);
                if (!ignoreMouse) {
                    hovered.value = true;
                }
            },
            onMouseleave() {
                console.log(`onMouseleave, current ignoreMouse: ${ignoreMouse}`);
                if (!ignoreMouse) {
                    hovered.value = false;
                }
            }
        };

        watch(() => route.name, () => {
            console.log(`Route changede, resetting hover state.`);
            hovered.value = false; // reset hover state when changing pages
        });

        // setup 函数返回一个渲染函数
        return () => {
            // 获取默认插槽的 VNode 数组
            const defaultSlot = slots.default?.();
            // 检查以确保只有一个根节点传入，这是此组件的预期用法
            if (defaultSlot == null || defaultSlot.length !== 1) {
                console.warn('<HoverWrapper> 需要且仅需要一个子元素。');
                // 如果不符合要求，直接渲染原始内容或 null
                return defaultSlot;
            }

            // 获取唯一的子节点 VNode
            const childNode = defaultSlot[0];
            const propsToAdd = {
                ...listeners,
                /** 如果当前是悬停状态，则添加 'fluent-hovered' 属性 */
                'fluent-hovered': hovered.value ? '' : undefined, // 添加 hover 状态
            };
            // 克隆子节点并混入新的属性，然后返回克隆后的节点进行渲染
            return cloneVNode(childNode, propsToAdd);
        };
    },
});
