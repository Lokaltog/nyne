fn example() {
    let work = |x: i32| {
        let a = x + 1;
        let b = a + 2;
        let c = b + 3;
        let d = c + 4;
        let e = d + 5;
        let f = e + 6;
        let g = f + 7;
        let h = g + 8;
        let i = h + 9;
        let j = i + 10;
        let k = j + 11;
        let l = k + 12;
        let m = l + 13;
        let n = m + 14;
        n + 15
    };
    work(0);
}
