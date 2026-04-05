# ESP32-C3 Rust Atomic Battery

Проект для `ESP32-C3 Super Mini` на `Rust + ESP-IDF (managed)` без Arduino-обёрток.

Это прошивка для устройства из RPG в реальном мире в духе Fallout:

- есть устройство-потребитель атомной энергии
- есть атомная батарейка
- на батарейке лежит NFC-метка с её атрибутами

Модель работы:

- ридер на стороне потребителя находит NFC-метку батареи
- тумблер включает или выключает само устройство
- если батарея обнаружена и устройство активировано, прошивка начинает расходовать её заряд
- помимо батареи есть сервисная NFC-метка для настройки скорости потребления

На текущем этапе поддерживаются два прикладных формата меток:

- `battery`:
  - `capacity`
  - `charge`
  - `healthy`
  - `dirty`
  - `session_id`
- `service`:
  - `service_type = "consumption_config"`
  - `consumption_per_sec`

Игровая логика ещё в проработке, поэтому код и документация пока описывают её на общем уровне.

В проекте есть:

- NFC через `PN532`
- 4-разрядный дисплей `TM1637`
- асинхронная LED-индикация с digital и PWM backend'ами
- основной `app`-bin
- отдельные demo-bin для проверки железа

## Что нужно на другой машине

Минимум:

- `rustup`
- `git`
- `cmake`
- `ninja`
- `make`
- `gcc`
- `pkgconf` или `pkg-config`
- `python`
- `cargo-espflash`
- `ldproxy`

Для Manjaro/Arch обычно достаточно:

```bash
sudo pacman -S --needed git cmake ninja make gcc pkgconf python
```

Rust toolchain:

```bash
rustup toolchain install nightly --component rust-src
```

В проекте закреплён nightly через `rust-toolchain.toml`, поэтому отдельно писать `+nightly` обычно не нужно.

Cargo-инструменты:

```bash
cargo install ldproxy cargo-espflash cargo-generate
```

## Доступ к USB / serial на Linux

Для Manjaro нужно добавить пользователя в группы:

```bash
sudo usermod -aG uucp,lock $USER
```

После этого нужно перелогиниться.

Если порт ведёт себя нестабильно, проверь `ModemManager`: он может перехватывать устройство.

## Как собрать проект

Проверка всех таргетов:

```bash
cargo check --all-targets
```

Полная сборка всех таргетов:

```bash
cargo build --all-targets
```

Сборка только основного приложения:

```bash
cargo build --bin app
```

Сборка demo-таргетов:

```bash
cargo build --bin battery_tag_demo
cargo build --bin service_tag_demo
cargo build --bin display_demo
cargo build --bin led_indicator_demo
```

Форматирование:

```bash
cargo fmt
```

## Как прошивать

Основной runtime:

```bash
cargo espflash flash --bin app --monitor
```

Battery tag demo:

```bash
cargo espflash flash --bin battery_tag_demo --monitor
```

Service tag demo:

```bash
cargo espflash flash --bin service_tag_demo --monitor
```

Display demo:

```bash
cargo espflash flash --bin display_demo --monitor
```

LED indicator demo:

```bash
cargo espflash flash --bin led_indicator_demo --monitor
```

В проекте несколько binary target, поэтому `cargo espflash flash` нужно вызывать с явным `--bin ...`.

## Структура проекта

```text
src/
  common/
    lib.rs
    drivers/
      mod.rs
      led_indicator/
        mod.rs
        constants.rs
        backend.rs
        async_controller.rs
        digital_backend.rs
        easing.rs
        pattern.rs
        pwm_backend.rs
      nfc_tag/
        mod.rs
        constants.rs
        format.rs
        sync_nfc.rs
        async_nfc.rs
        esp_idf.rs
      segment_display/
        mod.rs
        constants.rs
        types.rs
        frame.rs
        sync_display.rs
        async_display.rs
        worker.rs
    utils/
      mod.rs
      atomic_tags.rs
      kv_store.rs

  bin/
    app/
      main.rs
      errors.rs
      atomic_machine.rs
      hardware.rs
      storage.rs

    battery_tag_demo/
      main.rs

    service_tag_demo/
      main.rs

    display_demo/
      main.rs

    led_indicator_demo/
      main.rs
```

