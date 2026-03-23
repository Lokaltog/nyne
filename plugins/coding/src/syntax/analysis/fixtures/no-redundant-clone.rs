fn example(data: String) {
    let copy = data.clone();
    send(copy);
    println!("{}", data);
}

// Clone on last statement — can't move out of a MutexGuard/borrow.
fn returns_cloned(table: MutexGuard<Vec<Item>>) -> Vec<Item> {
    table.clone()
}
