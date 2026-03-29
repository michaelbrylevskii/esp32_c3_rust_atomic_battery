use esp32_c3_rust_atomic_battery::app::demo;
use esp_idf_svc::hal::delay::FreeRtos;

fn main() {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("Starting nfc_demo binary");

    if let Err(err) = demo::nfc_demo() {
        log::error!("Fatal error: {err:?}");
        loop {
            FreeRtos::delay_ms(1000);
        }
    }
}
