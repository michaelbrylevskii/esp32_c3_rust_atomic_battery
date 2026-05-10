# ESP32-C3 Rust Atomic Battery

Прошивка для устройства-потребителя атомной энергии на `ESP32-C3 Super Mini`.
Проект написан на `Rust + ESP-IDF` в managed mode и не использует Arduino-обёртки.

Идея устройства: в реальной RPG есть потребитель энергии, например транспорт или
механизм, и атомная батарейка с NFC-меткой. Устройство считывает параметры
батарейки, расходует заряд при включённом тумблере и умеет принимать сервисные
NFC-метки для настройки скорости потребления.

## Возможности

- Основной runtime `app` для ESP32-C3 Super Mini.
- NFC-обмен с PN532 по I2C.
- Чтение и запись прикладных NFC-меток через компактный KV-формат.
- 4-разрядный TM1637-дисплей с двоеточием.
- Асинхронная LED-индикация с digital и PWM backend'ами.
- Асинхронная запись NFC и NVS, чтобы бизнес-цикл не блокировался на I/O.
- Отдельные demo binary target для NFC, дисплея и LED-индикации.

## Железо

Целевая плата:

- `ESP32-C3 Super Mini`
- 4 MB onboard SPI flash
- 400 KB SRAM

Используемые компоненты:

- `PN532` NFC-модуль в I2C mode.
- `TM1637` 4-разрядный семисегментный дисплей с двоеточием.
- Красный и зелёный внешние LED.
- Тумблер активации устройства.
- Встроенный LED платы, обычно `GPIO8`, active-low.

Текущая распиновка основного приложения:

| Компонент | Назначение | GPIO |
| --- | --- | --- |
| PN532 | SDA | `GPIO3` |
| PN532 | SCL | `GPIO4` |
| TM1637 | CLK | `GPIO5` |
| TM1637 | DIO | `GPIO6` |
| Red LED | output | `GPIO0` |
| Green LED | output | `GPIO1` |
| Switch | input, pull-up | `GPIO10` |
| On-board LED | output, active-low | `GPIO8` |

Для I2C нужны внешние pull-up резисторы на SDA/SCL, обычно `4.7k` к `3.3V`.
Внутренние pull-up ESP32-C3 для нормального I2C слишком слабые.

Пины, которые лучше не занимать без явной причины:

- `GPIO2`, `GPIO8`, `GPIO9` — strapping pins.
- `GPIO12..17` — обычно линии flash.
- `GPIO18/19` — USB D-/D+.
- `GPIO20/21` — UART0.
- `GPIO4..7` пересекаются с JTAG, это важно при аппаратной отладке.

## Архитектура

Проект разделён на общий library crate `common` и несколько binary target.

```text
src/
  common/
    lib.rs
    drivers/
      led_indicator/
      nfc_tag/
      segment_display/
    utils/
      atomic_tags.rs
      kv_store.rs

  bin/
    app/
      main.rs
      errors.rs
      hardware.rs
      machine/
      storage/

    battery_tag_demo/
      main.rs

    service_tag_demo/
      main.rs

    display_demo/
      main.rs

    led_indicator_demo/
      main.rs
```

Основные зоны ответственности:

- `src/common/drivers/nfc_tag` — PN532 wrapper, NDEF/TLV/KV чтение и запись,
  sync API, async worker и ESP-IDF I2C-конфигурация.
- `src/common/drivers/segment_display` — TM1637 wrapper, форматирование чисел,
  пар чисел, текста, бегущей строки и управление двоеточием.
- `src/common/drivers/led_indicator` — универсальная LED-индикация, backend'ы,
  паттерны, easing и асинхронный контроллер.
- `src/common/utils/kv_store.rs` — общий KV-формат для данных на NFC-метках.
- `src/common/utils/atomic_tags.rs` — прикладные структуры battery/service tag.
- `src/bin/app/hardware.rs` — wiring железа и сборка runtime-зависимостей.
- `src/bin/app/machine` — бизнес-логика, события, эффекты и проекции UI.
- `src/bin/app/storage` — sync/async доступ к NVS.

Подробная документация по драйверам:

- [docs/nfc_tag.md](docs/nfc_tag.md)
- [docs/kv_store.md](docs/kv_store.md)
- [docs/segment_display.md](docs/segment_display.md)
- [docs/led_indicator.md](docs/led_indicator.md)

