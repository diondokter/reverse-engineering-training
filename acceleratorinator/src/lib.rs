use nusb::{transfer::RequestBuffer, DeviceInfo};
use pollster::FutureExt as _;
use std::{ffi::c_int, ptr::null_mut};

extern crate nusb;

pub struct CringUsbConnection {
    interface: Option<nusb::Interface>,
}

/// Create the USB structure
#[no_mangle]
#[inline(never)]
pub extern "C" fn cring_usb_create(usb: *mut *mut CringUsbConnection) -> c_int {
    if usb.is_null() {
        return err::CRING_EINVAL;
    }

    if unsafe { !(*usb).is_null() } {
        return err::CRING_EALREADY;
    }

    unsafe {
        *usb = Box::into_raw(Box::new(CringUsbConnection { interface: None }));
    }

    err::CRING_EOK
}

/// Free the USB structure
#[no_mangle]
#[inline(never)]
pub extern "C" fn cring_usb_free(usb: *mut *mut CringUsbConnection) -> c_int {
    if usb.is_null() {
        return err::CRING_EINVAL;
    }

    if unsafe { (*usb).is_null() } {
        return err::CRING_EINVAL;
    }

    unsafe {
        drop(Box::from_raw(*usb));
        *usb = null_mut();
    }

    err::CRING_EOK
}

/// Connect the USB to the first interface
#[no_mangle]
#[inline(never)]
pub extern "C" fn cring_usb_connect(
    usb: *mut CringUsbConnection,
    vendor_id: u16,
    product_id: u16,
) -> c_int {
    if usb.is_null() {
        return err::CRING_EINVAL;
    }

    let open_interface = |device_info: DeviceInfo| device_info.open()?.claim_interface(0);

    for d in nusb::list_devices().unwrap() {
        if d.vendor_id() == vendor_id && d.product_id() == product_id {
            match open_interface(d) {
                Ok(interface) => {
                    unsafe {
                        (*usb).interface = Some(interface);
                    }
                    return err::CRING_EOK;
                }
                Err(_) => return err::CRING_EUSB,
            }
        }
    }

    err::CRING_ENOTPRESENT
}

/// Send a bulk out message. The endpoint must *not* have its top-bit (`0x80`) set
#[no_mangle]
#[inline(never)]
pub extern "C" fn cring_usb_bulk_out(
    usb: *mut CringUsbConnection,
    ep: u8,
    data: *const u8,
    len: usize,
) -> c_int {
    if usb.is_null() {
        return err::CRING_EINVAL;
    }

    let interface = match unsafe { &mut *usb }.interface.as_mut() {
        Some(i) => i,
        None => return err::CRING_EINVAL,
    };

    let slice = unsafe { std::slice::from_raw_parts(data, len) };

    match interface
        .bulk_out(ep, slice.into())
        .block_on()
        .into_result()
    {
        Ok(_) => err::CRING_EOK,
        Err(_) => err::CRING_EUSB,
    }
}

/// Send a bulk in message. The endpoint must have its top-bit (`0x80`) set
#[no_mangle]
#[inline(never)]
pub extern "C" fn cring_usb_bulk_in(
    usb: *mut CringUsbConnection,
    ep: u8,
    data: *mut u8,
    len: usize,
) -> c_int {
    if usb.is_null() {
        return err::CRING_EINVAL;
    }

    let interface = match unsafe { &mut *usb }.interface.as_mut() {
        Some(i) => i,
        None => return err::CRING_EINVAL,
    };

    let slice = unsafe { std::slice::from_raw_parts_mut(data, len) };

    match interface
        .bulk_in(ep, RequestBuffer::new(len))
        .block_on()
        .into_result()
    {
        Ok(res) => {
            let min_len = res.len().min(len);
            slice[..min_len].copy_from_slice(&res[..min_len]);
            min_len as c_int
        }
        Err(_) => err::CRING_EUSB,
    }
}

pub const CRING_ACC_VID: u16 = 0xC0DE;
pub const CRING_ACC_PID: u16 = 0xCAFE;

pub const CRING_ACC_BOUT_EP: u8 = 0x01;
pub const CRING_ACC_BIN_EP: u8 = 0x81;

