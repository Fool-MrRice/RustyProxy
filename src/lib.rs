// 库导出 (方便测试)

mod upstream;
pub mod server;
mod proxy;
mod error;
pub mod config;
pub use config::Config;
use crate::error::MyLibError;

pub type Error=Box<dyn std::error::Error + Send + Sync + 'static>;
pub type Result<T> = anyhow::Result<T, MyLibError>;