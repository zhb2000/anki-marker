//! AppleScript 回退路径：等修饰键松开后模拟 Cmd+C 读取剪贴板（临时静音、事后恢复）。

use super::ax_ffi;

/// 模拟 Cmd+C 读取剪贴板的 AppleScript：
/// 备份剪贴板 → 临时静音系统提示音 → 模拟 Cmd+C → 轮询剪贴板变化计数 →
/// 读取剪贴板 → 恢复剪贴板与音量。
///
/// 输出协议：首字符为状态——`0` 表示剪贴板在超时内没有变化（应用没理会 Cmd+C，
/// 或没有选中内容），`1` 表示复制成功，其后紧跟所选文本。用状态码而不是空串
/// 表示“未变”，Rust 侧才能按触发场景分流（见 get_selected_text_by_applescript）。
///
/// 轮询而不是固定 delay：每 15ms 查一次 changeCount，最长约 1 秒——慢应用不会
/// 因固定等待太短读到旧内容，快应用也不用白等。上限不能太短：写入晚于上限的话，
/// 用户的剪贴板被选中文本覆盖后就再没有人恢复了（Word 这类慢应用正是走到这条路径
/// 的典型）；代价是真没选中时“没有选区”的反馈要等满上限。检测到变化后再留 30ms
/// 收尾：有的应用先 clearContents（计数已变）、稍后才写入数据，立即读取会读到空。
///
/// 静音之后的全部逻辑都包在 try 里、由同一个出口恢复：中途任何一步报错（如有“自动化”
/// 权限但没有“辅助功能”权限时 keystroke 报 1002）都不能把提示音永久留在静音，也不能
/// 丢掉用户的剪贴板；报错在恢复之后原样抛出。复制到的内容不是文本（如选中了图片）时
/// 返回 `0`，同样恢复。恢复音量不经 System Events，不依赖可能正是失败原因的权限；
/// 恢复剪贴板只在计数确实变了时才做，没变就不去重写（重写可能丢失原内容的其他格式）。
///
/// 剪贴板备份/恢复是尽力而为的：剪贴板当前内容不是文本（如图片、文件）时
/// `the clipboard` 读取失败，此时跳过备份与恢复、只做复制读取——牺牲“剪贴板
/// 无痕”换取回退成功率（代价是此类场景下原剪贴板内容会被复制的文本覆盖）。
const APPLE_SCRIPT: &str = r#"
use AppleScript version "2.4"
use scripting additions
use framework "Foundation"
use framework "AppKit"

set savedAlertVolume to alert volume of (get volume settings)

-- Back up clipboard contents (best-effort; fails for non-text clipboard, e.g. images):
set clipboardSaved to false
try
    set savedClipboard to the clipboard
    set clipboardSaved to true
end try

set thePasteboard to current application's NSPasteboard's generalPasteboard()
set theCount to thePasteboard's changeCount()

set theResult to "0"
set didFail to false
try
    tell application "System Events"
        set volume alert volume 0
    end tell

    -- Copy selected text to clipboard:
    tell application "System Events" to keystroke "c" using {command down}

    -- Poll the pasteboard change count (up to ~1s) instead of a fixed delay:
    set didChange to false
    repeat 66 times
        if thePasteboard's changeCount() is not theCount then
            set didChange to true
            exit repeat
        end if
        delay 0.015
    end repeat

    if didChange then
        -- Some apps clear the pasteboard first and write the data a moment later:
        delay 0.03

        -- The copied content may not be text (e.g. an image): theResult stays "0".
        try
            set theResult to "1" & (the clipboard as text)
        end try
    end if
on error errorMessage number errorNumber
    set didFail to true
    set failureMessage to errorMessage
    set failureNumber to errorNumber
end try

-- Single exit: whatever happened above, put the volume and the clipboard back.
try
    set volume alert volume savedAlertVolume
end try

