
// 初始化 tokio 运行时，读取 config.toml，创建 UpstreamManager，启动 Server

use clap::Parser;
use std::fs::OpenOptions;
use tracing::{debug, error, info, span, trace, warn, Level};
use rusty_proxy::config::Config;
use tracing_subscriber::{
    fmt,
    prelude::*, // 必须导入，否则 .with() 方法不可用
    EnvFilter,
    Registry,
};
#[tokio::main]
pub async fn main() -> rusty_proxy::Result<()> {
    set_up_logging()?;
    // 从config.toml 中读取默认信息
    let config = Config::from_file("config.toml")?;

    info!("应用: {} v{}", config.app_name, config.version);
    info!("应用: {} v{}", config.app_name, config.version);
    info!("监听: {}:{}", config.server.host, config.server.port);

    for proxy in &config.proxies {
        info!("代理 [{}] -> {} (超时 {}s)",
                 proxy.name, proxy.target, proxy.timeout);
    }

    // 从命令行中读取启动信息
    let cli = Cli::parse();
    // 解析启动信息
    let host = cli.host.unwrap_or(config.server.host);
    let port = cli.port.unwrap_or(config.server.port);

    info!("最终host和port是: {}和{}", host,port);

    bytes_example();
    info!("use bytes crate, ok!");
    tracing_example();

    // // Bind a TCP listener
    // let listener = TcpListener::bind(&format!("127.0.0.1:{}", port)).await?;
    //
    // server::run(listener, signal::ctrl_c()).await;

    Ok(())
}
// #[command(name = "rusty_proxy-server", version, author, about = "A  server")]
#[derive(Parser, Debug)]
#[command(name = "rusty_proxy-server",  about = "A Reverse Proxy Server")]
struct Cli {
    #[arg(long)]
    host:Option<String>,
    #[arg(long)]
    port: Option<u16>,
}


fn set_up_logging() -> rusty_proxy::Result<()> {

    // 1. 创建app.log日志文件
    let log_file = OpenOptions::new()
        .create(true)
        .write(true)
        .append(true)
        .open("app.log").expect("Could not open log file");

    // 2. 创建控制台 Layer
    // 可以单独配置控制台的格式，例如使用 pretty 模式
    let console_layer = fmt::layer()
        .with_target(true)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .pretty();

    // 3. 创建文件 Layer
    // 将输出重定向到文件，通常文件日志不需要太花哨的格式，或者使用 JSON
    let file_layer = fmt::layer()
        .with_target(true)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .with_writer(log_file); // 关键：指定写入文件

    // 4. 创建过滤层 (EnvFilter)
    // 允许通过 RUST_LOG 环境变量控制日志级别
    let filter_layer = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "info,my_crate=debug".into());

    // 5. 组装并初始化
    // 使用 Registry 作为基础，挂载所有 Layer
    Registry::default()
        .with(filter_layer)
        .with(console_layer)
        .with(file_layer)
        .init();

    // --- 测试日志 ---
    tracing::info!("程序启动，日志将同时输出到控制台和 app.log 文件");
    Ok(())
}
use bytes::{Bytes, BytesMut, Buf, BufMut};
fn bytes_example() {
    // 1. 从静态字符串或 Vec 创建 (零拷贝)
    let b1 = Bytes::from("Hello, World!");
    let b2 = Bytes::from(vec![1, 2, 3]);

    // 2. 廉价克隆 (共享底层数据)
    let b1_clone = b1.clone();
    // 此时 b1 和 b1_clone 指向同一块内存，引用计数 +1
    println!("{:?}", b1_clone);
    // 3. 切片 (不复制数据)
    let slice = b1.slice(0..5); // "Hello"
    println!("{:?}", slice);
    // 4. 可变缓冲区构建
    let mut buf = BytesMut::with_capacity(1024);
    buf.put_slice(b"Hello");
    buf.put_u8(33); // '!'

    // 冻结为不可变，以便在网络中发送
    let frozen: Bytes = buf.freeze();

    // 5. 转回 Vec (如果需要与旧 API 交互，这会触发复制)
    let vec: Vec<u8> = frozen.to_vec();
    println!("{:?}", vec);
    // 6. 转为字符串 (如果数据是 UTF-8)
    let s = std::str::from_utf8(&frozen).unwrap();
    println!("{:?}", s);
}
fn tracing_example(){
    test_trace_level();
    test_debug_level();
    test_info_level();
    test_warn_level();
    test_error_level();
}






