// 上游管理：存储服务器列表，实现轮询算法
// 调度员
// 负责：维护一个上游地址列表 Vec<String>。
// 关键点：提供一个 get_next_upstream() 方法，内部用原子计数器实现轮询。需要线程安全 (Arc + Mutex 或 AtomicUsize)。