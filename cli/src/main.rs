use clap::Parser;
use std::{
    error::Error,
    ffi::c_int,
    fmt::Display,
    fs::File,
    io::{Read, Write},
    path::PathBuf,
    ptr::null_mut,
};
#[derive(Parser)]
struct Args {
    /// The path to the bmp file on disk
    bmp_path: PathBuf,
    /// The path where the returned bmp file is stored
    output_path: PathBuf,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Easier setup for debugging:
    // let args = Args {
    //     bmp_path: "../Bird-inverted.bmp".into(),
    //     output_path: "../output.bmp".into(),
    // };

    let mut image = Vec::new();
    File::open(args.bmp_path)?.read_to_end(&mut image)?;

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

        println!("Sending BMP image");
        wrap(|| {
            acceleratorinator_sys::cring_acc_send_bmp(
                connection,
                image.as_mut_ptr(),
                image.len(),
            )
        })?;
        
        File::create(&args.output_path)?.write_all(&image)?;

        println!("Done. Freeing USB");
        wrap(|| acceleratorinator_sys::cring_usb_free(&mut connection as *mut _))?;
    }

    Ok(())
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

            acceleratorinator_sys::CRING_EACC_UNKNOWN => write!(f, "ACC_UNKNOWN")?,
            acceleratorinator_sys::CRING_EACC_UNSUP_COMP => write!(f, "ACC_UNSUP_COMP")?,
            acceleratorinator_sys::CRING_EACC_PARSE => write!(f, "ACC_PARSE")?,
            _ => unimplemented!(),
        }

        Ok(())
    }
}

impl Error for CringError {}
