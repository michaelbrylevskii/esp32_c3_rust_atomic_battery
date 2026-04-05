//! Helper-конструкторы и transport glue для `esp-idf-svc`.

use super::r#async::{AsyncNfcConfig, AsyncNfcError, AsyncNfcTag};
use super::sync::NfcTag;
use core::convert::Infallible;
use core::time::Duration;
use esp_idf_svc::hal::gpio::{InputPin, OutputPin};
use esp_idf_svc::hal::i2c::{I2c, I2cConfig, I2cDriver};
use esp_idf_svc::hal::units::Hertz;
use esp_idf_svc::sys::EspError;
use pn532::{i2c::I2CInterface, CountDown, Pn532};
use std::time::Instant;

/// Стандартная скорость I2C для PN532 helper-конструктора.
pub const DEFAULT_BAUDRATE: Hertz = Hertz(100_000);

/// Готовый тип `NfcTag` для `esp-idf` c `I2cDriver` и стандартным таймером.
pub type EspNfcTag<'d> = NfcTag<I2CInterface<I2cDriver<'d>>, StdTimer, 64>;
/// Async-тип `NfcTag` для `esp-idf` с тем же low-level transport.
pub type AsyncEspNfcTag<'d> = AsyncNfcTag<I2CInterface<I2cDriver<'d>>, StdTimer, 64>;

/// Таймер для `pn532::CountDown`, реализованный через `std::time::Instant`.
#[derive(Debug)]
pub struct StdTimer {
    deadline: Option<Instant>,
}

impl StdTimer {
    /// Создаёт таймер для PN532.
    pub fn new() -> Self {
        Self { deadline: None }
    }
}

impl Default for StdTimer {
    fn default() -> Self {
        Self::new()
    }
}

impl CountDown for StdTimer {
    type Time = Duration;

    fn start<T>(&mut self, count: T)
    where
        T: Into<Self::Time>,
    {
        self.deadline = Some(Instant::now() + count.into());
    }

    fn wait(&mut self) -> pn532::nb::Result<(), Infallible> {
        match self.deadline {
            Some(deadline) if Instant::now() >= deadline => Ok(()),
            _ => Err(pn532::nb::Error::WouldBlock),
        }
    }
}

/// Создаёт `NfcTag` из уже готового `I2cDriver`.
pub fn new_with_driver<'d>(i2c: I2cDriver<'d>) -> EspNfcTag<'d> {
    let interface = I2CInterface { i2c };
    let timer = StdTimer::new();
    let pn532 = Pn532::new(interface, timer);
    NfcTag::new(pn532)
}

/// Создаёт `NfcTag`, сам поднимая `I2cDriver` с указанной скоростью шины.
pub fn new<'d, I2C>(
    i2c: I2C,
    sda: impl InputPin + OutputPin + 'd,
    scl: impl InputPin + OutputPin + 'd,
    baudrate: Hertz,
) -> Result<EspNfcTag<'d>, EspError>
where
    I2C: I2c + 'd,
{
    let config = I2cConfig::new().baudrate(baudrate.into());
    let driver = I2cDriver::new(i2c, sda, scl, &config)?;
    Ok(new_with_driver(driver))
}

/// Создаёт `NfcTag` со стандартной скоростью I2C `100 kHz`.
pub fn new_default<'d, I2C>(
    i2c: I2C,
    sda: impl InputPin + OutputPin + 'd,
    scl: impl InputPin + OutputPin + 'd,
) -> Result<EspNfcTag<'d>, EspError>
where
    I2C: I2c + 'd,
{
    new(i2c, sda, scl, DEFAULT_BAUDRATE)
}

/// Создаёт и сразу запускает async NFC worker для `esp-idf`.
pub fn new_async_default<I2C>(
    i2c: I2C,
    sda: impl InputPin + OutputPin + 'static,
    scl: impl InputPin + OutputPin + 'static,
    worker_config: AsyncNfcConfig,
) -> Result<AsyncEspNfcTag<'static>, AsyncNfcError>
where
    I2C: I2c + 'static,
{
    let nfc =
        new_default(i2c, sda, scl).map_err(|err| AsyncNfcError::CommandFailed(err.to_string()))?;
    AsyncNfcTag::new(nfc, worker_config)
}
