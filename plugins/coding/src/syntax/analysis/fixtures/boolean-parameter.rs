fn connect(host: &str, port: u16, use_tls: bool) {
    if use_tls {
        connect_tls(host, port);
    }
}
