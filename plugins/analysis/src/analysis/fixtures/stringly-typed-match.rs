fn dispatch(cmd: &str) {
    match cmd {
        "start" => start(),
        "stop" => stop(),
        "restart" => restart(),
        _ => unknown(),
    }
}

fn start() {}
fn stop() {}
fn restart() {}
fn unknown() {}
