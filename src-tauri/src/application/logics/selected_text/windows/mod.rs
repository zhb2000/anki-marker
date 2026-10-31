//! Windows 取词实现：UI Automation（TextPattern）+ 模拟 Ctrl+C 剪贴板权威调和。
//!
//! 两条路径分工：UIA 提供选区偏移与上下文（剪贴板做不到）；模拟 Ctrl+C 读到的
//! 剪贴板内容是用户真实选区（UIA 做不到的权威——Edge PDF 的 Adobe 引擎文本层与
//! 视觉渲染错位，UIA 选区实测漂移 2 个字符）。因此 UIA 成功后仍经 Ctrl+C 调和
//! （reconcile_with_primary）：句子含权威词则替换词、不含则降级录权威文本。
//!
//! UIA 拿不到选区（控件不支持 TextPattern、GetSelection 为空或“成功”但返回空串，
//! 如自绘文本的编辑器）时回退纯 Ctrl+C。剪贴板序号不变的语义按 UIA 路径分流：
//! UIA 能正常响应（返回空串）= 没读到选区，返回空结果由调用方提示（无法区分
//! “真的没选”与“应用不向 UIA 暴露选区”）；UIA 报错（查询都失败）且剪贴板不变 =
//! 报取词失败。后者有两种成因：前台窗口以管理员身份运行（UIPI 拦截注入输入），
//! 或前台窗口本就是我们自己的窗口（托盘菜单抢占前台且未归还，见 windows_target
//! ——该情形已由 ensure_foreign_foreground 前置处理，走到这里说明归还也没成，
//! 会另记一条诊断日志）。
//!
//! Ctrl+C 只在“真复制”时才改动用户的剪贴板：注入前查前台窗口，终端里跳过注入（Ctrl+C
//! 在那里是中断信号，语义同“剪贴板不变”）；序号没变时不做任何恢复；复制发生后无论读取
//! 成败都恢复备份。备份覆盖全部可安全复制的格式（见 clipboard_backup），恢复时带上
//! 剪贴板历史排除标记。
//!
//! UIA 无需特殊权限。
//! 后台重试：Chromium 系应用在 UIA 客户端查询时才按需物化无障碍树，重试锁定
//! 目标窗口 HWND（不跟随焦点——弹窗会夺走焦点），树就绪后补发完整结果；
//! 目标窗口由 windows_target::retry_target 给出（前台优先，托盘菜单占着前台时
//! 退到那里跟踪的“用户最近所在的应用窗口”——见 windows_target）。
//!
//! ## 已知限制：UWP 应用取不到选区（2026-09-28 记录，待查）
//!
//! 实测一个 UWP 应用（窗口类 `ApplicationFrameWindow`，宿主 `ApplicationFrameHost.exe`）：
//! 快捷键触发时 UIA 报“焦点元素不支持 TextPattern”、注入的 Ctrl+C 未改变剪贴板；
//! 托盘菜单触发时（归还前台后）UIA 返回**空选区**——两条路都拿不到内容，且**无论是否
//! 选中文字表现一致**；后台重试（窗口内搜持有 TextPattern 的元素）扫 10 次也无结果。
//! 而**手工** Ctrl+C 能复制。待查方向：① 注入的按键可能没有进入 UWP 的输入通道；
//! ② 我们只把前台还给了 frame 窗口，应用内容窗口未必拿到键盘焦点（若属此项，可在归还
//! 后进一步把焦点落到内容子窗口）。两条都不影响非 UWP 应用。

mod clipboard;
mod uia;

use super::capture::{describe_context, reconcile_with_primary};
use super::{RetryCallbacks, SelectedContext};
use clipboard::copy_via_ctrl_c;
use uia::{
    context_from_range, uia_automation, uia_selected_context, uia_selected_text,
    uia_selection_in_window, ComGuard,
};

pub fn get_selected_text() -> Result<String, String> {
    match uia_selected_text() {
        Ok(text) if !text.trim().is_empty() => {
            // UIA 拿到文本：仍经剪贴板调和（选区漂移/撞错控件的修正；
            // 调和失败即剪贴板未变时原样返回 UIA 结果）
            return Ok(reconcile_with_primary(text, None, None, copy_via_ctrl_c(true).ok()).text);
        }
        Ok(_) => {
            // UIA “成功”但为空：无法区分“真没选”与“应用不暴露选区”（如 VSCode），
            // 回退 Ctrl+C 再判一次；应用对 UIA 响应正常，剪贴板不变即为真没选——静默
            log::warn!(
                "UIA path returned empty selected text (no selection or the app does not \
                 expose its selection via UIA), falling back to simulated Ctrl+C"
            );
            return copy_via_ctrl_c(true);
        }
        Err(error) => {
            log::warn!("UIA path failed, falling back to simulated Ctrl+C: {error}");
            return copy_via_ctrl_c(false);
        }
    }
}