#[no_mangle]
#[inline(never)]
pub extern "C" fn cring_acc_send_bmp(
    usb: *mut CringUsbConnection,
    bmp_data: *mut u8,
    bmp_len: usize,
) -> c_int {
    if bmp_data.is_null() {
        return err::CRING_EINVAL;
    }

    if bmp_len == 0 {
        return err::CRING_EINVAL;
    }

    let mut encoded =
        cring_rle_encode(unsafe { std::slice::from_raw_parts_mut(bmp_data, bmp_len) });

    let res = cring_usb_bulk_out(usb, CRING_ACC_BOUT_EP, encoded.as_ptr(), encoded.len());
    if res < err::CRING_EOK {
        return res;
    }

    let mut buffer = [0; 64];
    let res = cring_usb_bulk_out(usb, CRING_ACC_BOUT_EP, buffer.as_ptr(), 0);
    if res < err::CRING_EOK {
        return res;
    }

    let res = cring_usb_bulk_in(usb, CRING_ACC_BIN_EP, buffer.as_mut_ptr(), 1);
    if res < err::CRING_EOK {
        return res;
    }

    if err::cring_map_from_device_error(buffer[0]) != err::CRING_EOK {
        return err::cring_map_from_device_error(buffer[0]);
    }

    let res = cring_usb_bulk_in(usb, CRING_ACC_BIN_EP, encoded.as_mut_ptr(), encoded.len());

    if res < err::CRING_EOK {
        return res;
    }

    encoded.truncate(res as usize);

    let decoded = cring_rle_decode(&encoded);

    let bmp_data_slice = unsafe { std::slice::from_raw_parts_mut(bmp_data, bmp_len) };
    let min_len = decoded.len().min(bmp_data_slice.len());
    bmp_data_slice[..min_len].copy_from_slice(&decoded[..min_len]);

    err::CRING_EOK
}

#[inline(never)]
#[no_mangle]
fn cring_rle_encode(mut input: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();

    while !input.is_empty() {
        let mut possible_block_savings = [u16::MAX; 4];
        let max_len = input.len().min(32);

        possible_block_savings[0] =
            cring_rle_calc_block_savings_frac((max_len + 1) as u8, max_len as u8);
        for block_size in 1..=3 {
            let max_repeat_count = match cring_rle_calc_max_block_repeats(input, block_size) {
                Some(value) => value,
                None => continue,
            };

            possible_block_savings[block_size] = cring_rle_calc_block_savings_frac(
                (block_size + 1) as u8,
                (block_size * max_repeat_count) as u8,
            );
        }

        let (best_block_size, _) = possible_block_savings
            .iter()
            .enumerate()
            .min_by_key(|(_, val)| **val)
            .unwrap();

        if best_block_size == 0 {
            cring_rle_push_on_vec(&mut output, cring_rle_calc_header(max_len as u8, 0));
            for b in &input[..max_len] {
                cring_rle_push_on_vec(&mut output, *b);
            }

            input = &input[max_len..];
        } else {
            let repeats = cring_rle_calc_max_block_repeats(input, best_block_size).unwrap();
            cring_rle_push_on_vec(
                &mut output,
                cring_rle_calc_header(repeats as u8, best_block_size as u8),
            );
            for b in &input[..best_block_size] {
                cring_rle_push_on_vec(&mut output, *b);
            }

            input = &input[repeats * best_block_size..];
        }
    }

    output
}

#[inline(never)]
#[no_mangle]
fn cring_rle_decode(mut input: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();

    while !input.is_empty() {
        let header = input[0];
        let block_size = cring_rle_get_header_block_size(header);
        let repeats = cring_rle_get_header_len(header);

        if block_size == 0 {
            for b in &input[1..][..repeats as usize] {
                cring_rle_push_on_vec(&mut output, *b);
            }

            input = &input[repeats as usize + 1..];
        } else {
            for _ in 0..repeats {
                for b in &input[1..][..block_size as usize] {
                    cring_rle_push_on_vec(&mut output, *b);
                }
            }

            input = &input[(block_size as usize) + 1..];
        }
    }

    output
}

