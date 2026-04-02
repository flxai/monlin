use std::collections::{HashMap, VecDeque};
use std::env;

use crate::color::{gradient_for, interpolate, paint, split_gradients_for};
use crate::config::{Align, Config};
use crate::layout::{Layout, LayoutItem, MetricKind};
use crate::metrics::{HeadlineValue, MetricValue};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Renderer {
    Braille,
    Block,
}

const BLOCKS: &[char] = &[' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
const BRAILLE_LEFT_BITS: [u8; 4] = [6, 2, 1, 0];
const BRAILLE_RIGHT_BITS: [u8; 4] = [7, 5, 4, 3];

pub fn render_lines(
    config: &Config,
    width: usize,
    color_enabled: bool,
    histories: &HashMap<MetricKind, VecDeque<MetricValue>>,
    layout: &Layout,
    values: &HashMap<MetricKind, MetricValue>,
) -> Vec<String> {
    let headline_values = HashMap::new();
    render_lines_with_headlines(config, width, color_enabled, histories, layout, values, &headline_values)
}

pub fn render_lines_with_headlines(
    config: &Config,
    width: usize,
    color_enabled: bool,
    histories: &HashMap<MetricKind, VecDeque<MetricValue>>,
    layout: &Layout,
    values: &HashMap<MetricKind, MetricValue>,
    headline_values: &HashMap<MetricKind, HeadlineValue>,
) -> Vec<String> {
    let label_width = layout
        .metrics()
        .iter()
        .map(|metric| metric.short_label().chars().count())
        .max()
        .unwrap_or(0);

    layout
        .rows()
        .iter()
        .map(|items| {
            render_row(
                config,
                width,
                color_enabled,
                histories,
                items,
                values,
                headline_values,
                label_width,
            )
        })
        .collect()
}

fn render_row(
    config: &Config,
    width: usize,
    color_enabled: bool,
    histories: &HashMap<MetricKind, VecDeque<MetricValue>>,
    items: &[LayoutItem],
    values: &HashMap<MetricKind, MetricValue>,
    headline_values: &HashMap<MetricKind, HeadlineValue>,
    label_width: usize,
) -> String {
    let prefix = config
        .label
        .as_ref()
        .map(|label| format!("{label} "))
        .unwrap_or_default();

    let inner_width = width.saturating_sub(prefix.chars().count());
    let separators = items.len().saturating_sub(1);
    let segment_space = inner_width.saturating_sub(separators);
    let widths = split_weighted_width(segment_space, items);

    let segments = items
        .iter()
        .zip(widths)
        .map(|(item, width)| {
            let metric = item.metric();
            let history = histories.get(&metric).cloned().unwrap_or_default();
            let value = values
                .get(&metric)
                .copied()
                .unwrap_or(MetricValue::Single(0.0));
            render_segment_with_headline(
                *item,
                &history,
                value,
                headline_values.get(&metric).copied(),
                width,
                label_width,
                config.align,
                config.renderer,
                color_enabled,
            )
        })
        .collect::<Vec<_>>();

    pad_or_trim_visible(&format!("{prefix}{}", segments.join(" ")), width)
}

fn render_segment(
    item: LayoutItem,
    history: &VecDeque<MetricValue>,
    value: MetricValue,
    width: usize,
    label_width: usize,
    align: Align,
    renderer: Renderer,
    color_enabled: bool,
) -> String {
    render_segment_with_headline(
        item,
        history,
        value,
        None,
        width,
        label_width,
        align,
        renderer,
        color_enabled,
    )
}

fn split_weighted_width(total: usize, items: &[LayoutItem]) -> Vec<usize> {
    if items.is_empty() {
        return Vec::new();
    }

    let bases = items
        .iter()
        .map(|item| item.basis().unwrap_or(0))
        .collect::<Vec<_>>();
    let base_total = bases.iter().sum::<usize>();

    if base_total >= total {
        let weights = items
            .iter()
            .zip(&bases)
            .map(|(item, basis)| {
                if *basis > 0 {
                    *basis
                } else if item.grow() > 0 {
                    item.grow()
                } else {
                    1
                }
            })
            .collect::<Vec<_>>();
        let total_weight = weights.iter().sum::<usize>().max(1);
        let mut widths = weights
            .iter()
            .map(|weight| total.saturating_mul(*weight) / total_weight)
            .collect::<Vec<_>>();
        let allocated = widths.iter().sum::<usize>();
        let mut remaining = total.saturating_sub(allocated);
        let mut index = 0;
        let len = widths.len();
        while remaining > 0 {
            widths[index % len] += 1;
            remaining -= 1;
            index += 1;
        }
        return widths;
    }

    let mut widths = bases;
    let mut remaining = total - base_total;
    let total_grow = items.iter().map(|item| item.grow()).sum::<usize>();
    if total_grow == 0 {
        return widths;
    }

    for (width, item) in widths.iter_mut().zip(items.iter()) {
        *width += remaining.saturating_mul(item.grow()) / total_grow;
    }

    let allocated = widths.iter().sum::<usize>();
    remaining = total.saturating_sub(allocated);
    let mut index = 0;
    let len = widths.len();
    while remaining > 0 {
        if items[index % len].grow() > 0 {
            widths[index % len] += 1;
            remaining -= 1;
        }
        index += 1;
        if index >= len && items.iter().all(|item| item.grow() == 0) {
            break;
        }
    }

    widths
}

fn render_segment_with_headline(
    item: LayoutItem,
    history: &VecDeque<MetricValue>,
    value: MetricValue,
    headline_value: Option<HeadlineValue>,
    width: usize,
    label_width: usize,
    align: Align,
    renderer: Renderer,
    color_enabled: bool,
) -> String {
    let metric = item.metric();
    let label = format!("{:<label_width$}", metric.short_label());
    let usage_text = metric.format_value(
        item.view(),
        value.headline_value(),
        &headline_value.unwrap_or_else(|| HeadlineValue::Scalar(value.headline_value())),
    );
    let label_usage_separator = match metric {
        MetricKind::Vram => "",
        _ => " ",
    };
    let fixed = label.chars().count() + label_usage_separator.chars().count() + usage_text.chars().count();

    if width <= fixed {
        return pad_or_trim_visible(&format!("{label}{label_usage_separator}{usage_text}"), width);
    }

    let graph_width = width.saturating_sub(fixed + 1);
    let samples = history.iter().copied().collect::<Vec<_>>();
    let graph = match renderer {
        Renderer::Braille => render_braille_graph(&samples, graph_width, metric, color_enabled),
        Renderer::Block => render_block_graph(&samples, graph_width, metric, color_enabled),
    };

    pad_or_trim_visible(
        &match align {
            Align::Left => format!("{label}{label_usage_separator}{usage_text} {graph}"),
            Align::Right => format!("{label} {graph} {usage_text}"),
        },
        width,
    )
}

fn pad_or_trim_visible(text: &str, width: usize) -> String {
    let visible = visible_width(text);
    if visible >= width {
        return trim_visible(text, width);
    }

    let mut out = String::with_capacity(text.len() + width.saturating_sub(visible));
    out.push_str(text);
    out.push_str(&" ".repeat(width - visible));
    out
}

fn trim_visible(text: &str, width: usize) -> String {
    let mut out = String::new();
    let mut visible = 0;
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\x1b' && chars.peek() == Some(&'[') {
            out.push(ch);
            out.push(chars.next().unwrap());
            for next in chars.by_ref() {
                out.push(next);
                if ('@'..='~').contains(&next) {
                    break;
                }
            }
            continue;
        }

        if visible >= width {
            break;
        }

        out.push(ch);
        visible += 1;
    }

    if out.contains('\x1b') && !out.ends_with("\x1b[0m") {
        out.push_str("\x1b[0m");
    }

    out
}

