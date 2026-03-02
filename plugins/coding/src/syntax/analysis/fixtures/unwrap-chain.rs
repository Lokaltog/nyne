fn example() {
    let val = map.get("key").unwrap().parse::<i32>().unwrap();
    println!("{}", val);
}
