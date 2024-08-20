use nusb::{transfer::RequestBuffer, DeviceInfo};
use pollster::FutureExt as _;
use std::{ffi::c_int, ptr::null_mut};

extern crate nusb;

pub struct CringUsbConnection {
    interface: Option<nusb::Interface>,
}

/// Create the USB structure
#[no_mangle]
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
pub extern "C" fn cring_acc_send_bmp(
    usb: *mut CringUsbConnection,
    mut bmp_data: *mut u8,
    mut bmp_len: usize,
) -> c_int {
    let res = cring_usb_bulk_out(usb, CRING_ACC_BOUT_EP, bmp_data, bmp_len);
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

    if err::map_from_device_error(buffer[0]) != err::CRING_EOK {
        return err::map_from_device_error(buffer[0]);
    }

    loop {
        let res = cring_usb_bulk_in(usb, CRING_ACC_BIN_EP, bmp_data, bmp_len);

        if res < err::CRING_EOK {
            return res;
        }

        if res == 0 {
            break;
        } else {
            bmp_len -= res as usize;
            bmp_data = unsafe { bmp_data.add(res as usize) };
        }

        if bmp_len == 0 {
            break;
        }
    }

    err::CRING_EOK
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
    pub fn map_from_device_error(e: u8) -> c_int {
        match e {
            0 => CRING_EOK,
            1 => CRING_EACC_UNSUP_COMP,
            2 => CRING_EACC_PARSE,
            _ => CRING_EACC_UNKNOWN,
        }
    }
}
