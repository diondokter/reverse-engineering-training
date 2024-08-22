#![no_std]
#![no_main]

use core::{
    fmt::Write,
    panic::PanicInfo,
    sync::atomic::{AtomicBool, Ordering},
};

use bitvec::{field::BitField, order::Lsb0, view::BitView};
use embassy_executor::Spawner;
use embassy_nrf::{
    bind_interrupts,
    gpio::Output,
    pac,
    peripherals::{PWM0, USBD},
    pwm::{self, SequencePwm},
    uarte::{self, Uarte},
    usb::{self, In, Out},
};
use embassy_sync::{blocking_mutex::raw::{CriticalSectionRawMutex, ThreadModeRawMutex}, mutex::Mutex};
use embassy_usb::driver::{Endpoint, EndpointError, EndpointIn, EndpointOut};
use heapless::Vec;

bind_interrupts!(struct Irqs {
    USBD => usb::InterruptHandler<embassy_nrf::peripherals::USBD>;
    POWER_CLOCK => usb::vbus_detect::InterruptHandler;
    UARTE0_UART0 => uarte::InterruptHandler<embassy_nrf::peripherals::UARTE0>;
});

// This is a randomly generated GUID to allow clients on Windows to find our device
const DEVICE_INTERFACE_GUIDS: &[&str] = &["{EAA9A5DC-30BA-44BC-9232-606CDC875321}"];
const MAX_PACKET_SIZE: usize = 64;

static UART: Mutex<CriticalSectionRawMutex, UartWriter> = Mutex::new(UartWriter(None));

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_nrf::init(Default::default());
    let clock: pac::CLOCK = unsafe { core::mem::transmute(()) };

    let uart = UartWriter(Some(uarte::Uarte::new(
        p.UARTE0,
        Irqs,
        p.P0_08,
        p.P0_06,
        Default::default(),
    )));

    *UART.lock().await = uart;

    writeln!(UART.lock().await, "").unwrap();
    writeln!(UART.lock().await, "---------------------------------------").unwrap();
    writeln!(UART.lock().await, "Enabling ext hfosc...").unwrap();
    clock.tasks_hfclkstart.write(|w| unsafe { w.bits(1) });
    while clock.events_hfclkstarted.read().bits() != 1 {}

    writeln!(UART.lock().await, "Initialized!").unwrap();

    // Setup the LED PWM
    let mut config = pwm::Config::default();
    config.prescaler = pwm::Prescaler::Div64;
    let pwm = pwm::SequencePwm::new_1ch(p.PWM0, p.P0_13, config).unwrap();
    spawner.must_spawn(led_driver(
        pwm,
        Output::new(
            p.P0_14,
            embassy_nrf::gpio::Level::High,
            embassy_nrf::gpio::OutputDrive::Standard0Disconnect1,
        ),
    ));

    // Setup the USB
    // Create the driver, from the HAL.
    let driver = usb::Driver::new(
        p.USBD,
        Irqs,
        usb::vbus_detect::HardwareVbusDetect::new(Irqs),
    );

    // Create embassy-usb Config
    let mut config = embassy_usb::Config::new(0xc0de, 0xcafe);
    config.manufacturer = Some("Cring Electronics");
    config.product = Some("Video acceleratorinator");
    config.serial_number = Some("12345678");
    config.max_power = 100;
    config.max_packet_size_0 = 64;
    config.device_class = 0xFF;
    config.device_sub_class = 0x00;
    config.device_protocol = 0x00;
    config.composite_with_iads = false;

    // Create embassy-usb DeviceBuilder using the driver and config.
    // It needs some buffers for building the descriptors.
    let mut config_descriptor = [0; 32];
    let mut bos_descriptor = [0; 40];
    let mut msos_descriptor = [0; 162];
    let mut control_buf = [0; 64];

    let mut builder = embassy_usb::Builder::new(
        driver,
        config,
        &mut config_descriptor,
        &mut bos_descriptor,
        &mut msos_descriptor,
        &mut control_buf,
    );

    builder.msos_descriptor(embassy_usb::msos::windows_version::WIN8_1, 0);
    builder.msos_feature(embassy_usb::msos::CompatibleIdFeatureDescriptor::new(
        "WINUSB", "",
    ));
    builder.msos_feature(embassy_usb::msos::RegistryPropertyFeatureDescriptor::new(
        "DeviceInterfaceGUIDs",
        embassy_usb::msos::PropertyData::RegMultiSz(DEVICE_INTERFACE_GUIDS),
    ));

    let (mut ep_in, mut ep_out) = {
        let mut func = builder.function(0xFF, 0x00, 0x00);
        let mut iface = func.interface();
        let mut alt = iface.alt_setting(0xFF, 0x00, 0x00, None);

        let ep_in = alt.endpoint_bulk_in(MAX_PACKET_SIZE as u16);
        let ep_out = alt.endpoint_bulk_out(MAX_PACKET_SIZE as u16);

        (ep_in, ep_out)
    };

    writeln!(UART.lock().await, "{:?}", ep_in.info()).unwrap();
    writeln!(UART.lock().await, "{:?}", ep_out.info()).unwrap();

    let mut usb = builder.build();

    let usb_app = async {
        loop {
            if let Err(e) = listen(&mut ep_in, &mut ep_out).await {
                GLITCHY.store(true, Ordering::Relaxed);
                writeln!(UART.lock().await, "Endpoint error: {:?}", e).unwrap();
            }
        }
    };

    embassy_futures::join::join(usb.run(), usb_app).await;
}

