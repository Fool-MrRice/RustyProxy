// 核心逻辑：转发请求到上游，接收响应
// 负责：最核心的逻辑。
// 流程：读取客户端请求 -> 修改 Host 头 -> 发给上游 -> 读取上游响应 -> 返回给客户端。

use std::sync::Arc;
use bytes::Bytes;
use http_body_util::combinators::BoxBody;

use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use crate::upstream;

// ✅ 独立的转发函数，类型清晰
pub async fn do_proxy(
    req: hyper::Request<hyper::body::Incoming>,  // Request<Incoming>
    client: Arc<Client<HttpConnector, BoxBody<Bytes, hyper::Error>>>,  // ✅ 改为 BoxBody
    // client: Arc<Client<HttpConnector, Full<Bytes>>>,
    upstream_mgr: Arc<upstream::Manager>,
) -> crate::Result<hyper::Response<hyper::body::Incoming>> {  // 返回自定义错误类型

    // 🔹 步骤 1: 选择上游（不变）
    let upstream_addr = upstream_mgr
        .get_next()
        .ok_or_else(|| crate::error::MyLibError::NoUpstream)?;

    tracing::info!(
        "📤 转发请求: {} {} → {}",
        req.method(),
        req.uri(),
        upstream_addr
    );

    // 🔹 步骤 2: 构建转发请求（不变）
    let mut proxied_req = hyper::Request::builder()
        .method(req.method())
        .uri(build_upstream_uri(&upstream_addr, req.uri()))
        .version(req.version());

    for (name, value) in req.headers() {
        if name != "host" && name != "connection" {
            proxied_req = proxied_req.header(name, value);
        }
    }

    let upstream_host = extract_host(&upstream_addr)?;
    proxied_req = proxied_req.header("host", upstream_host);

    // 🔹 步骤 3: 转发请求体（关键修改！）
    // ✅ 使用 .boxed() 将 Incoming 转换为 BoxBody
    use http_body_util::BodyExt;  // 需要这个 trait 才能调用 .boxed()

    let proxied_req = proxied_req.body(
        req.into_body().boxed()  // ✅ Incoming -> BoxBody
    )?;

    // 🔹 步骤 4: 发送到上游
    let upstream_resp = client
        .request(proxied_req)
        .await
        .map_err(crate::error::MyLibError::UpstreamRequest)?;

    // 🔹 步骤 5-6: 构建并返回响应（不变，Incoming 可以直接透传）
    let mut resp_builder = hyper::Response::builder()
        .status(upstream_resp.status())
        .version(upstream_resp.version());

    for (name, value) in upstream_resp.headers() {
        resp_builder = resp_builder.header(name, value);
    }

    let response = resp_builder.body(upstream_resp.into_body())?;
    Ok(response)
}
// 构建上游 URI: "http://127.0.0.1:8081" + "/api/users" → "http://127.0.0.1:8081/api/users"
fn build_upstream_uri(upstream: &str, original_uri: &hyper::Uri) -> String {
    format!("{}{}", upstream, original_uri.path_and_query().map(|x| x.as_str()).unwrap_or(""))
}

// 提取 host: "http://127.0.0.1:8081" → "127.0.0.1:8081"
fn extract_host(url: &str) -> crate::Result<String> {
    // 简单实现：字符串解析（生产环境建议用 url crate）
    let without_proto = url.trim_start_matches("http://").trim_start_matches("https://");
    Ok(without_proto.split('/').next().unwrap_or(without_proto).to_string())
}