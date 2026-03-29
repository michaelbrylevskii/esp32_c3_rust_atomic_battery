use esp32_c3_rust_atomic_battery::app::AppError;
use esp32_c3_rust_atomic_battery::drivers::segment_display::{
    Align, AsyncSegmentDisplay4, IntFormat,
};
use esp_idf_svc::hal::{delay::FreeRtos, peripherals::Peripherals};
use std::time::Duration;

fn main() {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("Starting display_demo binary");

    if let Err(err) = run() {
        log::error!("Fatal error: {err:?}");
        loop {
            FreeRtos::delay_ms(1000);
        }
    }
}

fn run() -> Result<(), AppError> {
    let peripherals = Peripherals::take()?;

    let display = AsyncSegmentDisplay4::new(
        peripherals.pins.gpio5, // CLK
        peripherals.pins.gpio6, // DIO
    )?;

    loop {
        display.show_int(42, IntFormat::new().right())?;
        FreeRtos::delay_ms(1000);

        display.show_int(42, IntFormat::new().right().leading_zeros(true))?;
        FreeRtos::delay_ms(1000);

        display.show_int(42, IntFormat::new().left())?;
        FreeRtos::delay_ms(1000);

        display.set_colon(true)?;
        display.show_int_pair(12, 34)?;
        display.start_colon_blink(true, Duration::from_millis(500))?;
        FreeRtos::delay_ms(3000);
        display.stop_colon_blink(false)?;
        display.show_error()?;
        FreeRtos::delay_ms(1200);

        display.show_text("AbCd", Align::Left)?;
        FreeRtos::delay_ms(1200);

        display.start_scroll_error(Duration::from_millis(250))?;
        FreeRtos::delay_ms(3000);
        display.clear()?;
        FreeRtos::delay_ms(500);
    }
}