// ==================== TRACE 级别 ====================
/// **TRACE 级别**
/// - 含义：最详细的追踪信息
/// - 场景：函数入口/出口、循环迭代、变量中间值、性能剖析
/// - 生产环境：通常关闭
fn test_trace_level() {
    let _span = span!(Level::TRACE, "trace_test").entered();

    trace!("🔍 [TRACE] 最详细的追踪日志");
    trace!("🔍 [TRACE] 场景：函数入口、循环内部、变量快照");
    trace!("🔍 [TRACE] 示例：循环第 {} 次迭代，当前值 = {}", 1, 42);
    trace!("🔍 [TRACE] 生产环境建议：关闭 (噪音太大)");
}

// ==================== DEBUG 级别 ====================
/// **DEBUG 级别**
/// - 含义：调试信息
/// - 场景：开发调试、参数验证、分支逻辑、中间状态
/// - 生产环境：排查问题时临时开启
fn test_debug_level() {
    let _span = span!(Level::DEBUG, "debug_test").entered();

    debug!("🐛 [DEBUG] 调试级别日志");
    debug!("🐛 [DEBUG] 场景：开发调试、参数检查、逻辑分支");
    debug!("🐛 [DEBUG] 示例：用户输入参数 user_id={}, action={}", 123, "login");
    debug!("🐛 [DEBUG] 生产环境建议：排查问题时临时开启");
}

// ==================== INFO 级别 ====================
/// **INFO 级别**
/// - 含义：一般运行信息
/// - 场景：服务启动/停止、请求完成、定时任务、关键业务流程
/// - 生产环境：默认开启
fn test_info_level() {
    let _span = span!(Level::INFO, "info_test").entered();

    info!("ℹ️  [INFO] 信息级别日志");
    info!("ℹ️  [INFO] 场景：服务启停、请求处理、业务关键节点");
    info!("ℹ️  [INFO] 示例：HTTP 请求处理完成，耗时 {}ms", 150);
    info!("ℹ️  [INFO] 生产环境建议：默认开启 (默认级别)");
}

// ==================== WARN 级别 ====================
/// **WARN 级别**
/// - 含义：警告信息
/// - 场景：非致命错误、降级处理、重试成功、配置缺失
/// - 生产环境：开启 (需要关注)
fn test_warn_level() {
    let _span = span!(Level::WARN, "warn_test").entered();

    warn!("⚠️  [WARN] 警告级别日志");
    warn!("⚠️  [WARN] 场景：非致命错误、自动降级、重试成功");
    warn!("⚠️  [WARN] 示例：数据库连接超时，已切换到备用节点");
    warn!("⚠️  [WARN] 生产环境建议：开启 (需要关注但无需立即处理)");
}

// ==================== ERROR 级别 ====================
/// **ERROR 级别**
/// - 含义：错误信息
/// - 场景：操作失败、需要人工介入、系统异常、数据损坏
/// - 生产环境：必须开启 (需要报警)
fn test_error_level() {
    let _span = span!(Level::ERROR, "error_test").entered();

    error!("❌ [ERROR] 错误级别日志");
    error!("❌ [ERROR] 场景：操作失败、系统异常、需要人工介入");
    error!("❌ [ERROR] 示例：数据库连接失败，错误码={}", "CONNECTION_REFUSED");
    error!("❌ [ERROR] 生产环境建议：必须开启 (需要报警和监控)");

    // 演示错误链 (配合 anyhow)
    let err = anyhow::anyhow!("模拟一个业务错误");
    error!("❌ [ERROR] 错误链示例：{:?}", err);
}

// ==================== Span 跨度测试 ====================
/// **Span 测试**
/// 演示不同级别的 Span 如何影响日志输出
fn test_span_with_level() {
    info!("📊 开始测试 Span 跨度");

    // INFO 级别的 Span (默认级别下可见)
    {
        let _span = span!(Level::INFO, "info_span", operation = "database_query").entered();
        info!("  在 INFO Span 内的日志");
        debug!("  在 INFO Span 内的 DEBUG 日志 (需要 DEBUG 级别才可见)");
    }

    // DEBUG 级别的 Span (需要 DEBUG 级别才可见)
    {
        let _span = span!(Level::DEBUG, "debug_span", operation = "cache_lookup").entered();
        debug!("  在 DEBUG Span 内的日志");
        trace!("  在 DEBUG Span 内的 TRACE 日志");
    }

    info!("📊 Span 测试完成");
}

// ==================== 结构化字段测试 ====================
/// **结构化字段测试**
/// 演示 tracing 的结构化日志能力
fn test_structured_fields() {
    info!("📝 开始测试结构化字段");

    // 基本字段
    info!(
        user_id = 12345,
        action = "login",
        "用户登录成功"
    );

    // 带类型的字段
    info!(
        duration_ms = 150,
        success = true,
        status_code = 200,
        "HTTP 请求完成"
    );

    // 使用 % 格式化 (Display trait)
    let path = "/api/users";
    info!(path = %path, "访问路径");


    info!("📝 结构化字段测试完成");
}

