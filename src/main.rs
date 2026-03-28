use esp32_c3_rust_atomic_battery::nfc_tag::{
    self, esp_idf::StdTimer, KvFormatError, KvStore, NfcError, NfcTag,
};
use esp32_c3_rust_atomic_battery::segment_display::{
    Align, DisplayError, IntFormat, SegmentDisplay4,
};
use esp_idf_svc::hal::gpio::Pull;
use esp_idf_svc::hal::{
    delay::FreeRtos,
    gpio::{Level, PinDriver},
    i2c::{I2c, I2cConfig, I2cDriver, I2cError},
    peripherals::Peripherals,
};
use esp_idf_svc::sys::{false_, EspError};
use pn532::i2c::I2CInterface;
use std::fmt;

use std::time::Duration;

use log::{error, info, warn};

const WRITE_DEMO: bool = false;

#[derive(Debug)]
enum AppError {
    Esp(EspError),
    Kv(KvFormatError),
    Nfc(NfcError<I2cError>),
    Display(DisplayError),
}

impl From<EspError> for AppError {
    fn from(value: EspError) -> Self {
        Self::Esp(value)
    }
}

impl From<KvFormatError> for AppError {
    fn from(value: KvFormatError) -> Self {
        Self::Kv(value)
    }
}

impl From<NfcError<I2cError>> for AppError {
    fn from(value: NfcError<I2cError>) -> Self {
        Self::Nfc(value)
    }
}

impl From<DisplayError> for AppError {
    fn from(value: DisplayError) -> Self {
        Self::Display(value)
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::Esp(err) => write!(f, "esp error: {err}"),
            AppError::Kv(err) => write!(f, "kv format error: {err}"),
            AppError::Nfc(err) => write!(f, "nfc error: {err}"),
            AppError::Display(err) => write!(f, "display error: {err}"),
        }
    }
}

impl std::error::Error for AppError {}

fn main() {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("Starting...");

    if let Err(e) = run() {
        log::error!("Fatal error: {e:?}");
        loop {
            FreeRtos::delay_ms(1000);
        }
    }
}

const BAT_SERVICE_KEY: &str = "service";
const BAT_HEALTH_KEY: &str = "health";
const BAT_CHARGE_KEY: &str = "charge";
const BAT_CONSUMPTION_KEY: &str = "consumption";

fn run() -> Result<(), AppError> {
    // test_display()
    let p = Peripherals::take()?;

    let mut board_led_pin = PinDriver::output(p.pins.gpio8)?;
    board_led_pin.set_level(Level::Low)?;

    let mut red_led_pin = PinDriver::output(p.pins.gpio0)?;
    red_led_pin.set_level(Level::High)?;

    let mut green_led_pin = PinDriver::output(p.pins.gpio1)?;
    green_led_pin.set_level(Level::Low)?;

    let switch_pin = PinDriver::input(p.pins.gpio10, Pull::Up)?;

    let mut switch_enabled = false;
    let mut battery_plugged = false;
    let mut battery_healthy = false;
    let mut battery_has_charge = false;

    let mut nfc = nfc_tag::esp_idf::new_default(
        p.i2c0,
        p.pins.gpio3, // SDA
        p.pins.gpio4, // SCL
    )?;
    nfc.init_default()?;

    let mut display = SegmentDisplay4::new(
        p.pins.gpio5, // CLK
        p.pins.gpio6, // DIO
    )?;
    display.init()?;

    loop {
        log::info!("Loop begin");

        match read_nfc(&mut nfc) {
            Some(battery_data) => {
                // Разобрать и разложить по переменным
                battery_plugged = true;
            }
            None => {
                battery_plugged = false;
            }
        }

        switch_enabled = switch_pin.is_low();

        if !battery_plugged {
            red_led_pin.set_high();
            if switch_enabled {
                display.scroll_error_once(Duration::from_millis(250))?;
                // FreeRtos::delay_ms(500);
            } 
        } else {
            red_led_pin.set_low();
        }

        FreeRtos::delay_ms(100);

        // FreeRtos::delay_ms(500);
        // board_led_pin.set_level(Level::Low)?;
        // FreeRtos::delay_ms(500);
        // green_led_pin.set_level(Level::Low)?;
        // FreeRtos::delay_ms(500);
        // red_led_pin.set_level(Level::Low)?;
        // FreeRtos::delay_ms(500);
        // log::info!("loop 2");
        // while switch_pin.is_high() {
        //     FreeRtos::delay_ms(100);
        //     log::info!("loop 3");
        // }
        // board_led_pin.set_level(Level::High)?;
        // green_led_pin.set_level(Level::High)?;
        // red_led_pin.set_level(Level::High)?;
        // FreeRtos::delay_ms(1000);
        // log::info!("loop 4");
    }
}