/// word_to_sentence 为 true 时尝试“选词取句”：UIA 全链路成功后再经剪贴板调和
/// 返回 {句子, 权威词}；UIA 报错或词为空时回退模拟 Ctrl+C。
/// word_to_sentence 为 false 时完全等同 get_selected_text 的行为。
///
/// `callbacks`：UIA 失败回退后，若目标应用的无障碍树在后台物化
/// （Chromium 系按需激活），后台重试取到完整句子时经 `on_finished` 补发结果；
/// `on_started`/`on_finished` 同时供 UI 呈现“仍在补取”状态（见 RetryCallbacks
/// 的调用约定）。
pub fn get_selected_context(
    word_to_sentence: bool,
    callbacks: RetryCallbacks,
) -> Result<SelectedContext, String> {
    if !word_to_sentence {
        return get_selected_text().map(|text| SelectedContext {
            text,
            word: None,
            word_offset: None,
        });
    }
    match uia_selected_context() {
        Ok(context) if !context.text.trim().is_empty() => {
            let reconciled = reconcile_with_primary(
                context.text,
                context.word,
                context.word_offset,
                copy_via_ctrl_c(true).ok(),
            );
            log::info!("capture via UIA: {}", describe_context(&reconciled));
            return Ok(reconciled);
        }
        Ok(_) => {
            // UIA “成功”但为空：无法区分“真没选”与“应用不暴露选区”，回退 Ctrl+C
            // 再判一次（静默模式：剪贴板不变 = 真没选）；不触发后台重试，以免给
            // “未选中就按快捷键”的常见操作增加延迟
            log::warn!(
                "UIA path returned empty selected text (no selection or the app does not \
                 expose its selection via UIA), falling back to simulated Ctrl+C"
            );
            return copy_via_ctrl_c(true).map(|text| SelectedContext {
                text,
                word: None,
                word_offset: None,
            });
        }
        Err(error) => {
            // UIA 失败：立即回退 Ctrl+C 保证录入不阻塞（严格模式：剪贴板不变 =
            // 前台应用不理会注入的输入，多为 UIPI 提权窗口——报取词失败）；
            // 同时启动后台重试——Chromium 系应用的无障碍树在被 UIA 客户端查询后
            // 按需物化，树就绪后补发句子。
            // 重试锁定目标窗口的 HWND 而不是跟随焦点：随后的回退结果/失败提示
            // 会弹出本应用窗口夺走焦点，跟随焦点会把查询打到我们自己的窗口上；
            // 托盘菜单路径下前台还可能是我们自己的窗口（见 windows_target），
            // retry_target 优先取前台、否则退回跟踪到的用户应用窗口
            if super::windows_target::foreground_is_own() {
                // 诊断：此时 UIA 报的“focused element 不支持 TextPattern”指的很可能是
                // 我们自己的窗口（托盘菜单抢走前台且未归还），与目标应用无关
                log::warn!(
                    "the foreground window belongs to this process; the capture cannot reach the \
                     target app (the tray menu may have taken the foreground window)"
                );
            }
            if let Some(hwnd) = super::windows_target::retry_target() {
                log::info!("spawning background UIA selection retry");
                spawn_selection_retry(hwnd, callbacks);
            } else {
                log::info!("no capture target window, skipping the background UIA selection retry");
            }
            log::warn!("UIA path failed, falling back to simulated Ctrl+C: {error}");
            let context = copy_via_ctrl_c(false).map(|text| SelectedContext {
                text,
                word: None,
                word_offset: None,
            })?;
            log::info!(
                "capture via simulated Ctrl+C: {}",
                describe_context(&context)
            );
            return Ok(context);
        }
    }
}

/// 后台重试的互斥标记：同一时间只允许一个重试在途（避免连按快捷键叠加轮询线程）。
static RETRYING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 重试的收尾守卫：`RETRYING` 复位与 `on_finished` 恰好一次都收在这里。线程中途 panic
/// 时由 Drop 兜底（按超时放弃处理），否则互斥标记会卡死，此后再也不会启动重试。
struct RetryGuard {
    on_finished: Option<Box<dyn FnOnce(Option<SelectedContext>) + Send>>,
}

impl RetryGuard {
    fn finish(mut self, result: Option<SelectedContext>) {
        self.release(result);
    }

    fn release(&mut self, result: Option<SelectedContext>) {
        let Some(on_finished) = self.on_finished.take() else {
            return;
        };
        RETRYING.store(false, std::sync::atomic::Ordering::SeqCst);
        on_finished(result);
    }
}

impl Drop for RetryGuard {
    fn drop(&mut self) {
        self.release(None);
    }
}