Что где лежит:

- `src/common` — общий код
- `src/common/drivers` — hardware-обёртки и кастомные драйвера
- `src/common/drivers/led_indicator` — асинхронная LED-индикация, паттерны и backend'ы
- `src/common/drivers/nfc_tag` — NFC driver, разбитый на sync/async/format/esp-idf слои
- `src/common/utils` — общие утилиты и прикладные модели
- `src/common/utils/kv_store.rs` — общий key-value формат `KV1`
- `src/bin/app` — основной бинарник приложения
- `src/bin/app/atomic_machine.rs` — текущий runtime и логика "атомной машины"
- `src/bin/app/hardware.rs` — wiring и инициализация железа
- `src/bin/app/storage.rs` — энергонезависимые настройки и состояние через NVS
- `src/bin/battery_tag_demo` — запись и проверка базовой структуры battery-tag
- `src/bin/service_tag_demo` — запись и проверка базовой структуры service-tag
- `src/bin/display_demo` — изолированный demo-таргет для TM1637
- `src/bin/led_indicator_demo` — демонстрация digital backend для on-board LED и PWM backend для внешних LED

## Полезные файлы

- NFC wrapper: [src/common/drivers/nfc_tag/mod.rs](/mnt/data/Files/Projects/esp32_c3_rust_atomic_battery/src/common/drivers/nfc_tag/mod.rs)
- LED indicator wrapper: [src/common/drivers/led_indicator/mod.rs](/mnt/data/Files/Projects/esp32_c3_rust_atomic_battery/src/common/drivers/led_indicator/mod.rs)
- Display wrapper: [src/common/drivers/segment_display/mod.rs](/mnt/data/Files/Projects/esp32_c3_rust_atomic_battery/src/common/drivers/segment_display/mod.rs)
- KV store: [src/common/utils/kv_store.rs](/mnt/data/Files/Projects/esp32_c3_rust_atomic_battery/src/common/utils/kv_store.rs)
- Документация по NFC: [docs/nfc_tag.md](/mnt/data/Files/Projects/esp32_c3_rust_atomic_battery/docs/nfc_tag.md)
- Документация по KV store: [docs/kv_store.md](/mnt/data/Files/Projects/esp32_c3_rust_atomic_battery/docs/kv_store.md)
- Документация по LED: [docs/led_indicator.md](/mnt/data/Files/Projects/esp32_c3_rust_atomic_battery/docs/led_indicator.md)
- Документация по дисплею: [docs/segment_display.md](/mnt/data/Files/Projects/esp32_c3_rust_atomic_battery/docs/segment_display.md)

## Аппаратные замечания

Для текущего проекта используются такие подключения:

- `PN532`
  - `GPIO3 -> SDA`
  - `GPIO4 -> SCL`
- `TM1637`
  - `GPIO5 -> CLK`
  - `GPIO6 -> DIO`

На `ESP32-C3 Super Mini` лучше осторожно относиться к этим пинам:

- `GPIO2`, `GPIO8`, `GPIO9` — strapping pins
- `GPIO12..17` — обычно flash
- `GPIO18/19` — USB
- `GPIO20/21` — UART0

## Дополнительно

Документация на hardware-обёртки:

- NFC: [docs/nfc_tag.md](/mnt/data/Files/Projects/esp32_c3_rust_atomic_battery/docs/nfc_tag.md)
- Display: [docs/segment_display.md](/mnt/data/Files/Projects/esp32_c3_rust_atomic_battery/docs/segment_display.md)
