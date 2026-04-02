pub mod color;
pub mod config;
pub mod layout;
pub mod metrics;
pub mod render;

use std::collections::{HashMap, VecDeque};
use std::io::{self, IsTerminal, Write};
use std::thread;
use std::time::Duration;

use config::{ColorMode, OutputMode};
use layout::MetricKind;
use metrics::MetricValue;
use serde_json::json;

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
        .map_err(|error| format!("monlin: {error}"))?;

    let mut histories = init_histories(requested_metrics, config.history);
    match config.output_mode {
        OutputMode::Terminal => run_terminal(&config, &mut sampler, &mut histories),
        OutputMode::I3bar => run_i3bar(&config, &mut sampler, &mut histories),
    }
}

fn run_terminal(
    config: &config::Config,
    sampler: &mut metrics::Sampler,
    histories: &mut HashMap<MetricKind, VecDeque<MetricValue>>,
) -> Result<(), String> {
    let mut stdout = io::stdout().lock();
    let mut rendered_rows = 0;

    loop {
        if config.interval_ms > 0 {
            thread::sleep(Duration::from_millis(config.interval_ms));
        }

        let lines = sample_lines(config, sampler, histories)?;

        if rendered_rows > 0 {
            write!(stdout, "\r").map_err(|error| error.to_string())?;
            if rendered_rows > 1 {
                write!(stdout, "\x1b[{}A", rendered_rows - 1)
                    .map_err(|error| error.to_string())?;
            }
        }

        for (index, line) in lines.iter().enumerate() {
            write!(stdout, "{line}\x1b[K").map_err(|error| error.to_string())?;
            if index + 1 < lines.len() {
                writeln!(stdout).map_err(|error| error.to_string())?;
            }
        }
        stdout.flush().map_err(|error| error.to_string())?;
        rendered_rows = lines.len();

        if config.once {
            writeln!(stdout).map_err(|error| error.to_string())?;
            break;
        }
    }

    Ok(())
}

fn run_i3bar(
    config: &config::Config,
    sampler: &mut metrics::Sampler,
    histories: &mut HashMap<MetricKind, VecDeque<MetricValue>>,
) -> Result<(), String> {
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{}", json!({"version": 1, "click_events": false}))
        .map_err(|error| error.to_string())?;
    writeln!(stdout, "[").map_err(|error| error.to_string())?;

    let mut first = true;
    loop {
        if config.interval_ms > 0 {
            thread::sleep(Duration::from_millis(config.interval_ms));
        }

        let lines = sample_lines(config, sampler, histories)?;
        let frame = i3bar_frame_json(&lines).to_string();

        if !first {
            writeln!(stdout, ",{frame}").map_err(|error| error.to_string())?;
        } else {
            writeln!(stdout, "{frame}").map_err(|error| error.to_string())?;
            first = false;
        }
        stdout.flush().map_err(|error| error.to_string())?;

        if config.once {
            writeln!(stdout, "]").map_err(|error| error.to_string())?;
            break;
        }
    }

    Ok(())
}

fn sample_lines(
    config: &config::Config,
    sampler: &mut metrics::Sampler,
    histories: &mut HashMap<MetricKind, VecDeque<MetricValue>>,
) -> Result<Vec<String>, String> {
    let requested_metrics = config.layout.metrics();
    let sample = sampler
        .sample(requested_metrics)
        .map_err(|error| format!("monlin: {error}"))?;
    let values = &sample.values;
    let headlines = &sample.headlines;

    for metric in requested_metrics {
        if let Some(history) = histories.get_mut(metric) {
            if let Some(value) = values.get(metric).copied() {
                if history.len() == config.history.max(1) {
                    history.pop_front();
                }
                history.push_back(value);
            } else {
                history.clear();
            }
        }
    }

    let width = config
        .width
        .or_else(render::terminal_width)
        .unwrap_or(80)
        .max(16);
    let color_enabled = match config.output_mode {
        OutputMode::I3bar => false,
        OutputMode::Terminal => colors_enabled(config.color_mode),
    };
    let active_layout = config
        .layout
        .retain_available(|metric| values.contains_key(&metric));
    let mut lines = render::render_lines_with_headlines(
        config,
        width,
        color_enabled,
        histories,
        &active_layout,
        values,
        headlines,
    );
    if lines.is_empty() {
        lines.push(String::new());
    }

    Ok(lines)
}

fn i3bar_frame_json(lines: &[String]) -> serde_json::Value {
    serde_json::Value::Array(
        lines.iter()
            .enumerate()
            .map(|(index, line)| {
                json!({
                    "name": format!("monlin-{index}"),
                    "full_text": line,
                    "separator": false,
                    "separator_block_width": 0,
                })
            })
            .collect(),
    )
}

fn init_histories(
    metrics: &[MetricKind],
    history_len: usize,
) -> HashMap<MetricKind, VecDeque<MetricValue>> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i3bar_frame_uses_one_block_per_line() {
        let frame = i3bar_frame_json(&["sys  40% ....".to_owned(), "net 1.5M/s ....".to_owned()]);
        let blocks = frame.as_array().unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0]["name"], "monlin-0");
        assert_eq!(blocks[0]["full_text"], "sys  40% ....");
        assert_eq!(blocks[1]["name"], "monlin-1");
        assert_eq!(blocks[1]["full_text"], "net 1.5M/s ....");
    }
}
