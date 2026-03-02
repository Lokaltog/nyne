enum Command {
    Start,
    Stop,
}

fn dispatch(cmd: Command) {
    match cmd {
        Command::Start => start(),
        Command::Stop => stop(),
    }
}

fn start() {}
fn stop() {}
