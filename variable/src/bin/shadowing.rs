fn main() {
    let x: u64 = 10000;
    println!("1) x is {x} at  {:p}, the first binding", &x);

    let x: u64 = x * 2;
    println!("2) x is {x} at *{:p}, a new binding that shadows 1", &x);

    {
        let x: u64 = x * 2;
        println!("3) x is {x} at  {:p}, shadows 2 inside this block only", &x);
    }

    println!("4) x is {x} at *{:p}, binding 2 again after the block", &x);

    let x: &str = "Rust!";
    println!("5) x is {x} at  {:p}, shadowing may change the type", &x);
}
