fn example(x: i32) -> &'static str {
    match x {
        0 => "zero",
        1 => "one",
        _ => "other",
    }
}
