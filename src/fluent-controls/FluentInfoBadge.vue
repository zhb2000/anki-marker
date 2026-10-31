<!--
  WinUI InfoBadge：dot（4px 实心圆）与数字（11px 文字胶囊）两种形态，
  规格取自 InfoBadge_themeresources.xaml —— MinWidth 4、MaxHeight 16、数字字号 11、
  左右内边距 4，底色 AccentFillColorDefault（即本应用强调色）。

  控件自身不可聚焦、无 UIA，含义由父元素承担（调用方给父元素加 aria-label/title）。
  dot 直径可用 --info-badge-dot-size 覆盖：贴在图标按钮角落时 4px 过弱，调用方可适度放大。
-->
<script setup lang="ts">
withDefaults(defineProps<{
    /** 数字形态的数值；不传则为 dot 形态 */
    value?: number | null;
}>(), {
    value: null,
});
</script>

<template>
    <span class="info-badge" :class="{ 'is-dot': value == null }" aria-hidden="true">{{ value }}</span>
</template>

<style scoped>
/* 数字形态：高度固定 16px，最小宽度同高 —— 个位数正好是圆形，
   两位数自然撑成胶囊（WinUI 的胶囊圆角取短边一半，这里用足够大的固定值等效） */
.info-badge {
    box-sizing: border-box;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    min-width: 16px;
    height: 16px;
    padding: 0 5px;
    border-radius: 999px;
    background-color: var(--accent);
    color: var(--control-accent-text-color);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    line-height: 1;
}

.info-badge.is-dot {
    width: var(--info-badge-dot-size, 4px);
    height: var(--info-badge-dot-size, 4px);
    min-width: 0;
    padding: 0;
    border-radius: 50%;
}
</style>