const BUFFER_SIZE: usize = 100 * 1024;
static ENCODED_BMP_BUFFER: Mutex<ThreadModeRawMutex, Vec<u8, { BUFFER_SIZE }>> =
    Mutex::new(Vec::new());
static DECODED_BMP_BUFFER: Mutex<ThreadModeRawMutex, Vec<u8, { BUFFER_SIZE }>> =
    Mutex::new(Vec::new());

async fn listen(
    ep_in: &mut embassy_nrf::usb::Endpoint<'_, USBD, In>,
    ep_out: &mut embassy_nrf::usb::Endpoint<'_, USBD, Out>,
) -> Result<(), EndpointError> {
    let mut encoded_bmp_buffer = ENCODED_BMP_BUFFER.lock().await;
    let encoded_bmp_buffer = &mut *encoded_bmp_buffer;
    let mut decoded_bmp_buffer = DECODED_BMP_BUFFER.lock().await;
    let decoded_bmp_buffer = &mut *decoded_bmp_buffer;

    ep_out.wait_enabled().await;

    writeln!(UART.lock().await, "Connected!").unwrap();

    let mut buffer = [0; MAX_PACKET_SIZE];

    loop {
        let len = ep_out.read(&mut buffer).await?;

        if encoded_bmp_buffer.extend_from_slice(&buffer[..len]).is_err() {
            panic!("The receive buffer is full");
        }

        if len == 0 {
            writeln!(
                UART.lock().await,
                "@ {} - Received bmp, total: {}",
                embassy_time::Instant::now().as_millis(),
                encoded_bmp_buffer.len(),
            )
            .unwrap();

            embassy_futures::yield_now().await;

            cring_rle_decode(encoded_bmp_buffer, decoded_bmp_buffer);

            embassy_futures::yield_now().await;

            writeln!(
                UART.lock().await,
                "@ {} - Decoded bmp, total: {}",
                embassy_time::Instant::now().as_millis(),
                decoded_bmp_buffer.len(),
            )
            .unwrap();

            // Last message, so do the bmp thing
            let error = 'e: {
                match tinybmp::RawBmp::from_slice(&decoded_bmp_buffer) {
                    Ok(bmp) => {
                        embassy_futures::yield_now().await;

                        let header = bmp.header().clone();

                        let image_data_len = if header.image_data_len == 0 {
                            (header.image_size.width
                                * header.image_size.height
                                * header.bpp.bits() as u32
                                / 8) as usize
                        } else {
                            header.image_data_len as usize
                        }
                        .min(decoded_bmp_buffer[header.image_data_start..].len());

                        let image_bits_view = decoded_bmp_buffer[header.image_data_start..]
                            [..image_data_len]
                            .view_bits_mut::<Lsb0>();

                        if matches!(
                            header.compression_method,
                            tinybmp::CompressionMethod::Rle4 | tinybmp::CompressionMethod::Rle8
                        ) {
                            break 'e Error::UnsupportedCompression;
                        }

                        embassy_futures::yield_now().await;

                        let color_ranges = if let Some(channel_masks) = header.channel_masks {
                            let start = channel_masks.red.trailing_zeros() as usize;
                            let end = 32 - channel_masks.red.leading_zeros() as usize;
                            let red_range = start..end;

                            let start = channel_masks.green.trailing_zeros() as usize;
                            let end = 32 - channel_masks.green.leading_zeros() as usize;
                            let green_range = start..end;

                            let start = channel_masks.blue.trailing_zeros() as usize;
                            let end = 32 - channel_masks.blue.leading_zeros() as usize;
                            let blue_range = start..end;

                            Some((red_range, green_range, blue_range))
                        } else {
                            None
                        };

                        for pixel in image_bits_view.chunks_exact_mut(header.bpp.bits() as usize) {
                            embassy_futures::yield_now().await;

                            if let Some((red, green, blue)) = color_ranges.as_ref() {
                                let value = pixel[red.clone()].load_le::<u8>();
                                pixel[red.clone()].store_le(u8::MAX - value);

                                let value = pixel[green.clone()].load_le::<u8>();
                                pixel[green.clone()].store_le(u8::MAX - value);

                                let value = pixel[blue.clone()].load_le::<u8>();
                                pixel[blue.clone()].store_le(u8::MAX - value);
                            } else {
                                pixel.store_le(u32::MAX - pixel.load_le::<u32>());
                            }
                        }

                        writeln!(
                            UART.lock().await,
                            "@ {} - Processing done. Starting encoding",
                            embassy_time::Instant::now().as_millis(),
                        )
                        .unwrap();

                        embassy_futures::yield_now().await;

                        cring_rle_encode(&decoded_bmp_buffer, encoded_bmp_buffer);

                        embassy_futures::yield_now().await;

                        Error::Ok
                    }
                    Err(e) => {
                        writeln!(UART.lock().await, "BMP parse error: {:?}", e).unwrap();
                        Error::ParseError
                    }
                }
            };

            writeln!(
                UART.lock().await,
                "@ {} - Processing done. Starting send back",
                embassy_time::Instant::now().as_millis(),
            )
            .unwrap();

            ep_in.write(&[error as u8]).await?;

            if let Error::Ok = error {
                GLITCHY.store(false, Ordering::Relaxed);

                for chunk in encoded_bmp_buffer.chunks(MAX_PACKET_SIZE) {
                    ep_in.write(chunk).await?;
                }
            } else {
                GLITCHY.store(true, Ordering::Relaxed);
            }

            writeln!(
                UART.lock().await,
                "@ {} - Send back complete. Waiting for next bmp",
                embassy_time::Instant::now().as_millis(),
            )
            .unwrap();

            decoded_bmp_buffer.clear();
            encoded_bmp_buffer.clear();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum Error {
    Ok,
    UnsupportedCompression,
    ParseError,
}

static GLITCHY: AtomicBool = AtomicBool::new(false);

#[embassy_executor::task]
async fn led_driver(mut pwm: SequencePwm<'static, PWM0>, mut led2: Output<'static>) {
    let sin_words: [u16; 150] = SIN.clone();
    let glitch_words: [u16; 150] = GLITCHY_SIN.clone();

    let mut seq_config = pwm::SequenceConfig::default();
    seq_config.refresh = 1; // New sample every 16ms

    let mut sequencer;

    loop {
        led2.toggle();
        let selected_words = if GLITCHY.load(Ordering::Relaxed) {
            &glitch_words
        } else {
            &sin_words
        };

        sequencer = pwm::SingleSequencer::new(&mut pwm, selected_words, seq_config.clone());
        sequencer.start(pwm::SingleSequenceMode::Infinite).unwrap();

        embassy_time::Timer::after_millis(1200).await;
        drop(sequencer);
    }
}

const SIN: [u16; 150] = [
    0x00E6, 0x00F3, 0x0100, 0x010D, 0x011A, 0x0128, 0x0135, 0x0143, 0x0151, 0x015F, 0x016C, 0x017A,
    0x0188, 0x0196, 0x01A3, 0x01B0, 0x01BD, 0x01CA, 0x01D7, 0x01E3, 0x01EE, 0x01FA, 0x0205, 0x020F,
    0x0219, 0x0222, 0x022B, 0x0233, 0x023A, 0x0241, 0x0247, 0x024D, 0x0251, 0x0255, 0x0258, 0x025B,
    0x025C, 0x025D, 0x025D, 0x025C, 0x025B, 0x0258, 0x0255, 0x0251, 0x024D, 0x0247, 0x0241, 0x023A,
    0x0233, 0x022B, 0x0222, 0x0219, 0x020F, 0x0205, 0x01FA, 0x01EE, 0x01E3, 0x01D7, 0x01CA, 0x01BD,
    0x01B0, 0x01A3, 0x0196, 0x0188, 0x017A, 0x016C, 0x015F, 0x0151, 0x0143, 0x0135, 0x0128, 0x011A,
    0x010D, 0x0100, 0x00F3, 0x00E6, 0x00DA, 0x00CD, 0x00C2, 0x00B6, 0x00AB, 0x00A0, 0x0096, 0x008B,
    0x0082, 0x0078, 0x006F, 0x0067, 0x005F, 0x0057, 0x0050, 0x0049, 0x0042, 0x003C, 0x0036, 0x0030,
    0x002B, 0x0027, 0x0022, 0x001E, 0x001A, 0x0017, 0x0014, 0x0011, 0x000F, 0x000D, 0x000B, 0x0009,
    0x0008, 0x0007, 0x0006, 0x0005, 0x0005, 0x0005, 0x0005, 0x0006, 0x0007, 0x0008, 0x0009, 0x000B,
    0x000D, 0x000F, 0x0011, 0x0014, 0x0017, 0x001A, 0x001E, 0x0022, 0x0027, 0x002B, 0x0030, 0x0036,
    0x003C, 0x0042, 0x0049, 0x0050, 0x0057, 0x005F, 0x0067, 0x006F, 0x0078, 0x0082, 0x008B, 0x0096,
    0x00A0, 0x00AB, 0x00B6, 0x00C2, 0x00CD, 0x00DA,
];

const GLITCHY_SIN: [u16; 150] = [
    0x00E7, 0x017F, 0x00AB, 0x00DB, 0x0128, 0x0109, 0x01C3, 0x00D1, 0x0102, 0x0213, 0x01A6, 0x0209,
    0x0207, 0x016A, 0x015C, 0x0196, 0x019E, 0x0120, 0x027D, 0x0213, 0x02CF, 0x0143, 0x0174, 0x01A4,
    0x025B, 0x02EC, 0x0252, 0x031D, 0x02D8, 0x0286, 0x02F0, 0x02B4, 0x01A2, 0x028D, 0x0239, 0x033D,
    0x02CD, 0x0247, 0x02BF, 0x0194, 0x0327, 0x0305, 0x02A6, 0x01B3, 0x0285, 0x01F3, 0x017A, 0x023E,
    0x01E6, 0x02B0, 0x02FF, 0x01E1, 0x01AF, 0x0221, 0x01B8, 0x0244, 0x0270, 0x01AF, 0x026F, 0x0128,
    0x022B, 0x01E2, 0x012C, 0x0116, 0x01CB, 0x01E8, 0x0113, 0x01D7, 0x00D2, 0x00A6, 0x00F0, 0x00F7,
    0x015A, 0x0077, 0x0191, 0x00D9, 0x0142, 0x00B7, 0x012B, 0x0114, 0x011E, 0x00E5, 0x00B2, 0x003D,
    0x008D, 0x004F, 0x0025, 0x005E, 0x003A, 0x0023, 0x00CA, 0x0087, 0x0049, 0x004C, 0x004A, 0x0000,
    0x0000, 0x0042, 0x003F, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x001A, 0x0033, 0x000A,
    0x0000, 0x005F, 0x0004, 0x0000, 0x004C, 0x0000, 0x0000, 0x0041, 0x0011, 0x0000, 0x0013, 0x0025,
    0x0000, 0x0064, 0x0000, 0x0000, 0x0056, 0x005A, 0x0067, 0x002D, 0x0028, 0x0000, 0x0000, 0x0000,
    0x006E, 0x0094, 0x005B, 0x0037, 0x0089, 0x003F, 0x000F, 0x000F, 0x005E, 0x002A, 0x00FB, 0x00E5,
    0x0057, 0x00BE, 0x0079, 0x0078, 0x016A, 0x00CD,
];

#[cortex_m_rt::exception]
unsafe fn HardFault(ef: &cortex_m_rt::ExceptionFrame) -> ! {
    if let Ok(mut uart) = UART.try_lock() {
        let _ = writeln!(uart, "{ef:?}");
    }

    loop {
        cortex_m::asm::delay(32_000_000);
        cortex_m::peripheral::SCB::sys_reset();
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    cortex_m::interrupt::disable();

    if let Ok(mut uart) = UART.try_lock() {
        let _ = writeln!(uart, "{info}");
    }

    loop {
        cortex_m::asm::delay(32_000_000);
        cortex_m::peripheral::SCB::sys_reset();
    }
}

struct UartWriter(Option<Uarte<'static, embassy_nrf::peripherals::UARTE0>>);

impl core::fmt::Write for UartWriter {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.0
            .as_mut()
            .unwrap()
            .blocking_write(s.as_bytes())
            .map_err(|_| Default::default())
    }
}

#[inline(never)]
#[no_mangle]
fn cring_rle_encode(mut input: &[u8], output: &mut Vec<u8, { BUFFER_SIZE }>) {
    output.clear();

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
            cring_rle_push_on_vec(output, cring_rle_calc_header(max_len as u8, 0));
            for b in &input[..max_len] {
                cring_rle_push_on_vec(output, *b);
            }

            input = &input[max_len..];
        } else {
            let repeats = cring_rle_calc_max_block_repeats(input, best_block_size).unwrap();
            cring_rle_push_on_vec(
                output,
                cring_rle_calc_header(repeats as u8, best_block_size as u8),
            );
            for b in &input[..best_block_size] {
                cring_rle_push_on_vec(output, *b);
            }

            input = &input[repeats * best_block_size..];
        }
    }
}

#[inline(never)]
#[no_mangle]
fn cring_rle_decode(mut input: &[u8], output: &mut Vec<u8, { BUFFER_SIZE }>) {
    output.clear();

    while !input.is_empty() {
        let header = input[0];
        let block_size = cring_rle_get_header_block_size(header);
        let repeats = cring_rle_get_header_len(header);

        // defmt::println!("Block size: {}", block_size);
        // defmt::println!("Repeats: {}", repeats);

        if block_size == 0 {
            for b in &input[1..][..repeats as usize] {
                cring_rle_push_on_vec(output, *b);
            }

            input = &input[repeats as usize + 1..];
        } else {
            for _ in 0..repeats {
                for b in &input[1..][..block_size as usize] {
                    cring_rle_push_on_vec(output, *b);
                }
            }

            input = &input[(block_size as usize) + 1..];
        }
    }
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
fn cring_rle_push_on_vec(vec: &mut Vec<u8, { BUFFER_SIZE }>, val: u8) {
    vec.push(val).ok();
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
