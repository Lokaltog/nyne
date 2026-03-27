fn classify(x: i32) -> &'static str {
    if x == 1 {
        "one"
    } else if x == 2 {
        "two"
    } else {
        "other"
    }
}
