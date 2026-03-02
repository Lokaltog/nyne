fn example(x: Option<i32>) -> &str {
    match x {
        Some(v) => "has value",
        None => "empty",
    }
}
