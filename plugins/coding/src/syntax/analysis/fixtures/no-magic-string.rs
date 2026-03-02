const GREETING: &str = "Hello there";

fn greet(name: &str) -> String {
    format!("{GREETING}, {name}")
}
