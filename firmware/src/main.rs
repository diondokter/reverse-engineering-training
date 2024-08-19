#![no_std]
#![no_main]

use core::sync::atomic::AtomicBool;

use embassy_executor::Spawner;
use embassy_nrf::{
    bind_interrupts, pac,
    peripherals::{PWM0, USBD},
    pwm::{self, SequencePwm},
    usb::{self, In, Out},
};
use embassy_usb::driver::{Endpoint, EndpointError, EndpointIn, EndpointOut};
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    USBD => usb::InterruptHandler<embassy_nrf::peripherals::USBD>;
    POWER_CLOCK => usb::vbus_detect::InterruptHandler;
});

// This is a randomly generated GUID to allow clients on Windows to find our device
const DEVICE_INTERFACE_GUIDS: &[&str] = &["{EAA9A5DC-30BA-44BC-9232-606CDC875321}"];
const MAX_PACKET_SIZE: usize = 64;

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_nrf::init(Default::default());
    let clock: pac::CLOCK = unsafe { core::mem::transmute(()) };

    defmt::info!("Enabling ext hfosc...");
    clock.tasks_hfclkstart.write(|w| unsafe { w.bits(1) });
    while clock.events_hfclkstarted.read().bits() != 1 {}

    defmt::info!("Initialized!");

    // Setup the LED PWM
    let mut config = pwm::Config::default();
    config.prescaler = pwm::Prescaler::Div64;
    let pwm = defmt::unwrap!(pwm::SequencePwm::new_1ch(p.PWM0, p.P0_13, config));
    spawner.must_spawn(led_driver(pwm));

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

    let mut usb = builder.build();

    let usb_app = async {
        loop {
            if let Err(e) = listen(&mut ep_in, &mut ep_out).await {
                defmt::error!("Endpoint error: {}", e);
            }
        }
    };

    embassy_futures::join::join(usb.run(), usb_app).await;
}

async fn listen(
    ep_in: &mut embassy_nrf::usb::Endpoint<'_, USBD, In>,
    ep_out: &mut embassy_nrf::usb::Endpoint<'_, USBD, Out>,
) -> Result<(), EndpointError> {
    let mut buffer = [0; MAX_PACKET_SIZE];

    ep_out.wait_enabled().await;

    defmt::info!("Connected!");

    loop {
        let len = ep_out.read(&mut buffer).await?;
        let received = &buffer[..len];

        defmt::info!("Received: {:X}", received);

        ep_in.write(received).await?;
    }
}

static GLITCHY: AtomicBool = AtomicBool::new(false);

#[embassy_executor::task]
async fn led_driver(mut pwm: SequencePwm<'static, PWM0>) {
    let sin_words: [u16; 150] = SIN.clone();
    let glitch_words: [u16; 150] = GLITCHY_SIN.clone();

    let mut seq_config = pwm::SequenceConfig::default();
    seq_config.refresh = 1; // New sample every 16ms

    let mut sequencer;

    loop {
        let selected_words = if GLITCHY.load(core::sync::atomic::Ordering::Relaxed) {
            &glitch_words
        } else {
            &sin_words
        };

        sequencer = pwm::SingleSequencer::new(&mut pwm, selected_words, seq_config.clone());
        defmt::unwrap!(sequencer.start(pwm::SingleSequenceMode::Infinite));

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
