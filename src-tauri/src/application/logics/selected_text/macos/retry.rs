//! AX 失败后的后台重试：全局槽位互斥与收尾守卫、唤醒目标应用的无障碍树
//!（AXManualAccessibility / AXEnhancedUserInterface）与轮询补发。

use super::super::capture::describe_context;
use super::super::{RetryCallbacks, SelectedContext};
use super::ax::{
    copy_element_attribute_by_ax, create_application_element, find_element_with_selection_in_window,
};
use super::ax_ffi;
use super::context_from_word_and_element;

/// 设置应用元素的布尔属性，返回 AXError（0 表示成功）。
fn set_app_bool_attribute(
    app: &core_foundation::base::CFType,
    attribute: &'static str,
    value: bool,
) -> ax_ffi::AXError {
    use core_foundation::base::TCFType;
    use core_foundation::boolean::CFBoolean;
    use core_foundation::string::CFString;

    let attribute_ref = CFString::from_static_string(attribute);
    let value = if value {
        CFBoolean::true_value()
    } else {
        CFBoolean::false_value()
    };
    return unsafe {
        ax_ffi::AXUIElementSetAttributeValue(
            app.as_CFTypeRef(),
            attribute_ref.as_concrete_TypeRef(),
            value.as_CFTypeRef(),
        )
    };
}

/// 本次重试把 AXEnhancedUserInterface 从关闭改成了开启：Drop 时复原。
///
/// 这个开关持续生效：会让响应它的应用常驻完整无障碍树（额外的 CPU 与内存开销），
/// 窗口管理器移动其窗口时也可能出现动画异常，不能在重试结束后留给用户。
struct EnhancedUiRestore {
    app: core_foundation::base::CFType,
}

impl Drop for EnhancedUiRestore {
    fn drop(&mut self) {
        let attribute = ax_ffi::ENHANCED_USER_INTERFACE_ATTRIBUTE;
        let err = set_app_bool_attribute(&self.app, attribute, false);
        if err != 0 {
            log::warn!("restoring \"{attribute}\" on the application failed: AXError {err}");
        }
    }
}

/// 唤醒目标应用的无障碍支持（尽力而为）：Electron/Chromium 系应用监听
/// AXManualAccessibility 属性被设置来激活无障碍树，它没有副作用，每次轮询都设；
/// AXEnhancedUserInterface 是另一类应用使用的开关，只在前者不被接受、且它原本关闭时才
/// 开启，并经 `enhanced_restore` 在重试结束时复原。失败忽略——实测 Word/Zotero 对两者
/// 均不响应（-25205 / -25208），它们的树是被反复查询后异步物化的，不依赖这里；保留此
/// 机制给其他可能响应的应用一个机会。
fn wake_app_accessibility(
    app: &core_foundation::base::CFType,
    enhanced_restore: &mut Option<EnhancedUiRestore>,
) {
    use core_foundation::boolean::CFBoolean;

    let manual = ax_ffi::MANUAL_ACCESSIBILITY_ATTRIBUTE;
    let err = set_app_bool_attribute(app, manual, true);
    if err == 0 {
        return;
    }
    log::info!("setting \"{manual}\" on the application failed: AXError {err}");
    if enhanced_restore.is_some() {
        return;
    }
    let enhanced = ax_ffi::ENHANCED_USER_INTERFACE_ATTRIBUTE;
    // 已是开启状态（用户的辅助工具打开的）：既不必再设，也不该由本应用复原
    let already_enabled = copy_element_attribute_by_ax(app, enhanced)
        .ok()
        .and_then(|value| value.downcast_into::<CFBoolean>())
        .is_some_and(bool::from);
    if already_enabled {
        return;
    }
    let err = set_app_bool_attribute(app, enhanced, true);
    if err == 0 {
        *enhanced_restore = Some(EnhancedUiRestore { app: app.clone() });
    } else {
        log::info!("setting \"{enhanced}\" on the application failed: AXError {err}");
    }
}