fn visible_width(text: &str) -> usize {
    let mut width = 0;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            for next in chars.by_ref() {
                if ('@'..='~').contains(&next) {
                    break;
                }
            }
            continue;
        }
        width += 1;
    }
    width
}

fn render_block_graph(
    samples: &[MetricValue],
    width: usize,
    metric: MetricKind,
    color_enabled: bool,
) -> String {
    let samples = resample_channel(samples, width, MetricValue::headline_value);
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
    samples: &[MetricValue],
    width: usize,
    metric: MetricKind,
    color_enabled: bool,
) -> String {
    if metric.is_split() {
        return render_split_braille_graph(samples, width, metric, color_enabled);
    }

    let samples = resample_channel(samples, width.saturating_mul(2), MetricValue::headline_value);
    let mut out = String::new();

    for chunk in samples.chunks(2) {
        let left = quantize_level(chunk.first().copied().unwrap_or(0.0));
        let right = quantize_level(chunk.get(1).copied().unwrap_or(0.0));
        let cell = braille_cell(left, right);
        let intensity = chunk.iter().copied().reduce(f64::max).unwrap_or(0.0);
        let color = interpolate(gradient_for(metric), intensity);
        out.push_str(&paint(&cell.to_string(), color, color_enabled));
    }

    out
}

