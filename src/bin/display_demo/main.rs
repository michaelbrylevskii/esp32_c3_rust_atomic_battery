use common::drivers::nfc_tag::NfcError;
use common::drivers::segment_display::{
    Align, AsyncDisplayError, AsyncSegmentDisplay4, DisplayError, IntFormat,
};
use common::utils::kv_store::KvFormatError;
use esp_idf_svc::hal::{delay::FreeRtos, i2c::I2cError, peripherals::Peripherals};
use esp_idf_svc::sys::EspError;
use std::fmt;
use std::time::Duration;

#[derive(Debug)]
enum DemoError {
    Esp(EspError),
    Kv(KvFormatError),
    Nfc(NfcError<I2cError>),
    Display(DisplayError),
    AsyncDisplay(AsyncDisplayError),
}

impl From<EspError> for DemoError {
    fn from(value: EspError) -> Self {
        Self::Esp(value)
    }
}

impl From<KvFormatError> for DemoError {
    fn from(value: KvFormatError) -> Self {
        Self::Kv(value)
    }
}

impl From<NfcError<I2cError>> for DemoError {
    fn from(value: NfcError<I2cError>) -> Self {
        Self::Nfc(value)
    }
}

impl From<DisplayError> for DemoError {
    fn from(value: DisplayError) -> Self {
        Self::Display(value)
    }
}

impl From<AsyncDisplayError> for DemoError {
    fn from(value: AsyncDisplayError) -> Self {
        Self::AsyncDisplay(value)
    }
}

impl fmt::Display for DemoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DemoError::Esp(err) => write!(f, "esp error: {err}"),
            DemoError::Kv(err) => write!(f, "kv format error: {err}"),
            DemoError::Nfc(err) => write!(f, "nfc error: {err}"),
            DemoError::Display(err) => write!(f, "display error: {err}"),
            DemoError::AsyncDisplay(err) => write!(f, "async display error: {err}"),
        }
    }
}

impl std::error::Error for DemoError {}

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

fn run() -> Result<(), DemoError> {
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
