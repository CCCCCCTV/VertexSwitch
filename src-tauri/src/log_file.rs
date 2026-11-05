//! 日志文件：配置目录下的 `logs\vertexswitch.log`，内容与日志页相同（INFO 及以上，不含请求 /
//! 响应正文）。正式版没有控制台，程序出问题后靠它排查。
//!
//! 超过 5 MB 时改名为 `vertexswitch.log.1`（只留一份旧的）再新开一个。写不进去时丢弃，不影响程序。

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use vertexswitch_core::config::app_dir;

const MAX_BYTES: u64 = 5 * 1024 * 1024;
const FILE_NAME: &str = "vertexswitch.log";

pub fn dir() -> PathBuf {
    app_dir().join("logs")
}

struct State {
    path: PathBuf,
    file: Option<File>,
    size: u64,
    limit: u64,
}

/// 可以克隆的写入端：每次写入都追加到同一个文件，tracing 的 fmt 层每条日志调用一次。
#[derive(Clone)]
pub struct LogFile(Arc<Mutex<State>>);

impl LogFile {
    pub fn open(dir: &Path) -> Self {
        Self::with_limit(dir, MAX_BYTES)
    }

    fn with_limit(dir: &Path, limit: u64) -> Self {
        let _ = std::fs::create_dir_all(dir);
        let path = dir.join(FILE_NAME);
        let file = open_append(&path);
        let size = file
            .as_ref()
            .and_then(|f| f.metadata().ok())
            .map_or(0, |m| m.len());
        Self(Arc::new(Mutex::new(State {
            path,
            file,
            size,
            limit,
        })))
    }
}

fn open_append(path: &Path) -> Option<File> {
    OpenOptions::new().create(true).append(true).open(path).ok()
}

impl Write for LogFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut state = self.0.lock().expect("锁未中毒");
        if state.size > 0 && state.size + buf.len() as u64 > state.limit {
            // 先关掉再改名：Windows 上打开着的文件改不了名
            state.file = None;
            let _ = std::fs::rename(&state.path, state.path.with_extension("log.1"));
            state.file = open_append(&state.path);
            state.size = 0;
        }
        if let Some(file) = state.file.as_mut()
            && file.write_all(buf).is_ok()
        {
            state.size += buf.len() as u64;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut state = self.0.lock().expect("锁未中毒");
        match state.file.as_mut() {
            Some(file) => file.flush(),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_and_rotates_once_over_limit() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = LogFile::with_limit(dir.path(), 20);
        log.write_all(b"first line\n").unwrap();
        log.write_all(b"second\n").unwrap();
        // 再写就超过 20 字节：前两行移到 .log.1，新文件从这一行开始
        log.write_all(b"third line\n").unwrap();
        log.flush().unwrap();
        let read = |name: &str| std::fs::read_to_string(dir.path().join(name)).unwrap();
        assert_eq!(read("vertexswitch.log.1"), "first line\nsecond\n");
        assert_eq!(read("vertexswitch.log"), "third line\n");

        // 再轮换一次只留一份旧的
        log.write_all(b"fourth line\n").unwrap();
        assert_eq!(read("vertexswitch.log.1"), "third line\n");
        assert_eq!(read("vertexswitch.log"), "fourth line\n");
    }

    #[test]
    fn reopening_continues_the_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        LogFile::open(dir.path()).write_all(b"a\n").unwrap();
        LogFile::open(dir.path()).write_all(b"b\n").unwrap();
        let text = std::fs::read_to_string(dir.path().join("vertexswitch.log")).unwrap();
        assert_eq!(text, "a\nb\n");
    }
}
