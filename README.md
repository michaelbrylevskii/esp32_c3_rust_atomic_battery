# ESP32-C3 Rust Atomic Battery

Проект для `ESP32-C3 Super Mini` на `Rust + ESP-IDF (managed)` без Arduino-обёрток.

Сейчас в проекте есть:

- NFC через `PN532`
- 4-разрядный дисплей `TM1637`
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

В проекте уже закреплён nightly через `rust-toolchain.toml`, поэтому отдельно писать `+nightly` обычно не нужно.

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

Сборка конкретного demo-таргета:

```bash
cargo build --bin nfc_demo
cargo build --bin display_demo
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

NFC demo:

```bash
cargo espflash flash --bin nfc_demo --monitor
```

Display demo:

```bash
cargo espflash flash --bin display_demo --monitor
```

Важно: в проекте несколько binary target, поэтому `cargo espflash flash` нужно вызывать с явным `--bin ...`.

## Структура проекта

```text
src/
  common/
    lib.rs
    drivers/
      mod.rs
      nfc_tag.rs
      segment_display.rs
    utils/
      mod.rs

  bin/
    app/
      main.rs
      errors.rs
      some_other_logic.rs

    nfc_demo/
      main.rs

    display_demo/
      main.rs
```

Что где лежит:

- `src/common` — общий код
- `src/common/drivers` — hardware-обёртки и кастомные драйвера
- `src/common/utils` — общие утилиты
- `src/bin/app` — основной бинарник приложения
- `src/bin/nfc_demo` — изолированный demo-таргет для PN532/NFC
- `src/bin/display_demo` — изолированный demo-таргет для TM1637

## Полезные файлы

- NFC wrapper: [src/common/drivers/nfc_tag.rs](/mnt/data/Files/Projects/esp32_c3_rust_atomic_battery/src/common/drivers/nfc_tag.rs)
- Display wrapper: [src/common/drivers/segment_display.rs](/mnt/data/Files/Projects/esp32_c3_rust_atomic_battery/src/common/drivers/segment_display.rs)
- Документация по NFC: [docs/nfc_tag.md](/mnt/data/Files/Projects/esp32_c3_rust_atomic_battery/docs/nfc_tag.md)
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
