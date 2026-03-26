fn example() {
    let double = |x: i32| x * 2;
    let add = |a: i32, b: i32| {
        let result = a + b;
        result
    };
    double(add(1, 2));
}
