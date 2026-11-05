//! 外壳自己的文字（托盘菜单与提示、少量报错）跟随界面语言。
//!
//! 界面启动和切换语言时通过 `set_language` 告诉这里，同时记在配置目录里，
//! 下次启动时托盘一出现就是对的语言。核心的日志与错误信息不在此列。

use std::sync::atomic::{AtomicBool, Ordering};

use vertexswitch_core::config::app_dir;

static ENGLISH: AtomicBool = AtomicBool::new(false);
const FILE: &str = "language";

/// 启动时读回上次的界面语言（没有记录时用中文，界面加载后会马上更正）。
pub fn load() {
    if let Ok(saved) = std::fs::read_to_string(app_dir().join(FILE)) {
        ENGLISH.store(saved.trim() == "en", Ordering::Relaxed);
    }
}

/// 界面告知当前语言（"zh" 或 "en"）；与上次不同时记下来并返回 true。
pub fn set(lang: &str) -> Result<bool, String> {
    let english = match lang {
        "zh" => false,
        "en" => true,
        other => return Err(format!("unknown language: {other}")),
    };
    let path = app_dir().join(FILE);
    let changed = ENGLISH.swap(english, Ordering::Relaxed) != english;
    if changed || !path.is_file() {
        // 记不下来也不影响本次运行
        let _ = std::fs::create_dir_all(app_dir());
        let _ = std::fs::write(&path, lang);
    }
    Ok(changed)
}

pub fn english() -> bool {
    ENGLISH.load(Ordering::Relaxed)
}

/// 按当前界面语言二选一。
pub fn tr<'a>(zh: &'a str, en: &'a str) -> &'a str {
    if english() { en } else { zh }
}
