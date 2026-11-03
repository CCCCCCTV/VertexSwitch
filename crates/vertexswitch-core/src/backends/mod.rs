//! 上游后端：只负责 URL、请求头、请求体的构造（SPEC §3），不发请求。

pub mod claude;
pub mod gemini;
pub mod vertex;
