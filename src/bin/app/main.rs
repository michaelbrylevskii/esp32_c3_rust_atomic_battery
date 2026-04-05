mod errors;
mod hardware;
mod machine;
mod storage;

use esp_idf_svc::hal::delay::FreeRtos;

fn main() {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("Starting app binary");

    if let Err(err) = machine::run() {
        log::error!("Fatal error: {err:?}");
        loop {
            FreeRtos::delay_ms(1000);
        }
    }
}
