#[no_mangle]
pub extern "C" fn cring_acc_add(left: u64, right: u64) -> u64 {
    left + right
}
