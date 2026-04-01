pub mod color;
pub mod config;
pub mod layout;
pub mod metrics;
pub mod render;

use std::collections::{HashMap, VecDeque};
use std::io::{self, IsTerminal, Write};
use std::thread;
use std::time::Duration;

use config::ColorMode;
use layout::MetricKind;

pub fn run<I>(args: I) -> Result<(), String>
where
    I: IntoIterator<Item = String>,
{
    let config = config::parse_args(args)?;
    if config.show_help {
        print!("{}", config::help_text());
        return Ok(());
    }

    let requested_metrics = config.layout.metrics();
    let mut sampler = metrics::Sampler::default();
    sampler
        .prime(requested_metrics)
        .map_err(|error| format!("nxu-cpu: {error}"))?;

    let mut histories = init_histories(requested_metrics, config.history);
    let mut stdout = io::stdout().lock();

    loop {
        if config.interval_ms > 0 {
            thread::sleep(Duration::from_millis(config.interval_ms));
        }

        let values = sampler
            .sample(requested_metrics)
            .map_err(|error| format!("nxu-cpu: {error}"))?;

        for metric in requested_metrics {
            let value = values.get(metric).copied().unwrap_or(0.0);
            if let Some(history) = histories.get_mut(metric) {
                if history.len() == config.history.max(1) {
                    history.pop_front();
                }
                history.push_back(value);
            }
        }

        let width = config
            .width
            .or_else(render::terminal_width)
            .unwrap_or(80)
            .max(16);
        let color_enabled = colors_enabled(config.color_mode);
        let line = render::render_line(
            &config,
            width,
            color_enabled,
            &histories,
            requested_metrics,
            &values,
        );

        write!(stdout, "\r{line}\x1b[K").map_err(|error| error.to_string())?;
        stdout.flush().map_err(|error| error.to_string())?;

        if config.once {
            writeln!(stdout).map_err(|error| error.to_string())?;
            break;
        }
    }

    Ok(())
}

fn init_histories(
    metrics: &[MetricKind],
    history_len: usize,
) -> HashMap<MetricKind, VecDeque<f64>> {
    metrics
        .iter()
        .copied()
        .map(|metric| (metric, VecDeque::with_capacity(history_len.max(1))))
        .collect()
}

fn colors_enabled(mode: ColorMode) -> bool {
    match mode {
        ColorMode::Always => true,
        ColorMode::Never => false,
        ColorMode::Auto => io::stdout().is_terminal(),
    }
}
