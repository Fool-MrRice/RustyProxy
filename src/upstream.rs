use std::sync::atomic::{AtomicUsize, Ordering};

// 上游管理：存储服务器列表，实现轮询算法
// 调度员
// 负责：维护一个上游地址列表 Vec<Proxy>。
// 关键点：提供一个 get_next_upstream() 方法，内部用原子计数器实现轮询。需要线程安全 (Arc + Mutex 或 AtomicUsize)。
pub struct Manager{
    addresses: Vec<String>,
    counter: AtomicUsize,  // 原子计数器，无锁
}

impl Manager {
    pub fn new(addresses: Vec<String>) -> Self {
        Self {
            addresses,
            counter: AtomicUsize::new(0),
        }
    }

    pub fn get_next(&self) -> Option<&str> {
        if self.addresses.is_empty() {
            return None;
        }
        // 原子递增 + 取模 = 无锁轮询
        let idx = self.counter.fetch_add(1, Ordering::Relaxed) % self.addresses.len();
        Some(&self.addresses[idx])
    }
}