#[inline(never)]
#[no_mangle]
fn cring_rle_calc_header(len: u8, block_size: u8) -> u8 {
    // assert!(len <= 64, "Can only encode 64 repeats max");
    assert!(len != 0, "Must encode at least 1 byte");
    assert!(block_size < 4, "Block size must be less than 4");

    ((len - 1) << 2) | block_size
}

#[inline(never)]
#[no_mangle]
fn cring_rle_get_header_len(header: u8) -> u8 {
    ((header & 0xFC) >> 2) + 1
}

#[inline(never)]
#[no_mangle]
fn cring_rle_get_header_block_size(header: u8) -> u8 {
    header & 0x03
}

#[inline(never)]
#[no_mangle]
fn cring_rle_push_on_vec(vec: &mut Vec<u8>, val: u8) {
    vec.push(val);
}

#[inline(never)]
#[no_mangle]
fn cring_rle_calc_max_block_repeats(input: &[u8], block_size: usize) -> Option<usize> {
    let repeat_value = match input.get(0..block_size) {
        Some(repeat_value) => repeat_value,
        None => return None,
    };

    let max_repeat_count = input
        .chunks_exact(block_size)
        .take_while(|chunk| *chunk == repeat_value)
        .count();

    Some(max_repeat_count)
}

#[inline(never)]
#[no_mangle]
fn cring_rle_calc_block_savings_frac(output_size: u8, input_size: u8) -> u16 {
    1000u16 * output_size as u16 / input_size as u16
}

pub mod err {
    use std::ffi::c_int;

    /// Operation went ok
    pub const CRING_EOK: c_int = 0;
    /// Operation has already happened so this call in invalid
    pub const CRING_EALREADY: c_int = -1;
    /// Some parameter is invalid
    pub const CRING_EINVAL: c_int = -2;
    /// The search yielded no valid result
    pub const CRING_ENOTPRESENT: c_int = -3;
    /// There was an error interacting with the USB
    pub const CRING_EUSB: c_int = -4;

    /// Unknown acceleratorinator error
    pub const CRING_EACC_UNKNOWN: c_int = -100;
    /// Unsupported compression
    pub const CRING_EACC_UNSUP_COMP: c_int = -101;
    /// Parse failure
    pub const CRING_EACC_PARSE: c_int = -102;

    #[inline(never)]
    #[no_mangle]
    pub fn cring_map_from_device_error(e: u8) -> c_int {
        match e {
            0 => CRING_EOK,
            1 => CRING_EACC_UNSUP_COMP,
            2 => CRING_EACC_PARSE,
            _ => CRING_EACC_UNKNOWN,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rle_encode_correct() {
        assert_eq!(cring_rle_encode(&[]), &[]);
        assert_eq!(
            cring_rle_encode(&[0, 1, 2, 3, 4, 5, 6]),
            &[6 << 2 | 0, 0, 1, 2, 3, 4, 5, 6]
        );
        assert_eq!(
            cring_rle_encode(&[0, 0, 2, 3, 4, 5, 6]),
            &[1 << 2 | 1, 0, 4 << 2 | 0, 2, 3, 4, 5, 6]
        );
        assert_eq!(
            cring_rle_encode(&[0, 0, 2, 3, 4, 2, 3, 4]),
            &[1 << 2 | 1, 0, 1 << 2 | 3, 2, 3, 4]
        );
    }

    #[test]
    fn rle_round_trip() {
        test_round_trip(&[]);
        test_round_trip(&[0, 1, 2, 3, 4, 5, 6]);
        test_round_trip(&[0, 0, 2, 3, 4, 5, 6]);
        test_round_trip(&[0, 0, 2, 3, 4, 2, 3, 4]);
        test_round_trip(include_bytes!("../../Bird-inverted.bmp"));
        test_round_trip(include_bytes!("../../Tg-inverted.bmp"));
    }

    #[test]
    #[should_panic]
    fn rle_round_trip_bad() {
        test_round_trip(include_bytes!("../../Cring-electronics-inverted.bmp"));
    }

    fn test_round_trip(input: &[u8]) {
        let encoded = cring_rle_encode(input);
        let output = cring_rle_decode(&encoded);

        println!(
            "input: {}, encoded: {}, factor: {}",
            input.len(),
            encoded.len(),
            encoded.len() as f32 / input.len() as f32
        );
        assert!(input == output);
    }
}
