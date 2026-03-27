fn classify(x: i32) -> &'static str {
    if x == 1 {
        "one"
    } else if x == 2 {
        "two"
    } else if x == 3 {
        "three"
    } else if x == 4 {
        "four"
    } else {
        "other"
    }
}
