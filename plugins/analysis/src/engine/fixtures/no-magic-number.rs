const FACTOR: i32 = 42;
const OFFSET: i32 = 100;

fn compute(x: i32) -> i32 {
    x * FACTOR + OFFSET
}

fn trivial(x: i32) -> i32 {
    x + 1
}
