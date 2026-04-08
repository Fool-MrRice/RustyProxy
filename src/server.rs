// 服务端：监听端口，接受 TCP 连接
// 负责：绑定 TCP 端口 (如 8080)，无限循环 accept 新连接。
// 关键点：每接受一个连接，就 tokio::spawn 一个新任务去处理，实现并发。

use crate::upstream;
use crate::upstream::Manager;
use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::{TokioExecutor, TokioIo};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::pin;
use tokio::task::JoinSet;
use tracing::{info, warn};

pub async fn run(
    listener: TcpListener,
    shutdown: impl Future<Output = ()> + Send + 'static,
    upstream_manager: Manager,
) -> crate::Result<()> {
    info!("starting running server");
    pin!(shutdown);
    // client: 用于发送 HTTP 请求到上游服务器的客户端
    let client: Client<HttpConnector, BoxBody<Bytes, hyper::Error>> =
        Client::builder(TokioExecutor::new()).build(HttpConnector::new());

    // client: 用 Arc 包装（保持不变）：
    let client = Arc::new(client);
    // arc_upstream_manager: 用 Arc 包装（保持不变）：用于管理上游服务器连接
    let arc_upstream_manager = Arc::new(upstream_manager);

    // 用 JoinSet 跟踪所有连接任务（优雅关闭的关键）
    // 直接 tokio::spawn 的任务无法等待完成
    // JoinSet 允许我们:
    // - 批量生成任务
    // - 等待所有任务完成 (join_next)
    // - 超时强制终止 (abort_all)
    let mut connections = JoinSet::new();

    //  run主事件循环：同时监听新连接 / 关闭信号
    loop {
        tokio::select! {
            // 分支 A: 接受新客户端连接
            // listener.accept() 是一个 Future，返回 Result<(TcpStream, SocketAddr), std::io::Error>
            //  accept_result? 操作符: 如果 accept 出错，直接返回错误
            accept_result = listener.accept() => {
                let (stream, addr) = accept_result?;
                info!("📡 New connection from {}", addr);

                // 包装 TcpStream 为 TokioIo，使其兼容 hyper 的 IO trait
                // 这是 hyper 用于处理 TCP 连接的 trait，需要一个 TokioIo 实现
                // 用于处理 I/O 操作（如读取请求、写入响应）的异步性
                // 是用于连接客户端的 TCP 流，确保异步操作的正确执行
                let io = TokioIo::new(stream);

                // 克隆 Arc 指针（浅拷贝），每个任务持有独立引用
                let arc_client = Arc::clone(&client);
                let arc_manager=Arc::clone(&arc_upstream_manager);

                // 为每个连接 spawn 独立异步任务
                // - async move: 捕获 io/client 并移动到子任务
                // - 任务与主循环并发执行，避免阻塞 accept
                // - 错误在任务内记录，不影响主循环和其他连接
                //
                // 🔧 修复 "implementation of `From` is not general enough" 错误：
                // 该错误源于 anyhow::Error 在异步闭包中的生命周期推导歧义。
                // 解决方法：将 handle_each_connection 的 Result 显式转换为 ()，
                // 避免编译器尝试推导错误类型的 From 实现。
                // 使用一个辅助 async 函数来包装，确保类型推导明确。
                connections.spawn(handle_and_log(io, arc_client, arc_manager, addr));
            }

            // 分支 B: 收到关闭信号
            _ = &mut shutdown => {
                info!("🛑 Shutdown signal received, stopping accept loop");
                break;  // 退出循环，不再接受新连接
                //  注意: 此时已有连接任务仍在后台运行，需要等待它们完成
            }
        }
    }

    // ─────────────────────────────────────────────────────
    // 5️⃣ 优雅关闭：等待已有连接处理完成（带超时保护）
    // ─────────────────────────────────────────────────────
    info!("🔄 Draining {} active connections...", connections.len());

    // 创建 20 秒超时，避免无限等待（如连接卡死）
    let drain_timeout = tokio::time::sleep(Duration::from_secs(20));
    pin!(drain_timeout); // 同样需要 pin 住 timeout future

    // 循环等待: 要么有连接完成，要么超时
    loop {
        tokio::select! {
            // 情况 1: 有连接任务完成
            Some(result) = connections.join_next() => {
                // 检查任务是否恐慌（panic）
                if let Err(e) = result {
                    if e.is_panic() {
                        warn!("⚠️  Connection task panicked: {:?}", e.into_panic());
                    } else {
                        warn!("⚠️  Connection task cancelled: {}", e);
                    }
                }
                // ✅ 所有连接处理完毕，可以安全退出
                if connections.is_empty() {
                    info!("✨ All connections drained, shutdown complete");
                    break;
                }
            }

            // 情况 2: 超时强制退出
            _ = &mut drain_timeout => {
                warn!("⏰ Drain timeout (20s), forcing shutdown of {} remaining connections",connections.len());
                // 强制终止所有剩余任务（会触发任务内 Drop 清理资源）
                connections.abort_all();
                break;
            }
        }
    }
    Ok(())
}

async fn handle_each_connection(
    io: TokioIo<TcpStream>,
    client: Arc<Client<HttpConnector, BoxBody<Bytes, hyper::Error>>>,
    upstream_mgr: Arc<upstream::Manager>,
) -> crate::Result<()> {
    //  步骤 1: 用 hyper 解析 HTTP 请求
    let http_conn = http1::Builder::new().serve_connection(
        io,
        //  关键：service_fn 的泛型推导
        // 输入: Request<Incoming> (server 接收的流式请求)
        // 输出: Result<Response<Full<Bytes>>, BoxError> (server 返回的完整响应)
        service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
            // 捕获外部变量（需要 clone 或 Arc）
            let client = Arc::clone(&client);
            let upstream_mgr = Arc::clone(&upstream_mgr);

            //  返回类型必须明确：Result<_, anyhow::Error>
            // 🔧 使用 anyhow::Error 而非 Box<dyn Error> 来避免生命周期推导问题
            // anyhow::Error 实现了 Send + 'static，且没有泛型生命周期参数
            async move {
                crate::proxy::do_proxy(req, client, upstream_mgr)
                    .await
                    .map_err(|e| -> anyhow::Error { e.into() })
            }
        }),
    );

    // 🔹 步骤 2: 等待连接处理完成
    if let Err(e) = http_conn.with_upgrades().await {
        tracing::warn!("⚠️ 连接处理出错：{}", e);
    }

    Ok(())
}

/// 🔧 辅助函数：包装 handle_each_connection 并处理错误日志
///
/// 这个函数存在的意义：
/// - 将 `crate::Result<()>`（即 `Result<(), MyLibError>`）显式转换为 `()`
/// - 避免在 `spawn` 闭包中直接进行错误处理导致的生命周期推导问题
/// - 提供一个类型明确的 async 函数，让编译器能正确推导 `Send + 'static` 约束
async fn handle_and_log(
    io: TokioIo<TcpStream>,
    client: Arc<Client<HttpConnector, BoxBody<Bytes, hyper::Error>>>,
    upstream_mgr: Arc<upstream::Manager>,
    addr: std::net::SocketAddr,
) {
    if let Err(e) = handle_each_connection(io, client, upstream_mgr).await {
        warn!("❌ Connection {} error: {}", addr, e);
    }
}
