fn example(x: Option<i32>) -> Option<String> {
    match x {
        Some(v) => Some(v.to_string()),
        None => None,
    }
}
