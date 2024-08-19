fn main() {
    println!("Hello, world: {}", unsafe { acceleratorinator_sys::cring_acc_add(1, 2) });
}