## Логика приложения

Основное приложение построено как лёгкий событийный цикл. Бизнес-логика не
пишет напрямую в железо и не ждёт завершения NFC/NVS-операций. Вместо этого она
обрабатывает события, обновляет состояние и отдаёт эффекты во внешние async
обёртки.

Источники событий:

- изменение положения тумблера;
- появление, изменение или исчезновение NFC-метки;
- завершение NFC-записи;
- завершение операций NVS;
- периодический `Tick`.

Ключевые состояния батареи:

- `healthy=true`, `dirty=false` — батарейка исправна и может быть использована.
- `dirty=true` — батарейка находится в активной сессии или была вынута/заменена
  нештатно.
- `healthy=false` — батарейка сломана и не должна запускать устройство.
- `charge=0` — батарейка разряжена.

Основные правила:

- Если тумблер включён без батарейки, горит красный LED и на дисплее бежит
  `no bat`.
- Если приложена исправная батарейка и тумблер выключен, дисплей показывает
  оставшееся время, красный LED активен.
- Если исправная батарейка приложена и тумблер включён, запускается сессия,
  горит зелёный LED, заряд уменьшается по `consumption_per_sec`.
- При старте сессии батарейка сразу записывается как `dirty=true`.
- При штатном выключении тумблера заряд списывается на метку, `dirty` снимается.
- Если батарейку вставили или вынули при включённом тумблере, батарейка
  считается сломанной.
- Если устройство видит батарейку с `dirty=true`, она считается нештатно
  использованной и ломается.
- Сервисная метка обновляет `consumption_per_sec`, значение сохраняется в NVS и
  переживает перезагрузку.

## Форматы NFC-меток

Проект использует прикладные NFC-метки двух типов. Данные хранятся в KV-формате
поверх NDEF/TLV.

Battery tag:

| Ключ | Тип | Назначение |
| --- | --- | --- |
| `tag_type` | string | всегда `battery` |
| `capacity` | `u64` | полная ёмкость батарейки |
| `charge` | `u64` | текущий остаток заряда |
| `healthy` | boolean | исправность батарейки |
| `dirty` | boolean | признак незавершённой активной сессии |
| `session_id` | `u64` | идентификатор активной сессии |

Service tag:

| Ключ | Тип | Назначение |
| --- | --- | --- |
| `tag_type` | string | всегда `service` |
| `service_type` | string | сейчас `consumption_config` |
| `consumption_per_sec` | `u32` | скорость потребления заряда в секунду |

## Настройка среды

Проект рассчитан на `ESP32-C3`, то есть на RISC-V вариант ESP32. Для этого
проекта не нужен Arduino и не обязателен `espup`: ESP-IDF подтягивается через
managed mode.

В репозитории закреплены важные настройки:

- `rust-toolchain.toml` выбирает `nightly` и компонент `rust-src`.
- `.cargo/config.toml` задаёт target `riscv32imc-esp-espidf`.
- `.cargo/config.toml` задаёт `ESP_IDF_VERSION = "v5.5.3"`.
- `.cargo/config.toml` включает `build-std = ["std", "panic_abort"]`.

Первый build может занять заметное время: `embuild` скачивает ESP-IDF и
toolchain в рабочую директорию проекта.

### Linux

Установи системные зависимости. Для Manjaro/Arch:

```bash
sudo pacman -S --needed git cmake ninja make gcc pkgconf python
```

Для Debian/Ubuntu:

```bash
sudo apt update
sudo apt install -y git cmake ninja-build build-essential pkg-config python3
```

