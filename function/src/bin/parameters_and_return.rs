fn add(param1: i64, param2: i64) -> i64 {
    param1 + param2
}

fn main() {
    let arg1 = 32;
    let arg2 = 21;
    let sum = add(arg1, arg2);
    println!("add({arg1}, {arg2}) returned {sum}");
}