if clipboardSaved and (thePasteboard's changeCount() is not theCount) then
    try
        set the clipboard to savedClipboard
    end try
end if

if didFail then error failureMessage number failureNumber

return theResult
"#;

/// 模拟 Cmd+C 前等用户松开快捷键的修饰键：快捷键在按下时触发，形如 Cmd+Shift+X 时用户
/// 多半还按着 Shift，模拟按键会与之合并成 Cmd+Shift+C（Chrome 里是“检查元素”）。
/// 有界等待：超时照常继续（用户一直按着时总不能不取词）。
fn wait_for_modifier_release() {
    const TIMEOUT: std::time::Duration = std::time::Duration::from_millis(300);
    const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(10);

    let read_flags =
        || unsafe { ax_ffi::CGEventSourceFlagsState(ax_ffi::CG_EVENT_SOURCE_STATE_HID_SYSTEM) };
    if !modifiers_released_within(read_flags, TIMEOUT, POLL_INTERVAL) {
        log::info!("modifier keys are still held, simulating Cmd+C anyway");
    }
}

/// 轮询修饰键状态直到全部松开（返回 true）或超时（返回 false）。
fn modifiers_released_within(
    mut read_flags: impl FnMut() -> u64,
    timeout: std::time::Duration,
    poll_interval: std::time::Duration,
) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if read_flags() & ax_ffi::CG_EVENT_FLAG_MASK_MODIFIERS == 0 {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(poll_interval);
    }
}

/// 通过 AppleScript 模拟 Cmd+C 并读取剪贴板，获取当前选中的文本。
///
/// `silent_if_unchanged`：剪贴板在模拟 Cmd+C 后未变化的语义。true（AX 能正常响应、
/// 只是没读到选区）= 什么都没选，返回空串由调用方提示“没有选区”；false（AX 查询
/// 本身失败）= 没有任何一路能证明目标应用里有选区，剪贴板不变更可能是取词目标
/// 取错了、焦点在安全输入框、或应用不理会模拟按键，返回 Err 报取词失败。
pub(super) fn get_selected_text_by_applescript(
    silent_if_unchanged: bool,
) -> Result<String, String> {
    wait_for_modifier_release();
    let output = std::process::Command::new("osascript")
        .arg("-e")
        .arg(APPLE_SCRIPT)
        .output()
        .map_err(|e| format!("failed to run osascript: {e}"))?;
    if output.status.success() {
        let content = String::from_utf8(output.stdout)
            .map_err(|e| format!("osascript output is not valid utf-8: {e}"))?;
        return parse_applescript_output(&content, silent_if_unchanged);
    }
    let error = String::from_utf8_lossy(&output.stderr).into_owned();
    return Err(format!("osascript failed: {error}"));
}

/// 解析 APPLE_SCRIPT 的输出协议（首字符状态码 + 所选文本）。
fn parse_applescript_output(output: &str, silent_if_unchanged: bool) -> Result<String, String> {
    if let Some(text) = output.strip_prefix('1') {
        return Ok(text.trim().to_string());
    }
    if output.trim() == "0" {
        if silent_if_unchanged {
            return Ok(String::new());
        }
        return Err(
            "the clipboard did not change after the simulated Cmd+C (the target app may not \
             have been the one we queried, the focus may be in a secure input field, or the app \
             ignores the simulated keystroke)"
                .to_string(),
        );
    }
    return Err(format!("unexpected osascript output: {output:?}"));
}

#[cfg(test)]
mod tests {
    use super::{parse_applescript_output, APPLE_SCRIPT};

    /// AppleScript 输出协议：成功取文本、未变按触发语义分流、协议外输出报错
    #[test]
    fn applescript_output_parsing() {
        assert_eq!(
            parse_applescript_output("1hello world\n", true),
            Ok("hello world".into())
        );
        // 所选文本本身以 "0"/"1" 开头不应被误判（状态码只看首字符）
        assert_eq!(
            parse_applescript_output("10 apples\n", false),
            Ok("0 apples".into())
        );
        // 剪贴板未变：AX 正常（silent）→ 空串；AX 报错（strict）→ Err
        assert_eq!(parse_applescript_output("0\n", true), Ok(String::new()));
        assert!(parse_applescript_output("0\n", false).is_err());
        assert!(parse_applescript_output("", true).is_err());
        assert!(parse_applescript_output("garbage", true).is_err());
    }

