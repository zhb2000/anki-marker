import { computed, ref, watch } from 'vue';

/**
 * 文本输入控件的 v-model 草稿层：让写在组件上的 .trim 修饰符与原生 v-model.trim 行为一致。
 *
 * 组件上写 v-model.trim 时，Vue 的 emit 会把 update:modelValue 的值裁剪后再交给父组件；
 * 父组件回传的裁剪值若直接绑到内部元素，元素会被重新写成裁剪后的文本——句末刚打的空格
 * 被立刻吞掉，光标也会跳动。原生 v-model.trim 在聚焦期间会跳过这次回写，组件透传不到这层保护。
 *
 * 因此内部元素绑定本地草稿：父组件回传的值只是草稿裁剪后的“回声”时不覆盖草稿，外部真正
 * 改了值（程序化写入、清空）才同步进来；trim 时在 change（回车或失焦提交）时把草稿本身裁剪掉，
 * 与原生行为一致。
 *
 * @returns text 供内部元素 v-model 绑定；commitTrim 绑在内部元素的 change 事件上
 */
export function useTextModel(model: { value: unknown; }, modifiers: Partial<Record<string, true>>) {
    const toText = (value: unknown): string => {
        // eslint-disable-next-line @typescript-eslint/no-base-to-string
        return value == null ? '' : String(value);
    };
    const draft = ref(toText(model.value));
    /** 草稿经 v-model 修饰符处理后父组件会收到的值 */
    const echoOf = (text: string): string => modifiers.trim === true ? text.trim() : text;

    watch(() => model.value, value => {
        const text = toText(value);
        if (text !== echoOf(draft.value)) {
            draft.value = text;
        }
    });

    const text = computed({
        get: () => draft.value,
        set: (value: string) => {
            draft.value = value;
            model.value = value;
        },
    });

    function commitTrim(): void {
        if (modifiers.trim === true) {
            const trimmed = draft.value.trim();
            if (trimmed !== draft.value) {
                text.value = trimmed;
            }
        }
    }

    return { text, commitTrim };
}
