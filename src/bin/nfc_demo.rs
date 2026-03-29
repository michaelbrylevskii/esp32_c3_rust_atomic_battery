use esp32_c3_rust_atomic_battery::app::AppError;
use esp32_c3_rust_atomic_battery::drivers::nfc_tag::{self, KvStore, NfcError};
use esp_idf_svc::hal::{delay::FreeRtos, peripherals::Peripherals};
use log::{error, info, warn};

const WRITE_DEMO: bool = false;

fn main() {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("Starting nfc_demo binary");

    if let Err(err) = run() {
        log::error!("Fatal error: {err:?}");
        loop {
            FreeRtos::delay_ms(1000);
        }
    }
}

fn run() -> Result<(), AppError> {
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
    demo_store.insert_string("name", "Привет,\nESP32-C3")?;
    demo_store.insert_u8("counter", 42)?;
    demo_store.insert_u16("limit", 1024)?;
    demo_store.insert_u32("serial", 123_456)?;
    demo_store.insert_u64("energy", 9_876_543_210)?;
    demo_store.insert_i8("temperature_offset", -5)?;
    demo_store.insert_i16("bias", -32_000)?;
    demo_store.insert_i32("temp_raw", -123_456)?;
    demo_store.insert_i64("distance", -9_876_543_210)?;
    demo_store.insert_f32("soc", 98.5)?;
    demo_store.insert_f64("voltage", 12.625)?;
    demo_store.insert_bool("enabled", true)?;

    let mut wrote_once = false;

    loop {
        match nfc.poll_tag(std::time::Duration::from_millis(1000)) {
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
            Err(err) => {
                error!("PN532 error: {err}");
                FreeRtos::delay_ms(200);
            }
        }
    }
}
