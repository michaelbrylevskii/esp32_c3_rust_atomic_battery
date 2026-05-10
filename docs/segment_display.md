# `segment_display`: удобная обёртка для TM1637

## Что это такое

Модуль [`segment_display`](../src/common/drivers/segment_display/mod.rs) — высокоуровневая обёртка над `tm1637-embedded-hal` для 4-разрядного индикатора с физическим двоеточием между 2 и 3 разрядом.

В проекте он закрывает типовые задачи:

- показать число до 4 знакомест
- управлять выравниванием и ведущими нулями
- показать пару целых чисел в формате `NN:NN`
- независимо включать и мигать двоеточием
- выводить короткий текст
- выводить `Err`
- крутить бегущую строку
- делать это либо синхронно, либо через фоновую задачу без блокировки основной логики

## Что умеет модуль

- синхронный API через `SegmentDisplay4`
- асинхронный API через `AsyncSegmentDisplay4`
- настройка яркости
- поддержка короткого ASCII-текста
- поддержка бегущей строки
- отдельное управление двоеточием
- ограничение числа полных циклов бегущей строки
- импульсный режим двоеточия с настраиваемым периодом

## Важное ограничение по железу

Речь именно о 4-разрядном TM1637-дисплее с двоеточием посередине.

- одновременно доступно 4 знакоместа
- двоеточие физически отдельное, но привязано к центральной части индикатора
- длинные строки можно показывать только бегущей строкой
- статический `ERROR` полностью не помещается, поэтому показывается `Err`

## Основные типы

### `Align`

Выравнивание текста или числа:

- `Align::Left`
- `Align::Right`

### `IntFormat`

Формат вывода целого числа:

```rust
pub struct IntFormat {
    pub align: Align,
    pub leading_zeros: bool,
}
```

- `leading_zeros` имеет смысл в основном при `Align::Right`
- число должно помещаться в 4 знакоместа
- допустимый диапазон `-999..=9999`

### `DisplayConfig`

Настройки низкоуровневой TM1637-обёртки:

```rust
pub struct DisplayConfig {
    pub brightness: Brightness,
    pub delay_us: u32,
}
```

Поля:

- `brightness` — уровень яркости TM1637
- `delay_us` — внутренний шаг протокольной задержки библиотеки `tm1637-embedded-hal`

### `AsyncDisplayConfig`

Настройки фоновой задачи:

```rust
pub struct AsyncDisplayConfig {
    pub worker_tick: Duration,
    pub thread_stack_size: usize,
}
```

Поля:

- `worker_tick` — период пересчёта анимации и мигания в фоновой задаче
- `thread_stack_size` — размер стека фоновой задачи

Значения по умолчанию:

- `worker_tick = 20 ms`
- `thread_stack_size = 4096`

## Два режима работы

### `SegmentDisplay4`

Синхронный API. Все вызовы сразу пишут кадр на дисплей и выполняются в текущем потоке.

Подходит, если:

- нужно просто вывести число или текст
- блокировка на время вывода не критична
- бегущая строка будет редкой и простой

### `AsyncSegmentDisplay4`

Неблокирующий API. Все вызовы обновляют внутренний буфер состояния, а реальный вывод делает фоновая задача.

Подходит, если:

- нужно крутить бегущую строку, пока работает другая логика
- нужно независимо мигать двоеточием
- не хочется блокировать основной цикл приложения

- `show_*` в асинхронном API меняют желаемое состояние кадра
- новая команда заменяет старую
- например, `start_scroll_text(...)` крутится до следующего `show_*` или нового `start_scroll_text(...)`

Дополнительные async-возможности:

- `start_scroll_text_cycles(text, step_delay, cycles)`
- `start_colon_pulse(initial_on, period, on_duration)`

## Пример: синхронный API

```rust
use common::drivers::segment_display::{
    Align, IntFormat, SegmentDisplay4,
};

let mut display = SegmentDisplay4::new(
    p.pins.gpio5, // CLK
    p.pins.gpio6, // DIO
)?;

display.init()?;
display.show_int(42, IntFormat::new().right())?;
display.show_text("AbCd", Align::Left)?;
display.set_colon(true)?;
display.show_int_pair(12, 34)?;
```

## Пример: асинхронный API

```rust
use common::drivers::segment_display::{
    Align, AsyncSegmentDisplay4, IntFormat,
};
use std::time::Duration;

let display = AsyncSegmentDisplay4::new(
    p.pins.gpio5, // CLK
    p.pins.gpio6, // DIO
)?;

display.show_int(42, IntFormat::new().right())?;
display.start_colon_blink(true, Duration::from_millis(500))?;
display.start_scroll_text("no bat", Duration::from_millis(250))?;
display.show_text("AbCd", Align::Left)?;
```

### Пример: пара чисел с импульсом двоеточия

```rust
display.show_int_pair(5, 18)?;
display.start_colon_pulse(
    true,
    Duration::from_secs(1),
    Duration::from_millis(500),
)?;
```

- число на дисплей подаёт код приложения
- фоновая задача только обслуживает двоеточие и анимации
- двоеточие может жить в том же ритме, что и обновление числа в основном коде

## Вывод целого числа

### Пример: обычное число

```rust
display.show_int(42, IntFormat::new().right())?;
```

Результат:

