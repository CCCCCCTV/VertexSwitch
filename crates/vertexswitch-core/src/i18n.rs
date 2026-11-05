//! 核心与外壳产生的文字（日志、错误信息、托盘菜单）跟随界面语言：中文或英文。
//!
//! 界面启动和切换语言时经外壳告诉这里，同时记在配置目录的 `language` 文件里，
//! 下次启动（包括无界面运行的 `vertexswitch-core`）一开始就是对的语言。
//! 已经写下的日志和请求记录保持当时的语言。

use std::sync::atomic::{AtomicBool, Ordering};

use crate::config::app_dir;

static ENGLISH: AtomicBool = AtomicBool::new(false);
const FILE: &str = "language";

/// 启动时读回上次的界面语言（没有记录时用中文）。
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
    let changed = use_english(english);
    let path = app_dir().join(FILE);
    if changed || !path.is_file() {
        // 记不下来也不影响本次运行
        let _ = std::fs::create_dir_all(app_dir());
        let _ = std::fs::write(&path, lang);
    }
    Ok(changed)
}

/// 只改本次运行的语言，不写文件；返回是否有变化。
pub fn use_english(english: bool) -> bool {
    ENGLISH.swap(english, Ordering::Relaxed) != english
}

pub fn english() -> bool {
    ENGLISH.load(Ordering::Relaxed)
}

/// 按当前语言二选一（不需要格式化时用）。
pub fn tr<'a>(zh: &'a str, en: &'a str) -> &'a str {
    if english() { en } else { zh }
}

/// 按当前语言选格式串并格式化，得到 `String`：
/// `tr!("未知模型 {id:?}", "Unknown model {id:?}")`，参数写法同 `format!`。
#[macro_export]
macro_rules! tr {
    ($zh:literal, $en:literal $(,)?) => {
        if $crate::i18n::english() {
            ::std::format!($en)
        } else {
            ::std::format!($zh)
        }
    };
    ($zh:literal, $en:literal, $($arg:tt)+) => {
        if $crate::i18n::english() {
            ::std::format!($en, $($arg)+)
        } else {
            ::std::format!($zh, $($arg)+)
        }
    };
}
