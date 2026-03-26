struct Config {
    host: String,
    port: u16,
    database: String,
    username: String,
    password: String,
    timeout: u64,
    retries: u32,
    pool_size: u32,
    tls_enabled: bool,
    log_level: String,
    max_connections: u32,
}
