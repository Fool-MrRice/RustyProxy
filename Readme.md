# RustyProxy - 异步 HTTP 反向代理

基于 Rust + Tokio + Hyper 构建的高性能 HTTP 反向代理服务器，支持多上游轮询负载均衡。

***

## 目录

- [架构概览](#架构概览)
- [模块职责与调用关系](#模块职责与调用关系)
- [快速开始](#快速开始)
- [配置说明](#配置说明)
- [开发指南](#开发指南)
- [故障排查](#故障排查)

***

## 架构概览

### 程序运行流程图

```
客户端 (Browser/cURL)
        │
        │ HTTP Request
        ▼
┌──────────────────────────────────────────────────────────┐
│                    TcpListener (8080)                     │
│                      监听新连接                            │
└──────────────────────┬───────────────────────────────────┘
                       │ accept()
                       ▼
┌──────────────────────────────────────────────────────────┐
│              JoinSet (连接任务池)                          │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐       │
│  │ Connection 1│  │ Connection 2│  │ Connection N│  ...  │
│  │  (async task)│  │  (async task)│  │  (async task)│       │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘       │
│         │                │                │               │
│         ▼                ▼                ▼               │
│  ┌─────────────────────────────────────────────────┐     │
│  │          handle_each_connection()                │     │
│  │  ┌───────────────────────────────────────────┐  │     │
│  │  │  http1::serve_connection()                │  │     │
│  │  │  └─ service_fn(|req|)                     │  │     │
│  │  │     └─ async move { do_proxy() }          │  │     │
│  │  └───────────────────────────────────────────┘  │     │
│  └──────────────────────┬──────────────────────────┘     │
│                         │                                │
│                         ▼                                │
│              ┌────────────────────┐                      │
│              │   do_proxy()       │                      │
│              │  1. 选择上游        │                      │
│              │  2. 构建请求        │                      │
│              │  3. 转发请求体      │                      │
│              │  4. 发送上游        │                      │
│              │  5. 返回响应        │                      │
│              └────────┬───────────┘                      │
│                       │                                  │
└───────────────────────┼──────────────────────────────────┘
                        │
                        ▼
              ┌─────────────────────┐
              │  upstream::Manager   │
              │  (轮询调度器)         │
              │  get_next() → addr   │
              └────────┬────────────┘
                       │
                       ▼
              ┌─────────────────────┐
              │   上游服务器         │
              │   http://localhost  │
              │   :3000 / :3001     │
              └─────────────────────┘
```

### 并发模型

```
主任务 (run)
├── tokio::select! 循环
│   ├── listener.accept()  → 新连接到达
│   │   └── connections.spawn(handle_and_log(...))
│   │       └── 独立异步任务，并发处理
│   └── shutdown signal    → 优雅关闭
│       ├── 停止 accept
│       ├── 等待已有任务完成 (join_next)
│       └── 30s 超时强制终止 (abort_all)
```

***

## 模块职责与调用关系

### 文件结构

```
src/
├── bin/
│   ├── server.rs          # 程序入口 (main)，初始化运行时、日志、配置
│   └── client.rs          # (占位，待实现)
├── lib.rs                 # 库根，导出所有模块
├── config.rs              # 配置加载与解析
├── upstream.rs            # 上游服务器管理 + 轮询调度
├── server.rs              # TCP 监听 + 连接管理 + 优雅关闭
├── proxy.rs               # 核心代理逻辑 (请求转发)
└── error.rs               # 统一错误定义
```

### 模块详细职责

#### `bin/server.rs` — 程序入口

**职责**：组装所有组件，启动服务。

**执行流程**：

1. 初始化 tracing 日志系统（控制台 + 文件双输出）
2. 解析 CLI 参数（`--host`、`--port`）
3. 加载 `config.toml` 配置
4. 绑定 TCP 监听器
5. 创建 `upstream::Manager`（上游服务器列表）
6. 调用 `server::run()` 进入主事件循环
7. 监听 `Ctrl+C` 信号触发优雅关闭

***

#### `config.rs` — 配置层

**职责**：定义配置结构体，从 TOML 文件加载配置。

**核心类型**：

| 类型             | 说明                                        |
| -------------- | ----------------------------------------- |
| `Config`       | 顶层配置（app\_name, version, server, proxies） |
| `ServerConfig` | 服务器配置（host, port, workers）                |
| `Proxy`        | 单个代理目标（name, target, timeout）             |

**核心方法**：

- `Config::from_file(path)` → 解析 TOML 返回 `Config`
- `Config::from_file_get_proxies_string(path)` → 提取所有 upstream 地址 `Vec<String>`

***

#### `upstream.rs` — 上游调度器

**职责**：管理上游服务器列表，提供轮询负载均衡。

**核心类型**：

| 类型        | 说明                 |
| --------- | ------------------ |
| `Manager` | 上游管理器，包含地址列表和原子计数器 |

**核心方法**：

- `Manager::new(addresses)` → 创建实例
- `Manager::get_next()` → 返回下一个上游地址（原子递增 + 取模 = 无锁轮询）

**线程安全**：使用 `AtomicUsize` 实现无锁轮询，无需 `Mutex`。

***

#### `server.rs` — 连接管理层

**职责**：监听 TCP 端口，接受连接，为每个连接 spawn 异步任务，实现优雅关闭。

**核心函数**：

| 函数                                                 | 说明                                       |
| -------------------------------------------------- | ---------------------------------------- |
| `run(listener, shutdown, upstream_manager)`        | 主事件循环，accept + select! + 优雅关闭            |
| `handle_each_connection(io, client, upstream_mgr)` | 处理单个 HTTP 连接（hyper serve\_connection）    |
| `handle_and_log(...)`                              | 辅助函数，包装 `handle_each_connection` 并记录错误日志 |

**关键设计**：

- 使用 `JoinSet` 跟踪所有连接任务（而非裸 `tokio::spawn`），支持等待完成
- `tokio::select!` 同时监听新连接和关闭信号
- 关闭时先停止 accept，再 `join_next` 等待已有任务，30s 超时强制终止

***

#### `proxy.rs` — 核心代理逻辑

**职责**：将客户端请求转发到上游服务器，并将上游响应透传回客户端。

**核心函数**：

| 函数                                    | 说明      |
| ------------------------------------- | ------- |
| `do_proxy(req, client, upstream_mgr)` | 完整的代理流程 |

**代理流程（6 步）**：

1. **选择上游**：`upstream_mgr.get_next()` 获取上游地址
2. **构建请求**：复制方法、URI、版本，转发除 `host` 和 `connection` 外的所有头
3. **设置 Host 头**：从上游地址提取 host 并设置
4. **转发请求体**：`req.into_body().boxed()` 零拷贝透传
5. **发送上游**：`client.request(proxied_req).await`
6. **返回响应**：复制状态码、版本、头，body 使用上游响应的 `Incoming` 流

***

#### `error.rs` — 错误定义

**职责**：统一项目错误类型。

**核心类型**：

| 变体                                                   | 说明             |
| ---------------------------------------------------- | -------------- |
| `MissingConfig(String)`                              | 配置字段缺失         |
| `Io(io::Error)`                                      | IO 错误（自动 From） |
| `ConfigLoadError(toml::de::Error)`                   | TOML 解析错误      |
| `AddrParse(AddrParseError)`                          | 地址解析错误         |
| `HyperError(hyper::Error)`                           | Hyper 错误       |
| `NoUpstream`                                         | 无可用上游服务器       |
| `UpstreamRequest(hyper_util::client::legacy::Error)` | 上游请求失败         |
| `HttpBuild(http::Error)`                             | HTTP 构建失败      |
| `Unknown(Box<dyn Error + Send + Sync>)`              | 未知错误           |

***

### 模块调用关系图

```
bin/server.rs (main)
    │
    ├──► config.rs
    │       └── Config::from_file()
    │       └── Config::from_file_get_proxies_string()
    │
    ├──► upstream.rs
    │       └── Manager::new(addresses)
    │
    └──► server.rs
            └── run(listener, shutdown, upstream_manager)
                    │
                    ├──► JoinSet.spawn(handle_and_log(...))
                    │       │
                    │       └──► handle_each_connection()
                    │               │
                    │               └──► http1::serve_connection()
                    │                       └── service_fn(|req|)
                    │                               └──► proxy.rs::do_proxy()
                    │                                       │
                    │                                       ├──► upstream_mgr.get_next()
                    │                                       └──► client.request()
                    │
                    └──► shutdown 信号处理
                            └── join_next() / abort_all()
```

***

## 快速开始

### 环境要求

| 工具    | 版本                   |
| ----- | -------------------- |
| Rust  | 1.85+ (Edition 2024) |
| Cargo | 最新稳定版                |

### 编译

```bash
cargo build
```

### 运行

**方式一：使用默认配置**

```bash
cargo run --bin rusty_proxy-server
```

**方式二：指定 host 和 port**

```bash
cargo run --bin rusty_proxy-server -- --host 0.0.0.0 --port 9090
```

命令行参数会覆盖 `config.toml` 中的 `server.host` 和 `server.port`。

### 验证

启动后，向代理发送请求：

```bash
# 假设上游服务运行在 localhost:3000
curl http://localhost:8080/api/test
```

### 停止

按 `Ctrl+C` 触发优雅关闭：

1. 停止接受新连接
2. 等待已有连接处理完成（最长 30 秒）
3. 超时后强制终止剩余连接

***

## 配置说明

### `config.toml`

```toml
# 基础配置
app_name = "rusty_proxy"
version = "0.1.0"
debug = true

# 服务器配置
[server]
host = "0.0.0.0"    # 0.0.0.0 接受所有地址，127.0.0.1 仅本地
port = 8080         # 监听端口
workers = 4         # 工作线程数（默认 = CPU 核心数）

# 代理目标（数组，支持多个上游）
[[proxies]]
name = "service_1"
target = "http://localhost:3000"
timeout = 30

[[proxies]]
name = "service_2"
target = "http://localhost:3001"
timeout = 60
```

### 配置字段说明

| 字段                  | 类型     | 默认值           | 说明             |
| ------------------- | ------ | ------------- | -------------- |
| `server.host`       | String | `"127.0.0.1"` | 绑定地址           |
| `server.port`       | u16    | `8080`        | 绑定端口           |
| `server.workers`    | usize  | CPU 核心数       | Tokio 工作线程数    |
| `proxies[].name`    | String | 必填            | 上游名称（仅用于日志）    |
| `proxies[].target`  | String | 必填            | 上游地址（必须带协议头）   |
| `proxies[].timeout` | u64    | `30`          | 超时秒数（当前未使用，预留） |

***

## 开发指南

### 添加新的上游服务器

编辑 `config.toml`，添加新的 `[[proxies]]` 块：

```toml
[[proxies]]
name = "service_3"
target = "http://localhost:3002"
timeout = 45
```

重启服务即可生效（当前不支持热重载配置）。

### 添加新的错误类型

在 `src/error.rs` 的 `MyLibError` 枚举中添加变体：

```rust
#[error("自定义错误描述")]
MyNewError(#[from] SomeOtherError),
```

使用 `#[from]` 属性可自动实现 `From` trait，支持 `?` 操作符。

### 修改代理逻辑

核心逻辑在 `src/proxy.rs::do_proxy()` 函数中。常见修改点：

| 需求       | 修改位置                             |
| -------- | -------------------------------- |
| 添加请求头    | 步骤 2 的 `for` 循环后                 |
| 修改响应头    | 步骤 5-6 的 `resp_builder` 构建中      |
| 负载均衡策略   | `src/upstream.rs::get_next()` 方法 |
| 请求/响应体转换 | 步骤 3 的 `.boxed()` 前后             |

### 日志级别控制

通过 `RUST_LOG` 环境变量控制日志级别：

```bash
# Windows PowerShell
$env:RUST_LOG="debug"
cargo run --bin rusty_proxy-server

# 仅查看特定模块
$env:RUST_LOG="rusty_proxy::proxy=debug,rusty_proxy::server=info"
```

日志同时输出到：

- **控制台**：带颜色、pretty 格式
- **文件**：`app.log`（追加模式）

### 运行测试

```bash
cargo test
```

当前 `tests/` 目录为空，待补充集成测试。

***

## 故障排查

### 编译错误

| 错误                                             | 原因                              | 解决                                     |
| ---------------------------------------------- | ------------------------------- | -------------------------------------- |
| `implementation of From is not general enough` | 异步闭包中 `Box<dyn Error>` 生命周期推导失败 | 使用 `anyhow::Error` 替代 `Box<dyn Error>` |
| `no method named with_upgrades`                | 缺少 `mut` 或 hyper 版本不匹配          | 确保 `http_conn` 可变，检查 `hyper` 版本        |

### 运行时问题

| 现象              | 可能原因           | 解决                                 |
| --------------- | -------------- | ---------------------------------- |
| 连接被拒绝           | 上游服务未启动        | 确认 `config.toml` 中的 upstream 地址可访问 |
| 502 Bad Gateway | 上游服务器不可达       | 检查网络连接和防火墙规则                       |
| 日志无输出           | `RUST_LOG` 未设置 | 设置 `RUST_LOG=info` 或删除环境变量使用默认值    |

### 性能调优

| 参数    | 调整方式                                        |
| ----- | ------------------------------------------- |
| 并发连接数 | Tokio 默认自适应，可通过 `TOKIO_WORKER_THREADS` 覆盖   |
| 缓冲区大小 | 修改 `bytes::BytesMut::with_capacity()` 参数    |
| 超时时间  | 调整 `server.rs` 中的 `Duration::from_secs(30)` |

***

## 技术栈

| 组件      | 库                            | 版本               |
| ------- | ---------------------------- | ---------------- |
| 异步运行时   | tokio                        | 1.50.0           |
| HTTP 协议 | hyper                        | 1.9.0            |
| HTTP 工具 | hyper-util                   | 0.1.20           |
| 配置解析    | toml + serde                 | 1.1.0 / 1.0.228  |
| 日志系统    | tracing + tracing-subscriber | 0.1.44 / 0.3.23  |
| 错误处理    | thiserror + anyhow           | 2.0.18 / 1.0.102 |
| CLI 解析  | clap                         | 4.6.0            |
| 字节处理    | bytes                        | 1.11.1           |

