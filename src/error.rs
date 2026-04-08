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

    #[error("config load 失败")]
    ConfigLoadError(#[from] toml::de::Error),
    
    #[error("地址解析失败: {0}")]
    AddrParse(#[from] std::net::AddrParseError),

    #[error("hyper失败")]
    HyperError(#[from] hyper::Error),

    #[error("No upstream server available")]
    NoUpstream,

    #[error("Upstream request failed: {0}")]
    UpstreamRequest(#[from] hyper_util::client::legacy::Error),

    #[error("HTTP build failed: {0}")]
    HttpBuild(#[from] http::Error),



    // 如果不需要自动 From，只想记录来源
    #[error("未知错误")]
    Unknown(#[source] Box<dyn std::error::Error + Send + Sync+'static>),
}