/// 后台重试的互斥标记：0 表示无重试在途，否则为正在重试的应用 pid。
static RETRYING_PID: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// 重试的收尾守卫：释放互斥槽位并调用 `on_finished`，恰好一次。轮询中途 panic 时由 Drop
/// 以 `on_finished(None)` 收尾——否则槽位永不释放（此后所有重试都启动不了），界面也会
/// 一直停在“仍在补取”。
struct RetryFinishGuard {
    on_finished: Option<Box<dyn FnOnce(Option<SelectedContext>) + Send>>,
}

impl RetryFinishGuard {
    fn finish(&mut self, outcome: Option<SelectedContext>) {
        // 槽位的释放与回调绑定在同一次 take 上：重复收尾若再释放一次，
        // 可能释放掉下一个重试已占用的槽位
        if let Some(on_finished) = self.on_finished.take() {
            RETRYING_PID.store(0, std::sync::atomic::Ordering::SeqCst);
            on_finished(outcome);
        }
    }
}

impl Drop for RetryFinishGuard {
    fn drop(&mut self) {
        self.finish(None);
    }
}

/// 占用重试互斥槽位（同一时间只允许一个轮询线程）；等待期间本次重试已过期则返回 false，
/// 此时重试未真正启动，不触发任何回调。
///
/// 上层的取词互斥保证同一时间只有一次取词，这里遇到的在途重试必然属于更早的取词、已被判为
/// 过期，下一次轮询前就会退出，因此是等它让出，而不是放弃本次：放弃的话，用户在 Word 里
/// 重试窗口内再按一次，旧重试被作废、新重试又没启动，就再也补不回句子了。
fn acquire_retry_slot(pid: i32, is_cancelled: &dyn Fn() -> bool) -> bool {
    use std::sync::atomic::Ordering;

    const WAIT_INTERVAL: std::time::Duration = std::time::Duration::from_millis(20);

    while RETRYING_PID
        .compare_exchange(0, pid, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        if is_cancelled() {
            return false;
        }
        std::thread::sleep(WAIT_INTERVAL);
    }
    return true;
}

/// 重试的轮询骨架（调用方已占用互斥槽位）：每次轮询前、以及交付结果前都检查是否已过期，
/// 过期即停止；`on_finished` 恰好调用一次，过期与超时都是 None。
fn run_selection_retry(
    callbacks: RetryCallbacks,
    attempts: usize,
    interval: std::time::Duration,
    mut poll: impl FnMut(usize) -> Option<SelectedContext>,
) {
    let RetryCallbacks {
        on_started,
        on_finished,
        is_cancelled,
    } = callbacks;
    let mut guard = RetryFinishGuard {
        on_finished: Some(on_finished),
    };
    on_started();
    for attempt in 1..=attempts {
        std::thread::sleep(interval);
        if is_cancelled() {
            log::info!("AX selection retry cancelled before attempt {attempt}");
            break;
        }
        if let Some(context) = poll(attempt) {
            if is_cancelled() {
                log::info!(
                    "AX selection retry cancelled, dropping the result of attempt {attempt}"
                );
                break;
            }
            guard.finish(Some(context));
            return;
        }
    }
    guard.finish(None);
}

/// 后台重试的一次轮询：唤醒属性 → 树搜索 → 取句链路。
fn poll_selection_once(
    pid: i32,
    attempt: usize,
    enhanced_restore: &mut Option<EnhancedUiRestore>,
) -> Option<SelectedContext> {
    let app = create_application_element(pid)?;
    wake_app_accessibility(&app, enhanced_restore);
    let window = copy_element_attribute_by_ax(&app, ax_ffi::FOCUSED_WINDOW_ATTRIBUTE).ok()?;
    let (element, word) = find_element_with_selection_in_window(&window)?;
    let context = context_from_word_and_element(&element, &word);
    log::info!(
        "AX selection retry succeeded on attempt {attempt}: {}",
        describe_context(&context)
    );
    return Some(context);
}

