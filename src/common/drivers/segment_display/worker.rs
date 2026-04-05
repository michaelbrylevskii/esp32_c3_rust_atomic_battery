use super::async_display::{BufferedColonMode, BufferedContent, BufferedState};
use super::constants::DISPLAY_WIDTH;
use super::frame::{apply_colon, countdown_frame, scroll_window_frame};
use super::sync_display::SegmentDisplay4;
use core::time::Duration;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

pub(super) fn run_display_worker(
    mut display: SegmentDisplay4<'static>,
    state: Arc<Mutex<BufferedState>>,
    worker_error: Arc<Mutex<Option<String>>>,
    worker_tick: Duration,
) {
    let mut content_generation = 0u64;
    let mut colon_generation = 0u64;
    let mut brightness_generation = 0u64;
    let mut content_started = Instant::now();
    let mut colon_started = Instant::now();
    let mut last_rendered_frame = [u8::MAX; DISPLAY_WIDTH];

    loop {
        let snapshot = match state.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => {
                store_worker_error(
                    &worker_error,
                    "display worker state mutex poisoned".to_owned(),
                );
                return;
            }
        };

        if snapshot.shutdown {
            return;
        }

        let now = Instant::now();

        if snapshot.content_generation != content_generation {
            content_generation = snapshot.content_generation;
            content_started = now;
        }

        if snapshot.colon_generation != colon_generation {
            colon_generation = snapshot.colon_generation;
            colon_started = now;
        }

        if snapshot.brightness_generation != brightness_generation {
            if let Err(err) = display.set_brightness(snapshot.brightness) {
                store_worker_error(
                    &worker_error,
                    format!("failed to apply display brightness: {err}"),
                );
                return;
            }

            brightness_generation = snapshot.brightness_generation;
        }

        let mut frame = frame_from_content(&snapshot.content, content_started, now);
        apply_colon(&mut frame, colon_is_on(snapshot.colon, colon_started, now));

        if frame != last_rendered_frame {
            if let Err(err) = display.render_frame(frame) {
                store_worker_error(
                    &worker_error,
                    format!("failed to render display frame: {err}"),
                );
                return;
            }

            last_rendered_frame = frame;
        }

        thread::sleep(worker_tick);
    }
}

fn frame_from_content(
    content: &BufferedContent,
    content_started: Instant,
    now: Instant,
) -> [u8; DISPLAY_WIDTH] {
    match content {
        BufferedContent::Static(frame) => *frame,
        BufferedContent::Countdown {
            initial_total_seconds,
            step_period,
        } => countdown_frame(countdown_remaining_seconds(
            *initial_total_seconds,
            content_started,
            now,
            *step_period,
        )),
        BufferedContent::Scroll {
            source,
            step_delay,
            cycles,
        } => {
            let windows = source.len().saturating_sub(DISPLAY_WIDTH) + 1;
            let offset = if windows <= 1 {
                0
            } else {
                let raw_steps = animation_steps(now, content_started, *step_delay);
                match cycles {
                    Some(cycle_count) => {
                        let total_steps = windows.saturating_mul((*cycle_count).max(1) as usize);
                        if raw_steps >= total_steps.saturating_sub(1) {
                            windows - 1
                        } else {
                            raw_steps % windows
                        }
                    }
                    None => raw_steps % windows,
                }
            };

            scroll_window_frame(source, offset)
        }
    }
}

fn colon_is_on(mode: BufferedColonMode, colon_started: Instant, now: Instant) -> bool {
    match mode {
        BufferedColonMode::Static(enabled) => enabled,
        BufferedColonMode::Blink {
            initial_on,
            interval,
        } => {
            if animation_steps(now, colon_started, interval) % 2 == 0 {
                initial_on
            } else {
                !initial_on
            }
        }
        BufferedColonMode::Pulse {
            initial_on,
            period,
            on_duration,
        } => {
            let elapsed = now.duration_since(colon_started).as_millis();
            let period_ms = period.as_millis().max(1);
            let on_duration_ms = on_duration.as_millis().min(period_ms);
            let phase_ms = elapsed % period_ms;
            if phase_ms < on_duration_ms {
                initial_on
            } else {
                !initial_on
            }
        }
    }
}

fn countdown_remaining_seconds(
    initial_total_seconds: u32,
    started_at: Instant,
    now: Instant,
    step_period: Duration,
) -> u32 {
    let elapsed_steps = animation_steps(now, started_at, step_period) as u32;
    initial_total_seconds.saturating_sub(elapsed_steps)
}

fn animation_steps(now: Instant, started_at: Instant, step_delay: Duration) -> usize {
    let step_millis = step_delay.as_millis();
    if step_millis == 0 {
        return 0;
    }

    (now.duration_since(started_at).as_millis() / step_millis) as usize
}

fn store_worker_error(worker_error: &Arc<Mutex<Option<String>>>, error: String) {
    if let Ok(mut slot) = worker_error.lock() {
        *slot = Some(error);
    }
}
