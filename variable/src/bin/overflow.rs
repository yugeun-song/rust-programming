use std::panic;

fn do_overflow() {
    let mut x: u8 = u8::MAX;
    println!("just trying to add one...");
    x = x.strict_add(1);
    println!("x is {x}");
}

fn main() {
    let result = panic::catch_unwind(do_overflow);

    match result {
        Ok(_) => println!("completed normally!"),
        Err(_) => println!("panic caught!"),
    }
}
