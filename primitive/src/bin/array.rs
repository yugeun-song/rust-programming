use std::panic;

fn do_overflow() {
    let mut arr: [i32; 5] = [1, 2, 3, 4, 5];

    for i in 0..10 {
        println!("arr[{}] is {}", i, arr[i]);
        arr[i] = 0;
    }
}

fn main() {
    let arr1: [i32; 5] = [1, 2, 3, 4, 5];
    let arr2: [i32; 5] = [3; 5];

    println!("{:?}", arr1);
    println!("{:?}", arr2);

    let result = panic::catch_unwind(do_overflow);

    match result {
        Ok(_) => println!("completed normally!"),
        Err(_) => println!("panic caught!"),
    }
}