- справа будет `42`
- слева останутся пустые знакоместа

### Пример: ведущие нули

```rust
display.show_int(42, IntFormat::new().right().leading_zeros(true))?;
```

Результат:

- на дисплее будет `0042`

### Пример: выравнивание влево

```rust
display.show_int(42, IntFormat::new().left())?;
```

Результат:

- число будет прижато влево

## Вывод пары чисел `NN:NN`

```rust
display.set_colon(true)?;
display.show_int_pair(12, 34)?;
```

Ограничения:

- `left` должно быть в диапазоне `0..=99`
- `right` должно быть в диапазоне `0..=99`
- двоеточие включается отдельно через `set_colon(...)`, `start_colon_blink(...)` или `start_colon_pulse(...)`

## Управление двоеточием

### Синхронный API

```rust
display.set_colon(true)?;
display.toggle_colon()?;
```

### Асинхронный API

```rust
display.set_colon(true)?;
display.start_colon_blink(true, Duration::from_millis(500))?;
display.stop_colon_blink(false)?;
display.start_colon_pulse(
    true,
    Duration::from_secs(1),
    Duration::from_millis(500),
)?;
```

- двоеточие хранится отдельно от основного кадра
- поэтому можно сменить число или текст, не теряя режим двоеточия
- в асинхронном API мигание полностью живёт в фоновой задаче

## Короткий текст

```rust
display.show_text("AbCd", Align::Left)?;
```

Ограничения:

- поддерживается только ASCII
- текст длиннее 4 символов при статическом выводе обрежется
- для длинного текста нужна бегущая строка

Если нужен ровно один проход строки, а не бесконечная прокрутка:

```rust
display.start_scroll_text_cycles(
    "1500",
    Duration::from_millis(150),
    Some(1),
)?;
```

## `ERROR`

### Статически

```rust
display.show_error()?;
```

На 4 знакоместах это будет `Err`.

### Бегущей строкой

```rust
display.start_scroll_error(Duration::from_millis(250))?;
```

или в синхронном API:

```rust
display.scroll_error_once(Duration::from_millis(250))?;
```

## Бегущая строка

### Синхронный API

```rust
display.scroll_text_once("Error", Duration::from_millis(250))?;
```

Это блокирующий вариант:

- пока строка крутится, текущий поток занят

### Асинхронный API

```rust
display.start_scroll_text("no bat", Duration::from_millis(250))?;
```

Это неблокирующий вариант:

- команда только обновляет буфер
- строка крутится в фоне
- основной код продолжает работать

## Типичный сценарий для асинхронной обёртки

Паттерн использования обычно такой:

1. создать `AsyncSegmentDisplay4`
2. один раз установить исходное состояние, например `clear()`
3. при смене состояния приложения посылать новую display-команду
4. не дёргать `start_scroll_text(...)` на каждой итерации цикла без необходимости

То есть лучше так:

```rust
if new_state != old_state {
    match new_state {
        State::Idle => display.clear()?,
        State::NoBattery => display.start_scroll_text("no bat", Duration::from_millis(250))?,
    }
}
```

А не так:

```rust
loop {
    display.start_scroll_text("no bat", Duration::from_millis(250))?;
}
```

Во втором случае ты будешь постоянно перезапускать анимацию.

## Ошибки

### `DisplayError::IntegerOutOfRange`

Число не помещается в 4 знакоместа.

### `DisplayError::PairLeftOutOfRange`

Левое значение пары вне диапазона `0..=99`.

### `DisplayError::PairRightOutOfRange`

Правое значение пары вне диапазона `0..=99`.

### `DisplayError::NonAsciiText`

В `show_text()` или `scroll_text_once()` передан не-ASCII текст.

### `AsyncDisplayError::InvalidWorkerTick`

Для фоновой задачи передан `worker_tick = 0`.

### `AsyncDisplayError::InvalidAnimationDelay`

Для мигания или бегущей строки передан нулевой интервал.

### `AsyncDisplayError::WorkerFailed(...)`

Фоновая задача завершилась с ошибкой.

В этом случае полезно проверить:

- `last_worker_error()`
- питание и подключение дисплея
- состояние GPIO

## Практические замечания

- В проекте TM1637 подключён по двум GPIO, а не через I2C peripheral ESP32
- `delay_us` в builder-конфиге — шаг внутреннего TM1637-протокола, а не пользовательская пауза между кадрами
- Для длительных анимаций лучше использовать `AsyncSegmentDisplay4`
- Для разовых простых выводов `SegmentDisplay4` тоже остаётся нормальным вариантом

## Что уже есть в проекте

Пример использования находится в [src/bin/display_demo/main.rs](../src/bin/display_demo/main.rs#L1).

Там уже показаны:

- число с обычным выравниванием
- число с ведущими нулями
- пара чисел `NN:NN`
- мигающее двоеточие
- `Err`
- короткий ASCII-текст
- бегущая строка

## Кратко

Если совсем коротко:

- `SegmentDisplay4` — синхронная обёртка
- `AsyncSegmentDisplay4` — асинхронная обёртка с буфером состояния и фоновой задачей
- для реальной логики приложения чаще удобнее асинхронный API
- для 4-разрядного TM1637 ограничения по длине текста неизбежны, поэтому бегущая строка — штатный путь