/// 后台重试：部分应用（实测 Word）的无障碍树在首次被查询后才异步物化——
/// 首次划词时是只有百余节点的桩树，反复查询几秒后树才建成（所以第二次划词
/// 才成功）。此函数在后台按固定间隔轮询目标应用（pid 不随前台切换失效），
/// 树就绪后取到句子并通过 `on_finished` 补发。总轮询窗口约
/// RETRY_ATTEMPTS * RETRY_INTERVAL，超时或过期放弃（`on_finished(None)`）。
pub(super) fn spawn_selection_retry(pid: i32, callbacks: RetryCallbacks) {
    /// 轮询次数与间隔（总计约 3 秒，覆盖实测的树物化耗时）
    const RETRY_ATTEMPTS: usize = 5;
    const RETRY_INTERVAL: std::time::Duration = std::time::Duration::from_millis(600);

    std::thread::spawn(move || {
        if !acquire_retry_slot(pid, callbacks.is_cancelled.as_ref()) {
            return;
        }
        // 随线程结束（重试结束）Drop，复原本次开启的 AXEnhancedUserInterface
        let mut enhanced_restore = None;
        run_selection_retry(callbacks, RETRY_ATTEMPTS, RETRY_INTERVAL, |attempt| {
            return poll_selection_once(pid, attempt, &mut enhanced_restore);
        });
    });
}

#[cfg(test)]
mod tests {
    /// 重试测试共用全局互斥槽位，须串行
    static RETRY_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// 记录回调情况的 RetryCallbacks：返回 {回调, 开始次数, 各次结束的结果文本, 过期标志}
    #[allow(clippy::type_complexity)]
    fn recording_callbacks() -> (
        super::RetryCallbacks,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
        std::sync::Arc<std::sync::Mutex<Vec<Option<String>>>>,
        std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        use std::sync::{Arc, Mutex};

        let started = Arc::new(AtomicUsize::new(0));
        let finished = Arc::new(Mutex::new(Vec::new()));
        let cancelled = Arc::new(AtomicBool::new(false));
        let callbacks = super::RetryCallbacks {
            on_started: Box::new({
                let started = started.clone();
                move || {
                    started.fetch_add(1, Ordering::SeqCst);
                }
            }),
            on_finished: Box::new({
                let finished = finished.clone();
                move |outcome: Option<super::SelectedContext>| {
                    finished
                        .lock()
                        .unwrap()
                        .push(outcome.map(|context| context.text));
                }
            }),
            is_cancelled: Box::new({
                let cancelled = cancelled.clone();
                move || cancelled.load(Ordering::SeqCst)
            }),
        };
        return (callbacks, started, finished, cancelled);
    }

    fn plain_context(text: &str) -> super::SelectedContext {
        return super::SelectedContext {
            text: text.to_string(),
            word: None,
            word_offset: None,
        };
    }

    /// 重试轮询：命中即交付并释放槽位；超时给 None；on_finished 恰好一次
    #[test]
    fn retry_delivers_or_times_out_exactly_once() {
        use std::sync::atomic::Ordering;
        use std::time::Duration;

        let _lock = RETRY_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        super::RETRYING_PID.store(1, Ordering::SeqCst);
        let (callbacks, started, finished, _) = recording_callbacks();
        let mut polls = 0;
        super::run_selection_retry(callbacks, 5, Duration::ZERO, |attempt| {
            polls += 1;
            return (attempt == 2).then(|| plain_context("the sentence"));
        });
        assert_eq!(polls, 2);
        assert_eq!(started.load(Ordering::SeqCst), 1);
        assert_eq!(*finished.lock().unwrap(), vec![Some("the sentence".into())]);
        assert_eq!(super::RETRYING_PID.load(Ordering::SeqCst), 0);

        super::RETRYING_PID.store(1, Ordering::SeqCst);
        let (callbacks, _, finished, _) = recording_callbacks();
        super::run_selection_retry(callbacks, 3, Duration::ZERO, |_| None);
        assert_eq!(*finished.lock().unwrap(), vec![None]);
        assert_eq!(super::RETRYING_PID.load(Ordering::SeqCst), 0);
    }

