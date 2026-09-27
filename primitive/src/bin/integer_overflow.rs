use std::panic;

fn overflow_u8() {
    let mut x: u8 = u8::MAX;
    println!("x is {x}, the largest u8; adding 1 with strict_add");
    x = x.strict_add(1);
    println!("x is {x}");
}

fn main() {
    let result = panic::catch_unwind(overflow_u8);

    match result {
        Ok(()) => println!("completed normally"),
        Err(_) => println!("panic caught: strict_add panics in debug and release alike"),
    }
}
