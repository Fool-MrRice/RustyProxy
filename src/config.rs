// 配置加载：读取 config.toml
// 负责：定义配置结构体，用 serde 解析文件

use serde::Deserialize;
use std::fs;
use std::path::Path;

// 定义数据结构（与 TOML 结构对应）
#[derive(Debug, Deserialize)]
pub struct Config {
    pub app_name: String,
    pub version: String,
    pub debug: bool,

    #[serde(default)]  // 提供默认值，防止字段缺失报错
    pub server: ServerConfig,

    #[serde(default)]
    pub proxies: Vec<Proxy>,
}

#[derive(Debug, Deserialize, Default)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_workers")]
    pub workers: usize,
}

#[derive(Debug, Deserialize)]
pub struct Proxy {
    pub name: String,
    pub target: String,
    #[serde(default = "default_timeout")]
    pub timeout: u64,
}


// 默认值函数
fn default_host() -> String {
    // 默认只接受本地请求
    "127.0.0.1".to_string() }
fn default_port() -> u16 { 8080 }
fn default_workers() -> usize { num_cpus::get() }
fn default_timeout() -> u64 { 30 }


impl Config {
    /// 从文件加载配置
    pub fn from_file<P: AsRef<Path>>(path: P) -> crate::Result<Self> {
        let content = fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }
    pub fn from_file_get_proxies_string<P: AsRef<Path>>(path: P) -> crate::Result<Vec<String>> {
        let content = fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        let vec_proxies_string:Vec<String> =
            config
                .proxies
                .into_iter()
                .map(|p| p.target)
                .collect();

        Ok(vec_proxies_string)
    }
}