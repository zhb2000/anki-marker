/**
 * 输入框滚轮横向滚动（WebKit 兼容层）：
 * Chromium（Windows WebView2）悬停未聚焦的 input 即可用滚轮/触控板横滑滚动溢出内容，
 * 但 WebKit（macOS WKWebView、Linux WebKitGTK）只在聚焦后才滚动，
 * 此处手动把 wheel 事件映射到 scrollLeft，使三端行为一致（对齐 Chromium）。
 *
 * 仅在内容横向溢出且未滚到边界时拦截（preventDefault），其余情况放行给页面正常滚动。
 * 纵向滚轮不映射（与 Chromium 一致：input 内部无纵向溢出，wheel 穿透给页面滚动）。
 */
export function handleInputWheel(event: WheelEvent) {
    if (event.deltaX === 0) {
        return;
    }
    const el = event.currentTarget as HTMLInputElement;
    const maxScrollLeft = el.scrollWidth - el.clientWidth;
    if (maxScrollLeft <= 0) {
        return;
    }
    const canScroll = (event.deltaX < 0 && el.scrollLeft > 0) || (event.deltaX > 0 && el.scrollLeft < maxScrollLeft);
    if (!canScroll) {
        return;
    }
    event.preventDefault();
    el.scrollLeft += event.deltaX;
}