    /// 等修饰键松开：已松开立即返回；中途松开返回 true；一直按着有界超时；Caps Lock 不算
    #[test]
    fn modifier_release_wait() {
        use std::time::{Duration, Instant};

        const SHIFT: u64 = 0x0002_0000;
        const COMMAND: u64 = 0x0010_0000;
        const CAPS_LOCK: u64 = 0x0001_0000;
        let timeout = Duration::from_millis(50);
        let poll = Duration::from_millis(1);

        assert!(super::modifiers_released_within(|| 0, timeout, poll));
        // Caps Lock 是锁定键：长期置位也不能让每次取词都白等
        assert!(super::modifiers_released_within(
            || CAPS_LOCK,
            Duration::ZERO,
            poll
        ));
        let mut reads = 0;
        let released = super::modifiers_released_within(
            || {
                reads += 1;
                return if reads < 3 { COMMAND | SHIFT } else { 0 };
            },
            Duration::from_secs(5),
            poll,
        );
        assert!(released);
        assert_eq!(reads, 3);
        let started = Instant::now();
        assert!(!super::modifiers_released_within(|| SHIFT, timeout, poll));
        assert!(started.elapsed() >= timeout);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    /// 执行打桩后的 APPLE_SCRIPT：真实的控制流（try / 恢复 / 重新抛出 / 输出协议），但把会
    /// 动用户状态的操作都换成 log——不碰系统音量与通用剪贴板，换成用完即弃的私有 pasteboard
    ///（按进程与序号命名：实测 pasteboardWithUniqueName 在并行的 osascript 之间会撞名，
    /// 别的测试的写入会让本测试误判剪贴板变了）。
    /// `copy`：模拟按键那一行的替身；`read`：读剪贴板表达式的替身。返回 {是否成功, stdout, 日志}。
    fn run_stubbed_apple_script(
        copy: &str,
        read: &str,
        extra: &[(&str, &str)],
    ) -> (bool, String, String) {
        static NEXT_PASTEBOARD: std::sync::atomic::AtomicUsize =
            std::sync::atomic::AtomicUsize::new(0);

        let pasteboard = format!(
            "pasteboardWithName:\"anki-marker-test-{}-{}\"",
            std::process::id(),
            NEXT_PASTEBOARD.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        );
        let mut script = APPLE_SCRIPT.to_string();
        let stubs = [
            ("generalPasteboard()", pasteboard.as_str()),
            (
                "set savedClipboard to the clipboard",
                "set savedClipboard to \"original\"",
            ),
            (
                "tell application \"System Events\"\n        set volume alert volume 0\n    end tell",
                "log \"muted\"",
            ),
            (
                "tell application \"System Events\" to keystroke \"c\" using {command down}",
                copy,
            ),
            ("the clipboard as text", read),
            (
                "set volume alert volume savedAlertVolume",
                "log \"volume restored\"",
            ),
            (
                "set the clipboard to savedClipboard",
                "log \"clipboard restored\"",
            ),
            (
                "if didFail then",
                "thePasteboard's releaseGlobally()\nif didFail then",
            ),
        ];
        for (from, to) in stubs.iter().chain(extra.iter()) {
            assert!(
                script.contains(from),
                "the script no longer contains {from:?}"
            );
            script = script.replace(from, to);
        }
        let output = std::process::Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .output()
            .expect("failed to run osascript");
        return (
            output.status.success(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        );
    }

    /// 模拟复制成功：文本逐字符完整返回（含换行、以状态码字符开头），音量与剪贴板都恢复
    #[test]
    fn apple_script_success_restores_state_and_keeps_text() {
        let (ok, stdout, log) = run_stubbed_apple_script(
            "thePasteboard's clearContents()",
            "\"0 apples\" & linefeed & \"  and pears\"",
            &[],
        );
        assert!(ok, "{log}");
        assert_eq!(
            parse_applescript_output(&stdout, false),
            Ok("0 apples\n  and pears".to_string())
        );
        assert!(log.contains("volume restored"), "{log}");
        assert!(log.contains("clipboard restored"), "{log}");
    }

    /// 模拟按键报错（有“自动化”无“辅助功能”权限时的 1002）：恢复音量后原样抛出；
    /// 剪贴板没变就不去重写
    #[test]
    fn apple_script_failure_restores_volume_and_rethrows() {
        let (ok, _, log) = run_stubbed_apple_script(
            "error \"not allowed to send keystrokes\" number 1002",
            "\"unused\"",
            &[],
        );
        assert!(!ok);
        assert!(log.contains("not allowed to send keystrokes"), "{log}");
        assert!(log.contains("1002"), "{log}");
        assert!(log.contains("volume restored"), "{log}");
        assert!(!log.contains("clipboard restored"), "{log}");
    }

    /// 剪贴板已被覆盖后才报错：音量和剪贴板都要恢复，再抛出
    #[test]
    fn apple_script_failure_after_copy_restores_clipboard() {
        let (ok, _, log) = run_stubbed_apple_script(
            "thePasteboard's clearContents()\nerror \"boom\" number 1",
            "\"unused\"",
            &[],
        );
        assert!(!ok);
        assert!(log.contains("boom"), "{log}");
        assert!(log.contains("volume restored"), "{log}");
        assert!(log.contains("clipboard restored"), "{log}");
    }

    /// 复制到的内容不是文本：返回状态 0，恢复音量与剪贴板
    #[test]
    fn apple_script_non_text_copy_returns_unchanged_status() {
        let (ok, stdout, log) = run_stubbed_apple_script(
            "thePasteboard's clearContents()",
            // 记录强转文本必然报错，与“剪贴板是图片时 as text 报错”走同一条路
            "({a:1} as text)",
            &[],
        );
        assert!(ok, "{log}");
        assert_eq!(stdout.trim(), "0");
        assert!(log.contains("volume restored"), "{log}");
        assert!(log.contains("clipboard restored"), "{log}");
    }

    /// 应用没理会模拟按键：返回状态 0，恢复音量，不重写剪贴板
    #[test]
    fn apple_script_unchanged_clipboard_returns_unchanged_status() {
        let (ok, stdout, log) = run_stubbed_apple_script("log \"ignored\"", "\"unused\"", &[]);
        assert!(ok, "{log}");
        assert_eq!(stdout.trim(), "0");
        assert!(log.contains("volume restored"), "{log}");
        assert!(!log.contains("clipboard restored"), "{log}");
    }

    /// 慢应用写入剪贴板晚于旧上限（第 20 轮约 300ms）：仍判为复制成功并恢复剪贴板。
    /// 回归：旧实现此时直接返回 0，用户的剪贴板被覆盖且不再恢复
    #[test]
    fn apple_script_late_clipboard_write_is_restored() {
        let (ok, stdout, log) = run_stubbed_apple_script(
            "set pollCount to 0",
            "\"late text\"",
            &[(
                "delay 0.015",
                "set pollCount to pollCount + 1\n\
                 if pollCount = 40 then thePasteboard's clearContents()\n\
                 delay 0.015",
            )],
        );
        assert!(ok, "{log}");
        assert_eq!(
            parse_applescript_output(&stdout, false),
            Ok("late text".to_string())
        );
        assert!(log.contains("clipboard restored"), "{log}");
    }

    /// APPLE_SCRIPT 应能被 osacompile 编译（仅语法检查，不执行脚本）。
    #[test]
    fn apple_script_syntax_check() {
        let target_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
        std::fs::create_dir_all(&target_dir).expect("failed to create target dir");
        let output_path = target_dir.join("apple-script-syntax-check.scpt");
        let output = std::process::Command::new("osacompile")
            .arg("-e")
            .arg(APPLE_SCRIPT)
            .arg("-o")
            .arg(&output_path)
            .output()
            .expect("failed to run osacompile");
        let _ = std::fs::remove_file(&output_path);
        assert!(
            output.status.success(),
            "osacompile failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
