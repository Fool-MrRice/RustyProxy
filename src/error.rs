// 错误定义：统一项目中的错误类型

use thiserror::Error;
use std::io;

// 1. 定义错误枚举
#[derive(Error, Debug)]
pub enum MyLibError {
    // 普通错误消息
    #[error("配置文件中缺少 {0} 字段")]
    MissingConfig(String),

    // 包装 std::io::Error，自动实现 From<io::Error>
    #[error("IO 操作失败")]
    Io(#[from] io::Error),

    #[error("IO 操作失败")]
    ConfigLoadError(#[from] toml::de::Error),


    // 如果不需要自动 From，只想记录来源
    #[error("未知错误")]
    Unknown(#[source] Box<dyn std::error::Error + Send + Sync>),
}

// // 2. 在库函数中使用
// pub fn read_config(path: &str) -> Result<String, MyLibError> {
//     // 不需要手动 map_error，#[from] 会自动处理 ? 转换
//     let content = std::fs::read_to_string(path)?;
//
//     if content.is_empty() {
//         // 返回自定义错误
//         return Err(MyLibError::MissingConfig("data".to_string()));
//     }
//
//     Ok(content)
// }