fn main() {
    let x: u64 = 10000;
    println!("1) x is {} at {:p}", x, &x);

    let x: u64 = x * 2;
    println!("2) x is {} at *{:p}", x, &x);

    {
        let x: u64 = x * 2;
        println!("3) x is {} at {:p}", x, &x);
    }

    println!("4) x is {} at *{:p}", x, &x);

    let x: &str = "Rust!";
    println!("5) x is {} at {:p}", x, &x);
}
