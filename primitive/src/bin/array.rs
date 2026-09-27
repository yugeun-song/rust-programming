use std::panic;

fn index_out_of_bounds() {
    let mut arr: [i32; 5] = [1, 2, 3, 4, 5];

    println!("indexing arr[0] through arr[9] on a [i32; 5]");
    for i in 0..10 {
        println!("arr[{i}] is {}", arr[i]);
        arr[i] = 0;
    }
}

fn main() {
    let listed: [i32; 5] = [1, 2, 3, 4, 5];
    let repeated: [i32; 5] = [3; 5];

    println!("listed   {listed:?}, written out element by element");
    println!("repeated {repeated:?}, written as [3; 5]");

    let result = panic::catch_unwind(index_out_of_bounds);

    match result {
        Ok(()) => println!("completed normally"),
        Err(_) => println!("panic caught: every index is checked at run time"),
    }
}