fn render_split_braille_graph(
    samples: &[MetricValue],
    width: usize,
    metric: MetricKind,
    color_enabled: bool,
) -> String {
    let uppers = resample_channel(samples, width, MetricValue::upper);
    let lowers = resample_channel(samples, width, MetricValue::lower);
    let (upper_gradient, lower_gradient) =
        split_gradients_for(metric).unwrap_or((gradient_for(metric), gradient_for(metric)));
    let mut out = String::new();

    for index in 0..width {
        let upper = quantize_half_level(uppers.get(index).copied().unwrap_or(0.0));
        let lower = quantize_half_level(lowers.get(index).copied().unwrap_or(0.0));
        let upper_intensity = uppers
            .get(index)
            .copied()
            .unwrap_or(0.0);
        let lower_intensity = lowers.get(index).copied().unwrap_or(0.0);

        out.push_str(&render_split_cell(
            upper,
            lower,
            upper_intensity,
            lower_intensity,
            upper_gradient,
            lower_gradient,
            color_enabled,
        ));
    }

    out
}

fn resample_channel(
    samples: &[MetricValue],
    target: usize,
    channel: fn(MetricValue) -> f64,
) -> Vec<f64> {
    if target == 0 {
        return Vec::new();
    }
    if samples.is_empty() {
        return vec![0.0; target];
    }
    if samples.len() <= target {
        let mut out = vec![0.0; target - samples.len()];
        out.extend(samples.iter().copied().map(channel));
        return out;
    }

    (0..target)
        .map(|index| {
            let start = index * samples.len() / target;
            let end = ((index + 1) * samples.len() / target).max(start + 1);
            let slice = &samples[start..end];
            slice.iter().copied().map(channel).sum::<f64>() / slice.len() as f64
        })
        .collect()
}

fn quantize_level(sample: f64) -> usize {
    (sample.clamp(0.0, 1.0) * 4.0).round() as usize
}

