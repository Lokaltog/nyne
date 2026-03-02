fn build_csv(items: &[&str]) -> String {
    let mut result = String::new();
    for item in items {
        result += item;
        result += ",";
    }
    result
}