fn read_nfc(nfc: &mut NfcTag<I2CInterface<I2cDriver<'_>>, StdTimer, 64>) -> Option<KvStore> {
    match nfc.poll_tag(Duration::from_millis(1000)) {
        Ok(Some(tag)) => {
            info!(
                "Tag UID: {:02X?}, ATQA={:02X?}, SAK=0x{:02X}",
                tag.uid, tag.atqa, tag.sak
            );

            match nfc.read_kv_store() {
                Ok(store) => {
                    info!("Tag key-value data: {:?}", store.entries());
                    return Option::Some(store);
                }
                Err(_) => return Option::None,
            }
        }
        Ok(None) => return Option::None,
        Err(e) => return Option::None,
    }
}

fn test_display() -> Result<(), AppError> {
    let peripherals = Peripherals::take()?;

    let mut display = SegmentDisplay4::new(
        peripherals.pins.gpio5, // CLK
        peripherals.pins.gpio6, // DIO
    )?;
    display.init()?;

    loop {
        display.show_int(42, IntFormat::new().right())?;
        FreeRtos::delay_ms(1000);

        display.show_int(42, IntFormat::new().right().leading_zeros(true))?;
        FreeRtos::delay_ms(1000);

        display.show_int(42, IntFormat::new().left())?;
        FreeRtos::delay_ms(1000);

        display.set_colon(true)?;
        display.show_mmss(12, 34)?;

        for _ in 0..6 {
            FreeRtos::delay_ms(500);
            display.toggle_colon()?;
        }

        display.set_colon(false)?;
        display.show_error()?;
        FreeRtos::delay_ms(1200);

        display.show_text("AbCd", Align::Left)?;
        FreeRtos::delay_ms(1200);

        display.scroll_error_once(Duration::from_millis(250))?;
        FreeRtos::delay_ms(500);
    }
}

fn test_reader() -> Result<(), AppError> {
    let p = Peripherals::take()?;

    let mut nfc = nfc_tag::esp_idf::new_default(
        p.i2c0,
        p.pins.gpio3, // SDA
        p.pins.gpio4, // SCL
    )?;

    nfc.init_default()?;

    let fw = nfc.firmware_version()?;
    info!("PN532 firmware raw: {:02X?}", fw);

    let mut demo_store = KvStore::new();
    demo_store.insert_string("name", "ESP32-C3")?;
    demo_store.insert_u8("counter", 42)?;
    demo_store.insert_i8("temperature_offset", -5)?;
    demo_store.insert_u4("mode", 9)?;
    demo_store.insert_i4("delta", -3)?;
    demo_store.insert_bool("enabled", true)?;

    let mut wrote_once = false;

    loop {
        match nfc.poll_tag(Duration::from_millis(1000)) {
            Ok(Some(tag)) => {
                info!(
                    "Tag UID: {:02X?}, ATQA={:02X?}, SAK=0x{:02X}",
                    tag.uid, tag.atqa, tag.sak
                );

                match nfc.read_kv_store() {
                    Ok(store) => info!("Tag key-value data: {:?}", store.entries()),
                    Err(NfcError::NoNdefMessage) => {
                        info!("Tag is empty or does not contain NDEF yet")
                    }
                    Err(err) => warn!("read_kv_store error: {err}"),
                }

                if WRITE_DEMO && !wrote_once {
                    match nfc.write_kv_store(&demo_store) {
                        Ok(()) => {
                            info!("write_kv_store OK: {:?}", demo_store.entries());
                            match nfc.read_kv_store() {
                                Ok(read_back) => {
                                    info!("read-back after write: {:?}", read_back.entries());
                                    if read_back == demo_store {
                                        info!("read-back matches written data");
                                        wrote_once = true;
                                    } else {
                                        warn!("read-back differs from written data");
                                    }
                                }
                                Err(err) => warn!("read-back error after write: {err}"),
                            }
                        }
                        Err(err) => warn!("write_kv_store error: {err}"),
                    }
                }
                FreeRtos::delay_ms(300);
            }
            Ok(None) => {}
            Err(e) => {
                error!("PN532 error: {e}");
                FreeRtos::delay_ms(200);
            }
        }
    }
}