/// 后台重试：Chromium 系应用在 UIA 客户端查询时才按需物化无障碍树，首次查询
/// 可能失败。按固定间隔轮询目标窗口（锁定 HWND 重跑取句链路，不跟随系统焦点——
/// 失败/成功弹窗会夺走焦点），取到非空选区后经 `on_finished` 补发完整结果；
/// 超时放弃或重试已过期（`is_cancelled`）时 `on_finished(None)`。
fn spawn_selection_retry(hwnd: ::windows::Win32::Foundation::HWND, callbacks: RetryCallbacks) {
    /// 轮询次数与间隔（总计约 6 秒，覆盖 Chromium 首次物化无障碍树的耗时）
    const RETRY_ATTEMPTS: usize = 10;
    const RETRY_INTERVAL_MS: u64 = 600;

    /// 轮询间隔内检查过期的粒度：旧重试据此在新一次取词后很快退出、让出互斥
    const CANCEL_CHECK_MS: u64 = 50;
    /// 等待旧重试让出互斥的上限（旧重试已随新一次取词过期，至多一个检查粒度加一次
    /// UIA 查询就会退出）
    const ACQUIRE_TIMEOUT_MS: u64 = 2000;

    // HWND 是窗口句柄（实为数值，不含可析构资源），可安全跨线程传递
    let hwnd_raw = hwnd.0 as usize;
    let RetryCallbacks {
        on_started,
        on_finished,
        is_cancelled,
    } = callbacks;
    std::thread::spawn(move || {
        // 同一时间只允许一个重试在途。取词本身已互斥，此时在途的重试必然属于上一次
        // 取词、已经过期，会很快退出——等它让出，而不是直接放弃（直接放弃会让新一次
        // 取词在重试窗口内再也补不回句子）。等待中自身过期或超时则不触发任何回调
        let start = std::time::Instant::now();
        while RETRYING.swap(true, std::sync::atomic::Ordering::SeqCst) {
            if is_cancelled() || start.elapsed().as_millis() >= ACQUIRE_TIMEOUT_MS as u128 {
                log::info!("UIA selection retry skipped: another retry is still running");
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(CANCEL_CHECK_MS));
        }
        // 互斥标记一经占用就由守卫负责复位，并保证 on_finished 恰好一次（含 panic）
        let guard = RetryGuard {
            on_finished: Some(on_finished),
        };
        on_started();
        let hwnd = ::windows::Win32::Foundation::HWND(hwnd_raw as *mut std::ffi::c_void);
        for attempt in 1..=RETRY_ATTEMPTS {
            let mut slept = 0;
            while slept < RETRY_INTERVAL_MS && !is_cancelled() {
                std::thread::sleep(std::time::Duration::from_millis(CANCEL_CHECK_MS));
                slept += CANCEL_CHECK_MS;
            }
            if is_cancelled() {
                log::info!("UIA selection retry cancelled before attempt {attempt}");
                guard.finish(None);
                return;
            }
            let result = (|| {
                let _com = ComGuard::init()?;
                let automation = uia_automation()?;
                let range = uia_selection_in_window(&automation, hwnd)?;
                return context_from_range(&range);
            })();
            match result {
                Ok(context) if !context.text.trim().is_empty() => {
                    // 查询本身可能耗时较久，补发前再判一次
                    if is_cancelled() {
                        log::info!("UIA selection retry cancelled after attempt {attempt}");
                        guard.finish(None);
                        return;
                    }
                    log::info!(
                        "UIA selection retry succeeded on attempt {attempt}: {}",
                        describe_context(&context)
                    );
                    guard.finish(Some(context));
                    return;
                }
                Ok(_) => {
                    log::info!("UIA selection retry attempt {attempt}: empty selection");
                }
                Err(error) => {
                    log::info!("UIA selection retry attempt {attempt}: {error}");
                }
            }
        }
        log::info!("UIA selection retry gave up after {RETRY_ATTEMPTS} attempts");
        guard.finish(None);
    });
}

#[cfg(test)]
mod tests {
    use super::{RetryGuard, RETRYING};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn counting_guard(count: &Arc<AtomicUsize>) -> RetryGuard {
        let count = Arc::clone(count);
        return RetryGuard {
            on_finished: Some(Box::new(move |_| {
                count.fetch_add(1, Ordering::SeqCst);
            })),
        };
    }

    /// 正常收尾、线程 panic 两条路径：on_finished 恰好一次、互斥标记都要复位。
    /// 两段写在同一个测试里，是因为 RETRYING 是全局状态，不能并行断言。
    #[test]
    fn retry_guard_finishes_exactly_once() {
        let count = Arc::new(AtomicUsize::new(0));

        RETRYING.store(true, Ordering::SeqCst);
        counting_guard(&count).finish(None);
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert!(!RETRYING.load(Ordering::SeqCst));

        RETRYING.store(true, Ordering::SeqCst);
        let guard = counting_guard(&count);
        let panicked = std::thread::spawn(move || {
            let _guard = guard;
            panic!("simulated retry thread panic");
        })
        .join();
        assert!(panicked.is_err());
        assert_eq!(count.load(Ordering::SeqCst), 2);
        assert!(!RETRYING.load(Ordering::SeqCst));
    }
}
