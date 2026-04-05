use common::drivers::nfc_tag::{self, NfcError, TagInfo};
use common::utils::atomic_tags::{AtomicTag, AtomicTagError, ServiceTag};
use common::utils::kv_store::KvFormatError;
use esp_idf_svc::hal::{delay::FreeRtos, i2c::I2cError, peripherals::Peripherals};
use esp_idf_svc::sys::EspError;
use log::{error, info, warn};
use std::fmt;
use std::time::Duration;

const DEMO_CONSUMPTION_PER_SEC: u32 = 1500;
const POLL_TIMEOUT: Duration = Duration::from_millis(1000);
const FIRMWARE_RETRY_DELAY_MS: u32 = 250;
const FIRMWARE_RETRY_ATTEMPTS: usize = 8;

#[derive(Debug)]
enum DemoError {
    Esp(EspError),
    Kv(KvFormatError),
    Nfc(NfcError<I2cError>),
    Tag(AtomicTagError),
    Validation(&'static str),
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

impl From<AtomicTagError> for DemoError {
    fn from(value: AtomicTagError) -> Self {
        Self::Tag(value)
    }
}

impl fmt::Display for DemoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DemoError::Esp(err) => write!(f, "esp error: {err}"),
            DemoError::Kv(err) => write!(f, "kv format error: {err}"),
            DemoError::Nfc(err) => write!(f, "nfc error: {err}"),
            DemoError::Tag(err) => write!(f, "tag error: {err}"),
            DemoError::Validation(err) => write!(f, "validation error: {err}"),
        }
    }
}

impl std::error::Error for DemoError {}

fn main() {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    info!("Starting service_tag_demo binary");

    if let Err(err) = run() {
        error!("Fatal error: {err:?}");
        loop {
            FreeRtos::delay_ms(1000);
        }
    }
}

fn run() -> Result<(), DemoError> {
    let peripherals = Peripherals::take()?;
    let mut nfc = nfc_tag::esp_idf::new_default(
        peripherals.i2c0,
        peripherals.pins.gpio3,
        peripherals.pins.gpio4,
    )?;

    nfc.init_default()?;
    log_firmware_version(&mut nfc);

    let expected = ServiceTag::new(DEMO_CONSUMPTION_PER_SEC)?;
    let mut last_seen_uid_hex: Option<String> = None;

    loop {
        match nfc.poll_tag(POLL_TIMEOUT) {
            Ok(Some(tag_info)) => {
                let uid_hex = encode_uid_hex(&tag_info.uid);
                if last_seen_uid_hex.as_deref() != Some(uid_hex.as_str()) {
                    info!("Detected tag {:02X?}", tag_info.uid);
                    write_and_validate_service_tag(&mut nfc, &tag_info, &expected)?;
                }
                last_seen_uid_hex = Some(uid_hex);
            }
            Ok(None) => {
                last_seen_uid_hex = None;
            }
            Err(err) => {
                warn!("PN532 error while polling service tag: {err}");
                FreeRtos::delay_ms(200);
            }
        }
    }
}

fn log_firmware_version(nfc: &mut nfc_tag::esp_idf::EspNfcTag<'_>) {
    for attempt in 1..=FIRMWARE_RETRY_ATTEMPTS {
        match nfc.firmware_version() {
            Ok(version) => {
                info!("PN532 firmware raw: {:02X?}", version);
                return;
            }
            Err(err) if attempt < FIRMWARE_RETRY_ATTEMPTS => {
                warn!(
                    "PN532 firmware_version attempt {attempt}/{FIRMWARE_RETRY_ATTEMPTS} failed: {err}"
                );
                FreeRtos::delay_ms(FIRMWARE_RETRY_DELAY_MS);
            }
            Err(err) => {
                warn!("PN532 firmware_version skipped after retries: {err}");
            }
        }
    }
}

fn write_and_validate_service_tag(
    nfc: &mut nfc_tag::esp_idf::EspNfcTag<'_>,
    tag_info: &TagInfo,
    expected: &ServiceTag,
) -> Result<(), DemoError> {
    let store = expected.to_store()?;
    nfc.write_kv_store(&store)?;

    let read_back = nfc.read_kv_store()?;
    let parsed = AtomicTag::from_store(&read_back)?;

    match parsed {
        AtomicTag::Service(actual) if actual == *expected => {
            info!(
                "Service tag {:02X?} validated: consumption_per_sec={}",
                tag_info.uid, actual.consumption_per_sec
            );
            Ok(())
        }
        AtomicTag::Service(actual) => {
            warn!("Read-back differs from expected service tag: {:?}", actual);
            Err(DemoError::Validation(
                "service tag read-back differs from expected",
            ))
        }
        AtomicTag::Battery(_) => Err(DemoError::Validation(
            "expected service tag, got battery tag after read-back",
        )),
    }
}

fn encode_uid_hex(uid: &[u8]) -> String {
    let mut value = String::with_capacity(uid.len() * 2);
    for byte in uid {
        use core::fmt::Write as _;
        let _ = write!(&mut value, "{byte:02X}");
    }
    value
}
