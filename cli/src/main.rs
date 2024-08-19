use std::{error::Error, ffi::c_int, fmt::Display, ptr::null_mut};

fn main() -> anyhow::Result<()> {
    let mut connection = null_mut();
    unsafe {
        println!("Creating USB");
        wrap(|| acceleratorinator_sys::cring_usb_create(&mut connection as *mut _))?;
        println!("Connecting USB to acceleratorinator");
        wrap(|| {
            acceleratorinator_sys::cring_usb_connect(
                connection,
                acceleratorinator_sys::CRING_ACC_VID as u16,
                acceleratorinator_sys::CRING_ACC_PID as u16,
            )
        })?;

        bulk_out(
            connection,
            acceleratorinator_sys::CRING_ACC_BOUT_EP as u8,
            &vec![0; 64],
        )?;

        let received = bulk_in(connection, acceleratorinator_sys::CRING_ACC_BIN_EP as u8, 64)?;
        println!("Received {}: {:?}", received.len(), received);

        println!("Done. Freeing USB");
        wrap(|| acceleratorinator_sys::cring_usb_free(&mut connection as *mut _))?;
    }

    Ok(())
}

fn bulk_out(
    connection: *mut acceleratorinator_sys::UsbConnection,
    ep: u8,
    data: &[u8],
) -> Result<(), CringError> {
    wrap(|| unsafe {
        acceleratorinator_sys::cring_usb_bulk_out(connection, ep, data.as_ptr(), data.len())
    })?;
    Ok(())
}

fn bulk_in(
    connection: *mut acceleratorinator_sys::UsbConnection,
    ep: u8,
    len: usize,
) -> Result<Vec<u8>, CringError> {
    let mut buffer = vec![0; len];

    let received_len = wrap(|| unsafe {
        acceleratorinator_sys::cring_usb_bulk_in(connection, ep, buffer.as_mut_ptr(), len)
    })?;

    buffer.truncate(received_len as usize);

    Ok(buffer)
}

fn wrap(f: impl FnOnce() -> c_int) -> Result<u32, CringError> {
    let val = f();

    if val >= 0 {
        Ok(val as u32)
    } else {
        Err(CringError(val))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CringError(c_int);

impl Display for CringError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Cring error `{}` => ", self.0)?;

        match self.0 {
            acceleratorinator_sys::CRING_EALREADY => write!(f, "ALREADY")?,
            acceleratorinator_sys::CRING_EINVAL => write!(f, "INVAL")?,
            acceleratorinator_sys::CRING_ENOTPRESENT => write!(f, "NOTPRESENT")?,
            acceleratorinator_sys::CRING_EUSB => write!(f, "USB")?,
            _ => unimplemented!(),
        }

        Ok(())
    }
}

impl Error for CringError {}