    /// 重试过期：轮询前过期不再轮询；轮询途中过期丢弃已取到的结果；都恰好一次 None
    #[test]
    fn retry_stops_when_cancelled() {
        use std::sync::atomic::Ordering;
        use std::time::Duration;

        let _lock = RETRY_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        super::RETRYING_PID.store(1, Ordering::SeqCst);
        let (callbacks, _, finished, cancelled) = recording_callbacks();
        cancelled.store(true, Ordering::SeqCst);
        let mut polls = 0;
        super::run_selection_retry(callbacks, 5, Duration::ZERO, |_| {
            polls += 1;
            return None;
        });
        assert_eq!(polls, 0);
        assert_eq!(*finished.lock().unwrap(), vec![None]);
        assert_eq!(super::RETRYING_PID.load(Ordering::SeqCst), 0);

        super::RETRYING_PID.store(1, Ordering::SeqCst);
        let (callbacks, _, finished, cancelled) = recording_callbacks();
        super::run_selection_retry(callbacks, 5, Duration::ZERO, |_| {
            cancelled.store(true, Ordering::SeqCst);
            return Some(plain_context("stale sentence"));
        });
        assert_eq!(*finished.lock().unwrap(), vec![None]);
        assert_eq!(super::RETRYING_PID.load(Ordering::SeqCst), 0);
    }

    /// 轮询 panic：槽位照样释放，on_finished(None) 恰好一次
    #[test]
    fn retry_finishes_when_polling_panics() {
        use std::sync::atomic::Ordering;
        use std::time::Duration;

        let _lock = RETRY_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        super::RETRYING_PID.store(1, Ordering::SeqCst);
        let (callbacks, _, finished, _) = recording_callbacks();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            super::run_selection_retry(callbacks, 5, Duration::ZERO, |_| {
                panic!("polling failed");
            });
        }));
        assert!(result.is_err());
        assert_eq!(*finished.lock().unwrap(), vec![None]);
        assert_eq!(super::RETRYING_PID.load(Ordering::SeqCst), 0);
    }

    /// 收尾守卫：Drop 不会重复释放——槽位此时可能已被下一个重试占用
    #[test]
    fn retry_guard_does_not_release_the_next_retrys_slot() {
        use std::sync::atomic::Ordering;

        let _lock = RETRY_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        super::RETRYING_PID.store(1, Ordering::SeqCst);
        let mut guard = super::RetryFinishGuard {
            on_finished: Some(Box::new(|_| {})),
        };
        guard.finish(None);
        super::RETRYING_PID.store(2, Ordering::SeqCst);
        drop(guard);
        assert_eq!(super::RETRYING_PID.load(Ordering::SeqCst), 2);
        super::RETRYING_PID.store(0, Ordering::SeqCst);
    }

    /// 占用槽位：等到在途重试退出后接手；等待中本次已过期则放弃
    #[test]
    fn retry_slot_waits_for_the_stale_retry() {
        use std::sync::atomic::Ordering;
        use std::time::Duration;

        let _lock = RETRY_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        assert!(super::acquire_retry_slot(7, &|| false));
        assert_eq!(super::RETRYING_PID.load(Ordering::SeqCst), 7);

        // 已被占用且本次过期：放弃，不动别人的槽位
        assert!(!super::acquire_retry_slot(8, &|| true));
        assert_eq!(super::RETRYING_PID.load(Ordering::SeqCst), 7);

        // 已被占用、未过期：等旧重试释放后接手
        let releaser = std::thread::spawn(|| {
            std::thread::sleep(Duration::from_millis(30));
            super::RETRYING_PID.store(0, Ordering::SeqCst);
        });
        assert!(super::acquire_retry_slot(9, &|| false));
        assert_eq!(super::RETRYING_PID.load(Ordering::SeqCst), 9);
        releaser.join().unwrap();
        super::RETRYING_PID.store(0, Ordering::SeqCst);
    }
}
