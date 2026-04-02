// 服务端：监听端口，接受 TCP 连接
// 负责：绑定 TCP 端口 (如 8080)，无限循环 accept 新连接。
// 关键点：每接受一个连接，就 tokio::spawn 一个新任务去处理，实现并发。

use std::sync::Arc;
use std::time::Duration;
use bytes::Bytes;
use http_body_util::Full;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::{TokioExecutor, TokioIo};
use tokio::net::{TcpListener, TcpStream};
use tokio::pin;
use tokio::task::JoinSet;
use tracing::{info, warn};

pub async fn run(listener: TcpListener, shutdown: impl Future<Output = ()> + Send + 'static) -> crate::Result<()> {

    pin!(shutdown);
    let client: Client<_, Full<Bytes>> = Client::builder(TokioExecutor::new()) .build_http();
    let client = Arc::new(client);


    // ─────────────────────────────────────────────────────
    // 3️⃣ 用 JoinSet 跟踪所有连接任务（优雅关闭的关键）
    // ─────────────────────────────────────────────────────
    // 直接 tokio::spawn 的任务无法等待完成
    // JoinSet 允许我们:
    // - 批量生成任务
    // - 等待所有任务完成 (join_next)
    // - 超时强制终止 (abort_all)
    let mut connections = JoinSet::new();

    // ─────────────────────────────────────────────────────
    // 4️⃣ 主事件循环：同时监听新连接 / 关闭信号
    // ─────────────────────────────────────────────────────
    loop {
        tokio::select! {
            // ─────────────────────────────────────
            // 分支 A: 接受新客户端连接
            // ─────────────────────────────────────
            accept_result = listener.accept() => {
                // ? 操作符: 如果 accept 出错（如监听器关闭），直接返回错误
                // 要求函数返回类型是 Result<_, std::io::Error>
                let (stream, addr) = accept_result?;
                info!("📡 New connection from {}", addr);

                // 包装 TcpStream 为 TokioIo，使其兼容 hyper 的 IO trait
                let io = TokioIo::new(stream);

                // 克隆 Arc 指针（浅拷贝），每个任务持有独立引用
                let client = Arc::clone(&client);

                // 🚀 为每个连接 spawn 独立异步任务
                // - async move: 捕获 io/client 并移动到子任务
                // - 任务与主循环并发执行，避免阻塞 accept
                // - 错误在任务内记录，不影响主循环和其他连接
                connections.spawn(async move {
                    if let Err(e) = handle_each_connection(io, client).await {
                        warn!("❌ Connection {} error: {}", addr, e);
                    }
                    // 任务结束 → 连接自动关闭（Drop 触发）
                });
            }

            // ─────────────────────────────────────
            // 分支 B: 收到关闭信号（修复版）
            // ─────────────────────────────────────
            // ✅ 正确: &mut shutdown 是 Pin<&mut Future> 的可变引用
            //     可以被 select! 内部 poll 推进
            // ❌ 错误: &shutdown 是 &Future，不实现 Future trait
            _ = &mut shutdown => {
                info!("🛑 Shutdown signal received, stopping accept loop");
                break;  // 退出循环，不再接受新连接
                // ⚠️ 注意: 此时已有连接任务仍在后台运行，需要等待它们完成
            }
        }
    }

    // ─────────────────────────────────────────────────────
    // 5️⃣ 优雅关闭：等待已有连接处理完成（带超时保护）
    // ─────────────────────────────────────────────────────
    info!("🔄 Draining {} active connections...", connections.len());

    // 创建 30 秒超时，避免无限等待（如连接卡死）
    let drain_timeout = tokio::time::sleep(Duration::from_secs(30));
    pin!(drain_timeout);  // 同样需要 pin 住 timeout future

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
                warn!("⏰ Drain timeout (30s), forcing shutdown of {} remaining connections",
                      connections.len());
                // 强制终止所有剩余任务（会触发任务内 Drop 清理资源）
                connections.abort_all();
                break;
            }
        }
    }

    // ─────────────────────────────────────────────────────
    // 6️⃣ 返回成功
    // ─────────────────────────────────────────────────────
    // - listener 自动 Drop → 关闭监听 socket
    // - client (Arc) 引用计数归零 → 连接池清理
    // - 所有连接任务已完成或被终止 → 资源安全释放
    Ok(())
    // //   创建全局客户端（连接池）
    // let client: Client<_, Full<Bytes>> = Client::builder(TokioExecutor::new())
    //     .build_http();
    // let client = Arc::new(client);  //  用 Arc 包装，方便多任务共享
    //
    // //  用 tokio::select! 同时监听 新连接 / 关闭信号
    // loop {
    //     tokio::select! {
    //         // 接受新连接
    //         accept_result = listener.accept() => {
    //             let (stream, addr) = accept_result?;
    //             info!("New connection from {}", addr);
    //
    //             let io = TokioIo::new(stream);
    //             let client = Arc::clone(&client);
    //
    //             //  spawn 任务处理连接
    //             tokio::task::spawn(async move {
    //                 if let Err(e) = handle_each_connection(io, client).await {
    //                     warn!("Connection {} error: {}", addr, e);
    //                 }
    //             });
    //         }
    //
    //         // 收到关闭信号
    //         _ = &shutdown => {
    //             info!("Shutdown signal received, stopping accept loop");
    //             break;
    //              //todo!
    //             //优雅关闭带实现
    //         }
    //     }
    // }
    //
    // Ok(())
}

async fn handle_each_connection(_io: TokioIo<TcpStream>, _client: Arc<Client<HttpConnector, Full<Bytes>>>)-> crate::Result<()> {
    todo!()
}
