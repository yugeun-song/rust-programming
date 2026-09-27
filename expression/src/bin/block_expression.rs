fn main() {
    let result = {
        let inner = 10;
        inner * 20
    };

    println!("result is {result}, the value of the block's final expression");
}