fn quantize_half_level(sample: f64) -> usize {
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

fn braille_half_cell(upper_level: usize, lower_level: usize) -> char {
    const UPPER_BITS: [u8; 4] = [1, 4, 0, 3];
    const LOWER_BITS: [u8; 4] = [2, 5, 6, 7];
    let mut bits = 0_u32;

    for bit in UPPER_BITS.iter().take(upper_level.min(4)) {
        bits |= 1 << u32::from(*bit);
    }
    for bit in LOWER_BITS.iter().take(lower_level.min(4)) {
        bits |= 1 << u32::from(*bit);
    }

    char::from_u32(0x2800 + bits).unwrap_or(' ')
}

fn render_split_cell(
    upper_level: usize,
    lower_level: usize,
    upper_intensity: f64,
    lower_intensity: f64,
    upper_gradient: crate::color::Gradient,
    lower_gradient: crate::color::Gradient,
    color_enabled: bool,
) -> String {
    let mut out = String::new();
    if upper_level > 0 {
        let ch = braille_half_cell(upper_level, 0).to_string();
        let color = interpolate(upper_gradient, upper_intensity);
        out.push_str(&paint(&ch, color, color_enabled));
    }
    if lower_level > 0 {
        let ch = braille_half_cell(0, lower_level).to_string();
        let color = interpolate(lower_gradient, lower_intensity);
        out.push_str(&paint(&ch, color, color_enabled));
    }
    if out.is_empty() {
        out.push('⠀');
    }
    out
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
    use crate::layout::{Layout, LayoutView, MetricKind};

    fn item(metric: MetricKind) -> LayoutItem {
        LayoutItem::new(metric, LayoutView::Default, None, 1)
    }

    #[test]
    fn full_braille_cell_is_filled() {
        assert_eq!(braille_cell(4, 4), '⣿');
    }

    #[test]
    fn braille_graph_renders_requested_width() {
        let graph = render_braille_graph(
            &[
                MetricValue::Single(0.25),
                MetricValue::Single(0.5),
                MetricValue::Single(0.75),
                MetricValue::Single(1.0),
            ],
            2,
            MetricKind::Cpu,
            false,
        );
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
        histories.insert(
            MetricKind::Cpu,
            VecDeque::from(vec![
                MetricValue::Single(0.1),
                MetricValue::Single(0.2),
                MetricValue::Single(0.3),
            ]),
        );
        histories.insert(
            MetricKind::Gpu,
            VecDeque::from(vec![
                MetricValue::Single(0.4),
                MetricValue::Single(0.5),
                MetricValue::Single(0.6),
            ]),
        );
        let values = HashMap::from([
            (MetricKind::Cpu, MetricValue::Single(0.3)),
            (MetricKind::Gpu, MetricValue::Single(0.6)),
        ]);

        let layout = crate::layout::parse_layout_spec("cpu gpu").unwrap();
        let lines = render_lines(
            &Config {
                layout: layout.clone(),
                ..config
            },
            40,
            false,
            &histories,
            &layout,
            &values,
        );

        let line = &lines[0];
        assert!(line.contains("cpu"));
        assert!(line.contains("gpu"));
    }

    #[test]
    fn segment_rendering_uses_exact_requested_width() {
        let segment = render_segment(
            item(MetricKind::Cpu),
            &VecDeque::from(vec![
                MetricValue::Single(0.1),
                MetricValue::Single(0.2),
                MetricValue::Single(0.3),
                MetricValue::Single(0.4),
            ]),
            MetricValue::Single(0.4),
            18,
            4,
            Align::Left,
            Renderer::Braille,
            false,
        );

        assert_eq!(visible_width(&segment), 18);
    }

    #[test]
    fn whole_line_respects_requested_width() {
        let config = Config {
            history: 8,
            interval_ms: 1000,
            align: Align::Left,
            label: Some("host".to_owned()),
            layout: crate::layout::parse_layout_spec("cpu gpu").unwrap(),
            renderer: Renderer::Braille,
            color_mode: ColorMode::Never,
            width: Some(40),
            once: true,
            show_help: false,
        };
        let histories = HashMap::from([
            (
                MetricKind::Cpu,
                VecDeque::from(vec![
                    MetricValue::Single(0.1),
                    MetricValue::Single(0.2),
                    MetricValue::Single(0.3),
                    MetricValue::Single(0.4),
                ]),
            ),
            (
                MetricKind::Gpu,
                VecDeque::from(vec![
                    MetricValue::Single(0.5),
                    MetricValue::Single(0.6),
                    MetricValue::Single(0.7),
                    MetricValue::Single(0.8),
                ]),
            ),
        ]);
        let values = HashMap::from([
            (MetricKind::Cpu, MetricValue::Single(0.4)),
            (MetricKind::Gpu, MetricValue::Single(0.8)),
        ]);

        let lines = render_lines(
            &config,
            40,
            false,
            &histories,
            &config.layout,
            &values,
        );

        let line = &lines[0];
        assert_eq!(visible_width(&line), 40);
    }

    #[test]
    fn weighted_rows_split_width_proportionally() {
        let widths = split_weighted_width(
            14,
            &[
                LayoutItem::new(MetricKind::Cpu, LayoutView::Default, None, 3),
                LayoutItem::new(MetricKind::Memory, LayoutView::Default, None, 4),
            ],
        );

        assert_eq!(widths, vec![6, 8]);
    }

    #[test]
    fn basis_and_grow_split_width_as_expected() {
        let widths = split_weighted_width(
            30,
            &[
                LayoutItem::new(MetricKind::Cpu, LayoutView::Default, Some(12), 2),
                LayoutItem::new(MetricKind::Memory, LayoutView::Default, Some(10), 0),
                LayoutItem::new(MetricKind::Net, LayoutView::Default, None, 1),
            ],
        );

        assert_eq!(widths, vec![18, 10, 2]);
    }

    #[test]
    fn multiline_layout_renders_multiple_rows() {
        let layout = crate::layout::parse_layout_spec("all").unwrap();
        let config = Config {
            history: 8,
            interval_ms: 1000,
            align: Align::Left,
            label: None,
            layout: layout.clone(),
            renderer: Renderer::Braille,
            color_mode: ColorMode::Never,
            width: Some(80),
            once: true,
            show_help: false,
        };
        let histories = HashMap::new();
        let values = HashMap::from([
            (
                MetricKind::Sys,
                MetricValue::Split {
                    upper: 0.1,
                    lower: 0.2,
                },
            ),
            (MetricKind::Cpu, MetricValue::Single(0.1)),
            (MetricKind::Memory, MetricValue::Single(0.2)),
            (
                MetricKind::Gfx,
                MetricValue::Split {
                    upper: 0.3,
                    lower: 0.4,
                },
            ),
            (MetricKind::Gpu, MetricValue::Single(0.3)),
            (MetricKind::Vram, MetricValue::Single(0.4)),
            (
                MetricKind::Io,
                MetricValue::Split {
                    upper: 0.5,
                    lower: 0.6,
                },
            ),
            (
                MetricKind::Net,
                MetricValue::Split {
                    upper: 0.7,
                    lower: 0.8,
                },
            ),
            (MetricKind::Ingress, MetricValue::Single(0.7)),
            (MetricKind::Egress, MetricValue::Single(0.8)),
        ]);

        let headlines = values
            .iter()
            .map(|(metric, value)| (*metric, HeadlineValue::Scalar(value.headline_value())))
            .collect::<HashMap<_, _>>();
        let lines = render_lines_with_headlines(&config, 80, false, &histories, &layout, &values, &headlines);

        assert_eq!(lines.len(), 2);
        assert_eq!(visible_width(&lines[0]), 80);
        assert_eq!(visible_width(&lines[1]), 80);
    }

    #[test]
    fn memory_segment_displays_percent_value() {
        let segment = render_segment(
            item(MetricKind::Memory),
            &VecDeque::from(vec![
                MetricValue::Single(0.1),
                MetricValue::Single(0.2),
                MetricValue::Single(0.3),
                MetricValue::Single(0.375),
            ]),
            MetricValue::Single(0.375),
            18,
            4,
            Align::Left,
            Renderer::Braille,
            false,
        );

        assert!(segment.contains("ram"));
        assert!(segment.contains(" 38%"));
    }

    #[test]
    fn combined_net_uses_summed_rate_headline() {
        let layout = crate::layout::parse_layout_spec("net").unwrap();
        let config = Config {
            history: 8,
            interval_ms: 1000,
            align: Align::Left,
            label: None,
            layout: layout.clone(),
            renderer: Renderer::Braille,
            color_mode: ColorMode::Never,
            width: Some(24),
            once: true,
            show_help: false,
        };
        let histories = HashMap::from([(
            MetricKind::Net,
            VecDeque::from(vec![MetricValue::Split {
                upper: 0.5,
                lower: 0.25,
            }]),
        )]);
        let values = HashMap::from([(
            MetricKind::Net,
            MetricValue::Split {
                upper: 0.5,
                lower: 0.25,
            },
        )]);
        let headlines = HashMap::from([(MetricKind::Net, HeadlineValue::Scalar(3.5 * 1024.0 * 1024.0))]);

        let lines =
            render_lines_with_headlines(&config, 24, false, &histories, &layout, &values, &headlines);

        assert!(lines[0].contains("3.5M/s"));
    }

    #[test]
    fn non_vram_metrics_keep_a_space_before_rate_units() {
        let segment = render_segment_with_headline(
            item(MetricKind::Net),
            &VecDeque::new(),
            MetricValue::Split {
                upper: 0.0,
                lower: 0.0,
            },
            Some(HeadlineValue::Scalar(0.0)),
            16,
            3,
            Align::Left,
            Renderer::Braille,
            false,
        );

        assert!(segment.contains("net 0B/s"));
    }

    #[test]
    fn vram_keeps_compact_label_value_spacing() {
        let segment = render_segment_with_headline(
            item(MetricKind::Vram),
            &VecDeque::new(),
            MetricValue::Single(0.0),
            Some(HeadlineValue::Scalar(0.0)),
            16,
            4,
            Align::Left,
            Renderer::Braille,
            false,
        );

        assert!(segment.contains("vram0%"));
    }

    #[test]
    fn storage_human_segment_displays_used_over_total() {
        let segment = render_segment_with_headline(
            LayoutItem::new(MetricKind::Storage, LayoutView::Hum, None, 1),
            &VecDeque::new(),
            MetricValue::Single(0.5),
            Some(HeadlineValue::Storage {
                used_bytes: 512 * 1024 * 1024,
                total_bytes: 2 * 1024 * 1024 * 1024,
            }),
            22,
            3,
            Align::Left,
            Renderer::Braille,
            false,
        );

        assert!(segment.contains("spc 512M/2.0G"));
    }

    #[test]
    fn braille_cell_coloring_prefers_spike_over_average() {
        let graph = render_braille_graph(
            &[MetricValue::Single(0.05), MetricValue::Single(1.0)],
            1,
            MetricKind::Cpu,
            true,
        );
        assert!(graph.contains("\x1b[38;2;169;219;255m"));
    }

    #[test]
    fn colored_segment_keeps_label_uncolored() {
        let segment = render_segment(
            item(MetricKind::Cpu),
            &VecDeque::from(vec![
                MetricValue::Single(0.1),
                MetricValue::Single(0.2),
                MetricValue::Single(0.9),
                MetricValue::Single(1.0),
            ]),
            MetricValue::Single(1.0),
            18,
            4,
            Align::Left,
            Renderer::Braille,
            true,
        );

        assert!(segment.starts_with("cpu  100% "));
        assert_eq!(visible_width(&segment), 18);
    }

    #[test]
    fn split_metric_uses_top_and_bottom_halves() {
        let graph = render_braille_graph(
            &[MetricValue::Split {
                upper: 1.0,
                lower: 0.0,
            }],
            1,
            MetricKind::Net,
            false,
        );
        assert_eq!(graph.chars().count(), 1);
        assert_ne!(graph, " ");
    }

    #[test]
    fn net_uses_different_colors_for_upper_and_lower_halves() {
        let graph = render_braille_graph(
            &[MetricValue::Split {
                upper: 1.0,
                lower: 1.0,
            }],
            1,
            MetricKind::Net,
            true,
        );

        assert!(graph.contains("\x1b[38;2;156;244;255m"));
        assert!(graph.contains("\x1b[38;2;239;173;255m"));
    }

    #[test]
    fn io_uses_different_colors_for_write_and_read_halves() {
        let graph = render_braille_graph(
            &[MetricValue::Split {
                upper: 1.0,
                lower: 1.0,
            }],
            1,
            MetricKind::Io,
            true,
        );

        assert!(graph.contains("\x1b[38;2;255;224;120m"));
        assert!(graph.contains("\x1b[38;2;235;255;143m"));
    }

    #[test]
    fn sys_uses_cpu_and_ram_colors_for_upper_and_lower_halves() {
        let graph = render_braille_graph(
            &[MetricValue::Split {
                upper: 1.0,
                lower: 1.0,
            }],
            1,
            MetricKind::Sys,
            true,
        );

        assert!(graph.contains("\x1b[38;2;169;219;255m"));
        assert!(graph.contains("\x1b[38;2;255;176;176m"));
    }

    #[test]
    fn gfx_uses_gpu_and_vram_colors_for_upper_and_lower_halves() {
        let graph = render_braille_graph(
            &[MetricValue::Split {
                upper: 1.0,
                lower: 1.0,
            }],
            1,
            MetricKind::Gfx,
            true,
        );

        assert!(graph.contains("\x1b[38;2;173;255;191m"));
        assert!(graph.contains("\x1b[38;2;181;255;214m"));
    }

    #[test]
    fn renamed_labels_show_up_in_segments() {
        let sys = render_segment(
            item(MetricKind::Sys),
            &VecDeque::from(vec![MetricValue::Split {
                upper: 0.5,
                lower: 0.25,
            }]),
            MetricValue::Split {
                upper: 0.5,
                lower: 0.25,
            },
            18,
            4,
            Align::Left,
            Renderer::Braille,
            false,
        );
        let gpu = render_segment(
            item(MetricKind::Gpu),
            &VecDeque::from(vec![MetricValue::Single(0.5)]),
            MetricValue::Single(0.5),
            18,
            4,
            Align::Left,
            Renderer::Braille,
            false,
        );
        let gfx = render_segment(
            item(MetricKind::Gfx),
            &VecDeque::from(vec![MetricValue::Split {
                upper: 0.5,
                lower: 0.25,
            }]),
            MetricValue::Split {
                upper: 0.5,
                lower: 0.25,
            },
            18,
            4,
            Align::Left,
            Renderer::Braille,
            false,
        );
        let vram = render_segment(
            item(MetricKind::Vram),
            &VecDeque::from(vec![MetricValue::Single(0.5)]),
            MetricValue::Single(0.5),
            18,
            4,
            Align::Left,
            Renderer::Braille,
            false,
        );
        let ram = render_segment(
            item(MetricKind::Memory),
            &VecDeque::from(vec![MetricValue::Single(0.5)]),
            MetricValue::Single(0.5),
            18,
            4,
            Align::Left,
            Renderer::Braille,
            false,
        );

        assert!(sys.contains("sys"));
        assert!(gpu.contains("gpu"));
        assert!(gfx.contains("gfx"));
        assert!(vram.contains("vram"));
        assert!(ram.contains("ram"));
    }

    #[test]
    fn shorter_labels_are_padded_to_match_vram_width() {
        let gpu = render_segment(
            item(MetricKind::Gpu),
            &VecDeque::from(vec![MetricValue::Single(0.5)]),
            MetricValue::Single(0.5),
            18,
            4,
            Align::Left,
            Renderer::Braille,
            false,
        );
        let vram = render_segment(
            item(MetricKind::Vram),
            &VecDeque::from(vec![MetricValue::Single(0.5)]),
            MetricValue::Single(0.5),
            18,
            4,
            Align::Left,
            Renderer::Braille,
            false,
        );

        assert!(gpu.starts_with("gpu   50% "));
        assert!(vram.starts_with("vram50% "));
    }

    #[test]
    fn vram_omits_label_value_separator() {
        let vram = render_segment(
            item(MetricKind::Vram),
            &VecDeque::from(vec![MetricValue::Single(1.0)]),
            MetricValue::Single(1.0),
            18,
            4,
            Align::Left,
            Renderer::Braille,
            false,
        );

        assert!(vram.starts_with("vram100% "));
    }
}