Установи Rust:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup toolchain install nightly --component rust-src
```

Установи cargo-инструменты:

```bash
cargo install ldproxy cargo-espflash cargo-generate
```

Для доступа к USB/serial на Manjaro добавь пользователя в группы:

```bash
sudo usermod -aG uucp,lock $USER
```

После изменения групп нужно перелогиниться. Если serial-порт ведёт себя
нестабильно, проверь `ModemManager`: он может перехватывать устройство.

### Windows

Рекомендуемый минимальный набор:

- Git for Windows.
- Rust через `rustup`.
- Python 3.
- CMake.
- Ninja.
- USB-драйвер для USB-UART чипа платы, если Windows не определяет порт сама.

Пример установки через `winget`:

```powershell
winget install Git.Git
winget install Rustlang.Rustup
winget install Python.Python.3.12
winget install Kitware.CMake
winget install Ninja-build.Ninja
```

После установки Rust:

```powershell
rustup toolchain install nightly --component rust-src
cargo install ldproxy cargo-espflash cargo-generate
```

Собирать и прошивать проект можно из PowerShell, Windows Terminal или терминала
IDE. Если `cargo-espflash` не видит плату, проверь COM-порт и драйвер USB-UART.

## Быстрый старт

```bash
git clone <repo-url>
cd esp32_c3_rust_atomic_battery
cargo check --all-targets
cargo espflash flash --bin app --monitor
```

## Сборка

Проверка всех target:

```bash
cargo check --all-targets
```

Сборка всех target:

```bash
cargo build --all-targets
```

Сборка основного приложения:

```bash
cargo build --bin app
```

Сборка demo target:

```bash
cargo build --bin battery_tag_demo
cargo build --bin service_tag_demo
cargo build --bin display_demo
cargo build --bin led_indicator_demo
```

Release-сборка:

```bash
cargo build --release --bin app
```

Форматирование:

```bash
cargo fmt
```

## Прошивка

В проекте несколько binary target, поэтому binary target нужно указывать явно.

Основное приложение:

```bash
cargo espflash flash --bin app --monitor
```

Demo для записи и проверки battery tag:

```bash
cargo espflash flash --bin battery_tag_demo --monitor
```

Demo для записи и проверки service tag:

```bash
cargo espflash flash --bin service_tag_demo --monitor
```

Demo дисплея:

```bash
cargo espflash flash --bin display_demo --monitor
```

Demo LED-индикации:

```bash
cargo espflash flash --bin led_indicator_demo --monitor
```

Если команда запускается без `--bin`, `cargo-espflash` не имеет однозначного
основного binary target в проекте с несколькими бинарниками.

## Demo target

`battery_tag_demo` записывает и валидирует базовую структуру батарейки на NFC
метке.

`service_tag_demo` записывает и валидирует сервисную метку с параметром
`consumption_per_sec`.

`display_demo` демонстрирует основные возможности TM1637 wrapper:

- вывод целых чисел;
- вывод пары чисел;
- управление двоеточием;
- статический текст;
- бегущую строку.

`led_indicator_demo` демонстрирует LED controller:

- active-high внешние LED;
- active-low on-board LED;
- blink/pulse/pattern;
- PWM backend для плавных переходов, если используется подходящий пин и канал.

## Постоянная память

Основное приложение использует NVS namespace `atomic_app`.

Сейчас сохраняются:

- `cons_per_sec` — текущая скорость потребления заряда;
- `session_ctr` — счётчик session id.

Дефолтная скорость потребления: `1000` единиц заряда в секунду.

## Диагностика

Частые проблемы:

- Нет доступа к serial на Linux: проверь группы `uucp,lock` и перелогинься.
- Порт занят: проверь `ModemManager` или другой serial monitor.
- PN532 не отвечает: проверь I2C mode модуля, SDA/SCL, питание, землю и внешние
  pull-up.
- PN532 завис после reset ESP32: временно помогает передёрнуть питание модуля;
  аппаратный reset pin PN532 стоит подключить отдельным GPIO.
- NFC-метка читается нестабильно: проверь расстояние до антенны, питание PN532
  и качество I2C pull-up.
- Дисплей не показывает данные: проверь CLK/DIO, питание и общий GND.

## Разработка

Перед коммитом стоит запускать:

```bash
cargo fmt
cargo check --all-targets
```

Для проверки конкретного target:

```bash
cargo check --bin app
```

GitHub Actions workflow находится в [.github/workflows/rust_ci.yml](.github/workflows/rust_ci.yml).

## Происхождение кода

Значительная часть проекта спроектирована, написана и отрефакторена при помощи
агентского ИИ `Codex` под инженерным контролем автора проекта.

## Лицензия

Проект распространяется под лицензией [0BSD](LICENSE).

Это permissive-лицензия без attribution-требования: код можно использовать,
копировать, изменять и распространять для любых целей, с оплатой или без. В
лицензии также явно указано, что ПО предоставляется как есть, без гарантий и без
ответственности автора.

## Статус проекта

Проект находится в активной разработке. Базовые драйверы и demo target уже
выделены, основное приложение работает через событийную state machine. Игровая
модель и правила эксплуатации батареек могут уточняться по мере тестов на
реальном железе.
