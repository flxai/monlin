use std::collections::{HashMap, VecDeque};
use std::env;

use crate::color::{gradient_for, interpolate, paint};
use crate::config::{Align, Config};
use crate::layout::{split_even_width, MetricKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Renderer {
    Braille,
    Block,
}

const BLOCKS: &[char] = &[' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
const BRAILLE_LEFT_BITS: [u8; 4] = [6, 2, 1, 0];
const BRAILLE_RIGHT_BITS: [u8; 4] = [7, 5, 4, 3];

pub fn render_line(
    config: &Config,
    width: usize,
    color_enabled: bool,
    histories: &HashMap<MetricKind, VecDeque<f64>>,
    metrics: &[MetricKind],
    values: &HashMap<MetricKind, f64>,
) -> String {
    let prefix = config
        .label
        .as_ref()
        .map(|label| format!("{label} "))
        .unwrap_or_default();

    let inner_width = width.saturating_sub(prefix.chars().count());
    let separators = metrics.len().saturating_sub(1);
    let segment_space = inner_width.saturating_sub(separators);
    let widths = split_even_width(segment_space, metrics.len());

    let segments = metrics
        .iter()
        .zip(widths)
        .map(|(metric, width)| {
            let history = histories.get(metric).cloned().unwrap_or_default();
            let value = values.get(metric).copied().unwrap_or(0.0);
            render_segment(
                *metric,
                &history,
                value,
                width,
                config.align,
                config.renderer,
                color_enabled,
            )
        })
        .collect::<Vec<_>>();

    format!("{prefix}{}", segments.join(" "))
}

fn render_segment(
    metric: MetricKind,
    history: &VecDeque<f64>,
    value: f64,
    width: usize,
    align: Align,
    renderer: Renderer,
    color_enabled: bool,
) -> String {
    let label = metric.short_label();
    let usage_text = format!("{:>3.0}%", value * 100.0);
    let fixed = label.chars().count() + 1 + usage_text.chars().count();

    if width <= fixed + 1 {
        return truncate_visible(&format!("{label} {usage_text}"), width);
    }

    let graph_width = width.saturating_sub(fixed + 2);
    let samples = history.iter().copied().collect::<Vec<_>>();
    let graph = match renderer {
        Renderer::Braille => render_braille_graph(&samples, graph_width, metric, color_enabled),
        Renderer::Block => render_block_graph(&samples, graph_width, metric, color_enabled),
    };

    match align {
        Align::Left => format!("{label} {usage_text} {graph}"),
        Align::Right => format!("{label} {graph} {usage_text}"),
    }
}

fn truncate_visible(text: &str, width: usize) -> String {
    text.chars().take(width).collect()
}

fn render_block_graph(
    samples: &[f64],
    width: usize,
    metric: MetricKind,
    color_enabled: bool,
) -> String {
    let samples = resample(samples, width);
    samples
        .into_iter()
        .map(|sample| {
            let idx = (sample.clamp(0.0, 1.0) * (BLOCKS.len() - 1) as f64).round() as usize;
            let ch = BLOCKS[idx.min(BLOCKS.len() - 1)].to_string();
            let color = interpolate(gradient_for(metric), sample);
            paint(&ch, color, color_enabled)
        })
        .collect()
}

fn render_braille_graph(
    samples: &[f64],
    width: usize,
    metric: MetricKind,
    color_enabled: bool,
) -> String {
    let samples = resample(samples, width.saturating_mul(2));
    let mut out = String::new();

    for chunk in samples.chunks(2) {
        let left = quantize_level(chunk.first().copied().unwrap_or(0.0));
        let right = quantize_level(chunk.get(1).copied().unwrap_or(0.0));
        let cell = braille_cell(left, right);
        let intensity = chunk.iter().copied().sum::<f64>() / chunk.len().max(1) as f64;
        let color = interpolate(gradient_for(metric), intensity);
        out.push_str(&paint(&cell.to_string(), color, color_enabled));
    }

    out
}

fn resample(samples: &[f64], target: usize) -> Vec<f64> {
    if target == 0 {
        return Vec::new();
    }
    if samples.is_empty() {
        return vec![0.0; target];
    }
    if samples.len() <= target {
        let mut out = vec![0.0; target - samples.len()];
        out.extend_from_slice(samples);
        return out;
    }

    (0..target)
        .map(|index| {
            let start = index * samples.len() / target;
            let end = ((index + 1) * samples.len() / target).max(start + 1);
            let slice = &samples[start..end];
            slice.iter().sum::<f64>() / slice.len() as f64
        })
        .collect()
}

fn quantize_level(sample: f64) -> usize {
    (sample.clamp(0.0, 1.0) * 4.0).round() as usize
}

fn braille_cell(left_level: usize, right_level: usize) -> char {
    let mut bits = 0_u32;

    for bit in BRAILLE_LEFT_BITS.iter().take(left_level.min(4)) {
        bits |= 1 << u32::from(*bit);
    }
    for bit in BRAILLE_RIGHT_BITS.iter().take(right_level.min(4)) {
        bits |= 1 << u32::from(*bit);
    }

    char::from_u32(0x2800 + bits).unwrap_or(' ')
}

pub fn terminal_width() -> Option<usize> {
    let from_env = env::var("COLUMNS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0);
    if from_env.is_some() {
        return from_env;
    }

    unsafe {
        let mut winsize = WinSize {
            ws_row: 0,
            ws_col: 0,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        if ioctl(1, TIOCGWINSZ, &mut winsize) == 0 && winsize.ws_col > 0 {
            return Some(winsize.ws_col as usize);
        }
    }

    None
}

const TIOCGWINSZ: u64 = 0x5413;

#[repr(C)]
struct WinSize {
    ws_row: u16,
    ws_col: u16,
    ws_xpixel: u16,
    ws_ypixel: u16,
}

unsafe extern "C" {
    fn ioctl(fd: i32, request: u64, ...) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ColorMode, Config};
    use crate::layout::{Layout, MetricKind};

    #[test]
    fn full_braille_cell_is_filled() {
        assert_eq!(braille_cell(4, 4), '⣿');
    }

    #[test]
    fn braille_graph_renders_requested_width() {
        let graph = render_braille_graph(&[0.25, 0.5, 0.75, 1.0], 2, MetricKind::Cpu, false);
        assert_eq!(graph.chars().count(), 2);
    }

    #[test]
    fn line_layout_respects_multiple_segments() {
        let config = Config {
            history: 8,
            interval_ms: 1000,
            align: Align::Left,
            label: None,
            layout: Layout::default(),
            renderer: Renderer::Braille,
            color_mode: ColorMode::Never,
            width: Some(40),
            once: true,
            show_help: false,
        };
        let mut histories = HashMap::new();
        histories.insert(MetricKind::Cpu, VecDeque::from(vec![0.1, 0.2, 0.3]));
        histories.insert(MetricKind::Gpu, VecDeque::from(vec![0.4, 0.5, 0.6]));
        let values = HashMap::from([(MetricKind::Cpu, 0.3), (MetricKind::Gpu, 0.6)]);

        let line = render_line(
            &Config {
                layout: crate::layout::parse_layout_spec("cpu gpu").unwrap(),
                ..config
            },
            40,
            false,
            &histories,
            &[MetricKind::Cpu, MetricKind::Gpu],
            &values,
        );

        assert!(line.contains("cpu"));
        assert!(line.contains("gpu"));
    }
}
