use std::collections::{HashMap, VecDeque};
use std::env;

use clap::ValueEnum;

use crate::color::{
    gradient_for_with_hues, interpolate, metric_hues_for_visible_hue, paint,
    split_gradients_for_with_hues, visible_hues, BaseHues, ColorSpec,
};
use crate::config::{Align, Config, Space, StreamLayout};
use crate::layout::{Layout, LayoutItem, LayoutView, MetricKind};
use crate::metrics::{HeadlineValue, MetricValue};

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
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
    render_lines_with_headlines(
        config,
        width,
        color_enabled,
        histories,
        layout,
        values,
        &headline_values,
    )
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
    let total_items = layout.rows().iter().map(|items| items.len()).sum::<usize>();
    let all_hues = visible_hues(total_items, config.colors.as_deref());
    let mut offset = 0;

    layout
        .rows()
        .iter()
        .map(|items| {
            let row_hues = &all_hues[offset..offset + items.len()];
            offset += items.len();
            render_row(
                config,
                width,
                color_enabled,
                histories,
                items,
                values,
                headline_values,
                label_width,
                row_hues,
            )
        })
        .collect()
}

pub fn render_stream_lines(
    config: &Config,
    width: usize,
    color_enabled: bool,
    histories: &[VecDeque<f64>],
    values: &[f64],
) -> Vec<String> {
    if values.is_empty() {
        return vec![String::new()];
    }

    match config.stream_layout {
        StreamLayout::Columns => vec![render_stream_columns_line(
            config,
            width,
            color_enabled,
            histories,
            values,
        )],
        StreamLayout::Lines => {
            render_stream_rows(config, width, color_enabled, histories, values)
        }
    }
}

fn render_stream_rows(
    config: &Config,
    width: usize,
    color_enabled: bool,
    histories: &[VecDeque<f64>],
    values: &[f64],
) -> Vec<String> {
    let prefix = config
        .label
        .as_ref()
        .map(|label| format!("{label} "))
        .unwrap_or_default();
    let stream_labels = config.stream_labels.as_deref().unwrap_or(&[]);
    let label_width = stream_labels
        .iter()
        .map(|label| label.chars().count())
        .max()
        .unwrap_or(0);

    let stream_hues = visible_hues(values.len(), config.colors.as_deref());

    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let row_label = stream_labels
                .get(index)
                .map(|label| format!("{label:>label_width$} "))
                .unwrap_or_default();
            let usage_text = format!("{:>3.0}%", value.clamp(0.0, 1.0) * 100.0);
            let fixed = prefix.chars().count()
                + row_label.chars().count()
                + usage_text.chars().count();

            let graph_width = width.saturating_sub(fixed + 1);
            let metric = stream_metric_for(index);
            let metric_history = histories
                .get(index)
                .map(|history| {
                    history
                        .iter()
                        .copied()
                        .map(MetricValue::Single)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let item_hues = metric_hues_for_visible_hue(
                metric,
                stream_hues[index % stream_hues.len()],
            );
            let graph = match config.renderer {
                Renderer::Braille => {
                    render_braille_graph(
                        &metric_history,
                        graph_width,
                        metric,
                        Some(&item_hues),
                        color_enabled,
                    )
                }
                Renderer::Block => {
                    render_block_graph(
                        &metric_history,
                        graph_width,
                        metric,
                        Some(&item_hues),
                        color_enabled,
                    )
                }
            };

            pad_or_trim_visible(
                &match config.align {
                    Align::Left => format!("{prefix}{row_label}{usage_text} {graph}"),
                    Align::Right => format!("{prefix}{row_label}{graph} {usage_text}"),
                },
                width,
            )
        })
        .collect()
}

fn render_stream_columns_line(
    config: &Config,
    width: usize,
    color_enabled: bool,
    histories: &[VecDeque<f64>],
    values: &[f64],
) -> String {
    let prefix = config
        .label
        .as_ref()
        .map(|label| format!("{label} "))
        .unwrap_or_default();
    let inner_width = width.saturating_sub(prefix.chars().count());
    let stream_hues = visible_hues(values.len(), config.colors.as_deref());
    let segments = values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let label = config
                .stream_labels
                .as_deref()
                .unwrap_or(&[])
                .get(index)
                .cloned()
                .unwrap_or_default();
            let usage_text = format!("{:>3.0}%", value.clamp(0.0, 1.0) * 100.0);
            let separator = if label.is_empty() { "" } else { " " };
            let fixed = label.chars().count() + separator.chars().count() + usage_text.chars().count();

            (index, label, separator, usage_text, fixed)
        })
        .collect::<Vec<_>>();
    let (segment_widths, graph_widths) =
        stream_column_widths(config.space, inner_width, &segments);

    let segments = segments
        .into_iter()
        .zip(segment_widths.into_iter().zip(graph_widths))
        .map(|((index, label, separator, usage_text, _fixed), (segment_width, graph_width))| {
            let metric = stream_metric_for(index);
            let metric_history = histories
                .get(index)
                .map(|history| {
                    history
                        .iter()
                        .copied()
                        .map(MetricValue::Single)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let item_hues = metric_hues_for_visible_hue(
                metric,
                stream_hues[index % stream_hues.len()],
            );
            let graph = match config.renderer {
                Renderer::Braille => {
                    render_braille_graph(
                        &metric_history,
                        graph_width,
                        metric,
                        Some(&item_hues),
                        color_enabled,
                    )
                }
                Renderer::Block => {
                    render_block_graph(
                        &metric_history,
                        graph_width,
                        metric,
                        Some(&item_hues),
                        color_enabled,
                    )
                }
            };
            let text_only = format!("{label}{separator}{usage_text}");

            if graph_width == 0 {
                return pad_or_trim_visible(&text_only, segment_width);
            }

            pad_or_trim_visible(
                &match config.align {
                    Align::Left => format!("{label}{separator}{usage_text} {graph}"),
                    Align::Right => {
                        if label.is_empty() {
                            format!("{graph} {usage_text}")
                        } else {
                            format!("{label} {graph} {usage_text}")
                        }
                    }
                },
                segment_width,
            )
        })
        .collect::<Vec<_>>();

    pad_or_trim_visible(&format!("{prefix}{}", segments.join(" ")), width)
}

fn stream_column_widths(
    space: Space,
    inner_width: usize,
    segments: &[(usize, String, &'static str, String, usize)],
) -> (Vec<usize>, Vec<usize>) {
    let count = segments.len();
    if count == 0 {
        return (Vec::new(), Vec::new());
    }

    let separators = count.saturating_sub(1);
    match space {
        Space::Graph => {
            let fixed_total = segments.iter().map(|(_, _, _, _, fixed)| fixed + 1).sum::<usize>();
            let graph_space = inner_width
                .saturating_sub(separators)
                .saturating_sub(fixed_total);
            let graph_widths = split_stream_widths(graph_space, count);
            let segment_widths = segments
                .iter()
                .zip(graph_widths.iter().copied())
                .map(|((_, _, _, _, fixed), graph_width)| {
                    fixed + usize::from(graph_width > 0) + graph_width
                })
                .collect::<Vec<_>>();
            (segment_widths, graph_widths)
        }
        Space::Segment => {
            let segment_space = inner_width.saturating_sub(separators);
            let segment_widths = split_stream_widths(segment_space, count);
            let graph_widths = segments
                .iter()
                .zip(segment_widths.iter().copied())
                .map(|((_, _, _, _, fixed), segment_width)| {
                    if segment_width <= *fixed {
                        0
                    } else {
                        segment_width.saturating_sub(*fixed + 1)
                    }
                })
                .collect::<Vec<_>>();
            (segment_widths, graph_widths)
        }
    }
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
    row_hues: &[ColorSpec],
) -> String {
    let prefix = config
        .label
        .as_ref()
        .map(|label| format!("{label} "))
        .unwrap_or_default();

    let inner_width = width.saturating_sub(prefix.chars().count());
    let separators = items.len().saturating_sub(1);
    let segment_space = inner_width.saturating_sub(separators);

    let fixed_widths = items
        .iter()
        .map(|item| {
            let metric = item.metric();
            let value = values
                .get(&metric)
                .copied()
                .unwrap_or(MetricValue::Single(0.0));
            segment_fixed_width(
                metric,
                item.view(),
                value,
                headline_values.get(&metric).copied(),
                label_width,
            )
        })
        .collect::<Vec<_>>();
    let fixed_total = fixed_widths.iter().sum::<usize>();

    let graph_widths = if !items.is_empty() && segment_space >= fixed_total + items.len() {
        split_weighted_width(segment_space - fixed_total - items.len(), items)
    } else {
        vec![0; items.len()]
    };

    let widths = if graph_widths.iter().any(|width| *width > 0) {
        fixed_widths
            .iter()
            .zip(&graph_widths)
            .map(|(fixed, graph)| fixed + 1 + graph)
            .collect::<Vec<_>>()
    } else {
        split_weighted_width(segment_space, items)
    };

    let segments = items
        .iter()
        .zip(widths)
        .zip(graph_widths)
        .enumerate()
        .map(|(index, ((item, width), graph_width))| {
            let metric = item.metric();
            let history = histories.get(&metric).cloned().unwrap_or_default();
            let value = values
                .get(&metric)
                .copied()
                .unwrap_or(MetricValue::Single(0.0));
            let item_hues = metric_hues_for_visible_hue(
                metric,
                row_hues[index % row_hues.len()],
            );
            if graph_width > 0 {
                render_segment_with_graph_width(
                    *item,
                    &history,
                    value,
                    headline_values.get(&metric).copied(),
                    label_width,
                    graph_width,
                    config.align,
                    config.renderer,
                    Some(&item_hues),
                    color_enabled,
                )
            } else {
                render_segment_with_headline(
                    *item,
                    &history,
                    value,
                    headline_values.get(&metric).copied(),
                    width,
                    label_width,
                    config.align,
                    config.renderer,
                    Some(&item_hues),
                    color_enabled,
                )
            }
        })
        .collect::<Vec<_>>();

    pad_or_trim_visible(&format!("{prefix}{}", segments.join(" ")), width)
}

fn stream_metric_for(index: usize) -> MetricKind {
    const STREAM_METRICS: &[MetricKind] = &[
        MetricKind::Cpu,
        MetricKind::Memory,
        MetricKind::Gpu,
        MetricKind::Storage,
        MetricKind::Ingress,
        MetricKind::Egress,
    ];

    STREAM_METRICS[index % STREAM_METRICS.len()]
}

#[cfg(test)]
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
        None,
        color_enabled,
    )
}

fn split_weighted_width(total: usize, items: &[LayoutItem]) -> Vec<usize> {
    if items.is_empty() {
        return Vec::new();
    }

    let floors = items
        .iter()
        .map(|item| item.basis().unwrap_or(0).max(item.min_width().unwrap_or(0)))
        .collect::<Vec<_>>();
    let floor_total = floors.iter().sum::<usize>();

    if floor_total >= total {
        let weights = floors;
        let total_weight = weights.iter().sum::<usize>().max(1);
        let mut widths = weights
            .iter()
            .map(|weight| total.saturating_mul(*weight) / total_weight)
            .collect::<Vec<_>>();
        let allocated = widths.iter().sum::<usize>();
        let mut remaining = total.saturating_sub(allocated);
        let active = weights
            .iter()
            .enumerate()
            .filter_map(|(index, weight)| (*weight > 0).then_some(index))
            .collect::<Vec<_>>();
        let mut index = 0;
        while remaining > 0 && !active.is_empty() {
            widths[active[index % active.len()]] += 1;
            remaining -= 1;
            index += 1;
        }
        return widths;
    }

    let mut widths = floors;
    let mut remaining = total - floor_total;
    loop {
        let active = items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                let max_width = item.max_width().unwrap_or(usize::MAX);
                (item.grow() > 0 && widths[index] < max_width).then_some(index)
            })
            .collect::<Vec<_>>();
        if remaining == 0 || active.is_empty() {
            break;
        }

        let total_grow = active
            .iter()
            .map(|index| items[*index].grow())
            .sum::<usize>();
        if total_grow == 0 {
            break;
        }

        let snapshot = remaining;
        let mut allocated = 0;
        for index in &active {
            let item = &items[*index];
            let grow_share = snapshot.saturating_mul(item.grow()) / total_grow;
            let cap = item
                .max_width()
                .unwrap_or(usize::MAX)
                .saturating_sub(widths[*index]);
            let add = grow_share.min(cap);
            widths[*index] += add;
            allocated += add;
        }
        remaining = remaining.saturating_sub(allocated);

        let mut round_robin = 0;
        while remaining > 0 {
            let index = active[round_robin % active.len()];
            let max_width = items[index].max_width().unwrap_or(usize::MAX);
            if widths[index] < max_width {
                widths[index] += 1;
                remaining -= 1;
            }
            round_robin += 1;
            if round_robin >= active.len()
                && active
                    .iter()
                    .all(|index| widths[*index] >= items[*index].max_width().unwrap_or(usize::MAX))
            {
                break;
            }
        }

        if allocated == 0 && snapshot == remaining {
            break;
        }
    }

    widths
}

fn split_stream_widths(total: usize, count: usize) -> Vec<usize> {
    if count == 0 {
        return Vec::new();
    }

    let base = total / count;
    let remainder = total % count;
    (0..count)
        .map(|index| base + usize::from(index < remainder))
        .collect()
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
    hues: Option<&BaseHues>,
    color_enabled: bool,
) -> String {
    let metric = item.metric();
    let (label, label_usage_separator, usage_text, fixed) =
        segment_text_parts(metric, item.view(), value, headline_value, label_width);

    if width <= fixed {
        return pad_or_trim_visible(
            &format!("{label}{label_usage_separator}{usage_text}"),
            width,
        );
    }

    let graph_width = width.saturating_sub(fixed + 1);
    let samples = history.iter().copied().collect::<Vec<_>>();
    let graph = match renderer {
        Renderer::Braille => render_braille_graph(&samples, graph_width, metric, hues, color_enabled),
        Renderer::Block => render_block_graph(&samples, graph_width, metric, hues, color_enabled),
    };

    pad_or_trim_visible(
        &match align {
            Align::Left => format!("{label}{label_usage_separator}{usage_text} {graph}"),
            Align::Right => format!("{label} {graph} {usage_text}"),
        },
        width,
    )
}

fn render_segment_with_graph_width(
    item: LayoutItem,
    history: &VecDeque<MetricValue>,
    value: MetricValue,
    headline_value: Option<HeadlineValue>,
    label_width: usize,
    graph_width: usize,
    align: Align,
    renderer: Renderer,
    hues: Option<&BaseHues>,
    color_enabled: bool,
) -> String {
    let metric = item.metric();
    let (label, label_usage_separator, usage_text, fixed) =
        segment_text_parts(metric, item.view(), value, headline_value, label_width);
    let samples = history.iter().copied().collect::<Vec<_>>();
    let graph = match renderer {
        Renderer::Braille => render_braille_graph(&samples, graph_width, metric, hues, color_enabled),
        Renderer::Block => render_block_graph(&samples, graph_width, metric, hues, color_enabled),
    };
    let width = fixed + 1 + graph_width;

    pad_or_trim_visible(
        &match align {
            Align::Left => format!("{label}{label_usage_separator}{usage_text} {graph}"),
            Align::Right => format!("{label} {graph} {usage_text}"),
        },
        width,
    )
}

fn segment_text_parts(
    metric: MetricKind,
    view: LayoutView,
    value: MetricValue,
    headline_value: Option<HeadlineValue>,
    _label_width: usize,
) -> (String, &'static str, String, usize) {
    let label = metric.short_label().to_owned();
    let usage_text = metric.format_value(
        view,
        value.headline_value(),
        &headline_value.unwrap_or_else(|| HeadlineValue::Scalar(value.headline_value())),
    );
    let label_usage_separator = match metric {
        MetricKind::Vram => "",
        _ => " ",
    };
    let fixed =
        label.chars().count() + label_usage_separator.chars().count() + usage_text.chars().count();
    (label, label_usage_separator, usage_text, fixed)
}

fn segment_fixed_width(
    metric: MetricKind,
    view: LayoutView,
    value: MetricValue,
    headline_value: Option<HeadlineValue>,
    label_width: usize,
) -> usize {
    segment_text_parts(metric, view, value, headline_value, label_width).3
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
    hues: Option<&BaseHues>,
    color_enabled: bool,
) -> String {
    let samples = resample_channel(samples, width, MetricValue::headline_value);
    samples
        .into_iter()
        .map(|sample| {
            let idx = (sample.clamp(0.0, 1.0) * (BLOCKS.len() - 1) as f64).round() as usize;
            let ch = BLOCKS[idx.min(BLOCKS.len() - 1)].to_string();
            let color = interpolate(gradient_for_with_hues(metric, hues), sample);
            paint(&ch, color, color_enabled)
        })
        .collect()
}

pub(crate) fn render_braille_graph(
    samples: &[MetricValue],
    width: usize,
    metric: MetricKind,
    hues: Option<&BaseHues>,
    color_enabled: bool,
) -> String {
    if metric.is_split() {
        return render_split_braille_graph(samples, width, metric, hues, color_enabled);
    }

    let samples = resample_channel(
        samples,
        width.saturating_mul(2),
        MetricValue::headline_value,
    );
    let mut out = String::new();

    for chunk in samples.chunks(2) {
        let left = quantize_level(chunk.first().copied().unwrap_or(0.0));
        let right = quantize_level(chunk.get(1).copied().unwrap_or(0.0));
        let cell = braille_cell(left, right);
        let intensity = chunk.iter().copied().reduce(f64::max).unwrap_or(0.0);
        let color = interpolate(gradient_for_with_hues(metric, hues), intensity);
        out.push_str(&paint(&cell.to_string(), color, color_enabled));
    }

    out
}

fn render_split_braille_graph(
    samples: &[MetricValue],
    width: usize,
    metric: MetricKind,
    hues: Option<&BaseHues>,
    color_enabled: bool,
) -> String {
    let mut uppers = resample_channel(samples, width, MetricValue::upper);
    let mut lowers = resample_channel(samples, width, MetricValue::lower);
    normalize_split_channels(&mut uppers, &mut lowers);
    let (upper_gradient, lower_gradient) = split_gradients_for_with_hues(metric, hues).unwrap_or((
        gradient_for_with_hues(metric, hues),
        gradient_for_with_hues(metric, hues),
    ));
    let mut out = String::new();

    for index in 0..width {
        let upper = quantize_half_level(uppers.get(index).copied().unwrap_or(0.0));
        let lower = quantize_half_level(lowers.get(index).copied().unwrap_or(0.0));
        let upper_intensity = uppers.get(index).copied().unwrap_or(0.0);
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

fn normalize_split_channels(uppers: &mut [f64], lowers: &mut [f64]) {
    let scale = uppers
        .iter()
        .chain(lowers.iter())
        .copied()
        .fold(0.0_f64, f64::max);

    if scale <= f64::EPSILON {
        return;
    }

    for value in uppers.iter_mut().chain(lowers.iter_mut()) {
        *value = (*value / scale).clamp(0.0, 1.0);
    }
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
    const UPPER_BITS: [u8; 4] = [4, 1, 3, 0];
    const LOWER_BITS: [u8; 4] = [5, 2, 7, 6];
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
    if upper_level == 0 && lower_level == 0 {
        return "⠀".to_owned();
    }

    let ch = braille_half_cell(upper_level, lower_level).to_string();
    let upper_color = interpolate(upper_gradient, upper_intensity);
    let lower_color = interpolate(lower_gradient, lower_intensity);
    let color = blend_split_color(upper_level, lower_level, upper_color, lower_color);
    paint(&ch, color, color_enabled)
}

fn blend_split_color(
    upper_level: usize,
    lower_level: usize,
    upper_color: crate::color::Rgb,
    lower_color: crate::color::Rgb,
) -> crate::color::Rgb {
    if upper_level == 0 {
        return lower_color;
    }
    if lower_level == 0 {
        return upper_color;
    }

    let upper_weight = upper_level as u16;
    let lower_weight = lower_level as u16;
    let total = upper_weight + lower_weight;
    let mix = |upper: u8, lower: u8| -> u8 {
        (((u16::from(upper) * upper_weight) + (u16::from(lower) * lower_weight)) / total) as u8
    };

    crate::color::Rgb {
        r: mix(upper_color.r, lower_color.r),
        g: mix(upper_color.g, lower_color.g),
        b: mix(upper_color.b, lower_color.b),
    }
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
    use crate::color::split_gradients_for;
    use crate::config::{ColorMode, Config, OutputMode};
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
            None,
            false,
        );
        assert_eq!(graph.chars().count(), 2);
    }

    #[test]
    fn block_graph_renders_requested_width() {
        let graph = render_block_graph(
            &[
                MetricValue::Single(0.25),
                MetricValue::Single(0.5),
                MetricValue::Single(0.75),
                MetricValue::Single(1.0),
            ],
            4,
            MetricKind::Cpu,
            None,
            false,
        );
        assert_eq!(graph.chars().count(), 4);
    }

    #[test]
    fn line_layout_respects_multiple_segments() {
        let config = Config {
            history: 8,
            interval_ms: 1000,
            align: Align::Left,
            label: None,
            stream_labels: None,
            stream_layout: StreamLayout::Columns,
            space: Space::Graph,
            layout: Layout::default(),
            renderer: Renderer::Braille,
            color_mode: ColorMode::Never,
            output_mode: OutputMode::Terminal,
            width: Some(40),
            once: true,
            colors: None,
            force_stream_input: false,
            print_completion: None,
            debug_colors_steps: None,
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
    fn line_layout_supports_block_renderer() {
        let layout = crate::layout::parse_layout_spec("cpu").unwrap();
        let config = Config {
            history: 8,
            interval_ms: 1000,
            align: Align::Left,
            label: None,
            stream_labels: None,
            stream_layout: StreamLayout::Columns,
            space: Space::Graph,
            layout: layout.clone(),
            renderer: Renderer::Block,
            color_mode: ColorMode::Never,
            output_mode: OutputMode::Terminal,
            width: Some(18),
            once: true,
            colors: None,
            force_stream_input: false,
            print_completion: None,
            debug_colors_steps: None,
            show_help: false,
        };
        let histories = HashMap::from([(
            MetricKind::Cpu,
            VecDeque::from(vec![
                MetricValue::Single(0.0),
                MetricValue::Single(0.25),
                MetricValue::Single(0.5),
                MetricValue::Single(1.0),
            ]),
        )]);
        let values = HashMap::from([(MetricKind::Cpu, MetricValue::Single(1.0))]);

        let lines = render_lines(&config, 18, false, &histories, &layout, &values);

        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("cpu"));
        assert!(lines[0].chars().any(|ch| BLOCKS[1..].contains(&ch)));
        assert_eq!(visible_width(&lines[0]), 18);
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
            stream_labels: None,
            stream_layout: StreamLayout::Columns,
            space: Space::Graph,
            layout: crate::layout::parse_layout_spec("cpu gpu").unwrap(),
            renderer: Renderer::Braille,
            color_mode: ColorMode::Never,
            output_mode: OutputMode::Terminal,
            width: Some(40),
            once: true,
            colors: None,
            force_stream_input: false,
            print_completion: None,
            debug_colors_steps: None,
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

        let lines = render_lines(&config, 40, false, &histories, &config.layout, &values);

        let line = &lines[0];
        assert_eq!(visible_width(line), 40);
    }

    #[test]
    fn quintuple_layout_respects_requested_width_without_trimming_tail_metric() {
        let config = Config {
            history: 8,
            interval_ms: 1000,
            align: Align::Left,
            label: None,
            stream_labels: None,
            stream_layout: StreamLayout::Columns,
            space: Space::Graph,
            layout: crate::layout::parse_layout_spec("cpu ram spc io net").unwrap(),
            renderer: Renderer::Braille,
            color_mode: ColorMode::Never,
            output_mode: OutputMode::Terminal,
            width: Some(80),
            once: true,
            colors: None,
            force_stream_input: false,
            print_completion: None,
            debug_colors_steps: None,
            show_help: false,
        };
        let histories = HashMap::from([
            (
                MetricKind::Cpu,
                VecDeque::from(vec![
                    MetricValue::Single(0.1),
                    MetricValue::Single(0.2),
                    MetricValue::Single(0.3),
                ]),
            ),
            (
                MetricKind::Memory,
                VecDeque::from(vec![
                    MetricValue::Single(0.4),
                    MetricValue::Single(0.5),
                    MetricValue::Single(0.6),
                ]),
            ),
            (
                MetricKind::Storage,
                VecDeque::from(vec![
                    MetricValue::Single(0.7),
                    MetricValue::Single(0.8),
                    MetricValue::Single(0.9),
                ]),
            ),
            (
                MetricKind::Io,
                VecDeque::from(vec![
                    MetricValue::Split {
                        upper: 0.3,
                        lower: 0.2,
                    },
                    MetricValue::Split {
                        upper: 0.5,
                        lower: 0.4,
                    },
                ]),
            ),
            (
                MetricKind::Net,
                VecDeque::from(vec![
                    MetricValue::Split {
                        upper: 0.2,
                        lower: 0.1,
                    },
                    MetricValue::Split {
                        upper: 0.6,
                        lower: 0.5,
                    },
                ]),
            ),
        ]);
        let values = HashMap::from([
            (MetricKind::Cpu, MetricValue::Single(0.3)),
            (MetricKind::Memory, MetricValue::Single(0.6)),
            (MetricKind::Storage, MetricValue::Single(0.9)),
            (
                MetricKind::Io,
                MetricValue::Split {
                    upper: 0.5,
                    lower: 0.4,
                },
            ),
            (
                MetricKind::Net,
                MetricValue::Split {
                    upper: 0.6,
                    lower: 0.5,
                },
            ),
        ]);
        let headlines = HashMap::from([
            (MetricKind::Memory, HeadlineValue::Memory {
                used_bytes: 15 * 1024 * 1024 * 1024,
                available_bytes: 17 * 1024 * 1024 * 1024,
                total_bytes: 32 * 1024 * 1024 * 1024,
            }),
            (MetricKind::Storage, HeadlineValue::Storage {
                used_bytes: 12,
                total_bytes: 1024,
            }),
            (MetricKind::Io, HeadlineValue::Scalar(16.0 * 1024.0)),
            (MetricKind::Net, HeadlineValue::Scalar(14.0 * 1024.0)),
        ]);

        let line = &render_lines_with_headlines(
            &config,
            80,
            false,
            &histories,
            &config.layout,
            &values,
            &headlines,
        )[0];

        assert_eq!(visible_width(line), 80);
        assert!(line.contains("net 14K/s "), "unexpected row: {line}");
    }

    #[test]
    fn stream_columns_default_to_single_row_without_series_labels() {
        let config = Config {
            history: 8,
            interval_ms: 1000,
            align: Align::Left,
            label: None,
            stream_labels: None,
            stream_layout: StreamLayout::Columns,
            space: Space::Graph,
            layout: Layout::default(),
            renderer: Renderer::Braille,
            color_mode: ColorMode::Never,
            output_mode: OutputMode::Terminal,
            width: Some(24),
            once: true,
            colors: None,
            force_stream_input: false,
            print_completion: None,
            debug_colors_steps: None,
            show_help: false,
        };
        let histories = vec![VecDeque::from(vec![0.0, 0.25]), VecDeque::from(vec![0.5, 0.75])];
        let values = vec![0.25, 0.75];

        let lines = render_stream_lines(&config, 24, false, &histories, &values);

        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("25%"), "unexpected row: {}", lines[0]);
        assert!(lines[0].contains("75%"), "unexpected row: {}", lines[0]);
    }

    #[test]
    fn stream_columns_use_explicit_labels_when_given() {
        let config = Config {
            history: 8,
            interval_ms: 1000,
            align: Align::Left,
            label: None,
            stream_labels: Some(vec!["wifi".to_owned(), "vpn".to_owned()]),
            stream_layout: StreamLayout::Columns,
            space: Space::Graph,
            layout: Layout::default(),
            renderer: Renderer::Braille,
            color_mode: ColorMode::Never,
            output_mode: OutputMode::Terminal,
            width: Some(28),
            once: true,
            colors: None,
            force_stream_input: false,
            print_completion: None,
            debug_colors_steps: None,
            show_help: false,
        };
        let histories = vec![VecDeque::from(vec![0.0, 0.25]), VecDeque::from(vec![0.5, 0.75])];
        let values = vec![0.25, 0.75];

        let lines = render_stream_lines(&config, 28, false, &histories, &values);

        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("wifi"), "unexpected row: {}", lines[0]);
        assert!(lines[0].contains("vpn"), "unexpected row: {}", lines[0]);
        assert!(lines[0].contains("25%"), "unexpected row: {}", lines[0]);
        assert!(lines[0].contains("75%"), "unexpected row: {}", lines[0]);
    }

    #[test]
    fn stream_columns_keep_graph_widths_balanced_with_long_labels() {
        let config = Config {
            history: 8,
            interval_ms: 1000,
            align: Align::Left,
            label: None,
            stream_labels: Some(vec![
                "a".to_owned(),
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
            ]),
            stream_layout: StreamLayout::Columns,
            space: Space::Graph,
            layout: Layout::default(),
            renderer: Renderer::Braille,
            color_mode: ColorMode::Never,
            output_mode: OutputMode::Terminal,
            width: Some(120),
            once: true,
            colors: None,
            force_stream_input: false,
            print_completion: None,
            debug_colors_steps: None,
            show_help: false,
        };
        let histories = vec![
            VecDeque::from(vec![0.0, 0.25, 0.5, 0.75, 1.0]),
            VecDeque::from(vec![0.0, 0.25, 0.5, 0.75, 1.0]),
        ];
        let values = vec![1.0, 1.0];

        let line = &render_stream_lines(&config, 120, false, &histories, &values)[0];

        assert!(line.contains("a 100%"), "unexpected row: {line}");
        assert!(
            line.contains("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa 100%"),
            "unexpected row: {line}"
        );
        assert!(
            line.matches('⢸').count() >= 2 || line.matches('⣠').count() >= 2,
            "expected both segments to retain visible graph width: {line}"
        );
    }

    #[test]
    fn stream_columns_can_use_legacy_segment_spacing() {
        let segments = vec![
            (0, "a".to_owned(), " ", "100%".to_owned(), 6),
            (
                1,
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
                " ",
                "100%".to_owned(),
                43,
            ),
        ];

        let (_, graph_widths_graph) = stream_column_widths(Space::Graph, 120, &segments);
        let (_, graph_widths_segment) = stream_column_widths(Space::Segment, 120, &segments);

        assert_eq!(graph_widths_graph[0], graph_widths_graph[1]);
        assert!(graph_widths_segment[0] > graph_widths_segment[1]);
    }

    #[test]
    fn stream_lines_layout_renders_one_row_per_series() {
        let config = Config {
            history: 8,
            interval_ms: 1000,
            align: Align::Left,
            label: None,
            stream_labels: Some(vec!["wifi".to_owned(), "vpn".to_owned()]),
            stream_layout: StreamLayout::Lines,
            space: Space::Graph,
            layout: Layout::default(),
            renderer: Renderer::Braille,
            color_mode: ColorMode::Never,
            output_mode: OutputMode::Terminal,
            width: Some(28),
            once: true,
            colors: None,
            force_stream_input: false,
            print_completion: None,
            debug_colors_steps: None,
            show_help: false,
        };
        let histories = vec![VecDeque::from(vec![0.0, 0.25]), VecDeque::from(vec![0.5, 0.75])];
        let values = vec![0.25, 0.75];

        let lines = render_stream_lines(&config, 28, false, &histories, &values);

        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("wifi  25%"), "unexpected first row: {}", lines[0]);
        assert!(lines[1].starts_with(" vpn  75%"), "unexpected second row: {}", lines[1]);
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
    fn grow_uses_slash_syntax_in_parsed_layouts() {
        let layout = crate::layout::parse_layout_spec("cpu:12/2 ram:10 net.hum/3").unwrap();
        let widths = split_weighted_width(30, &layout.rows()[0]);
        assert_eq!(widths, vec![16, 10, 4]);
    }

    #[test]
    fn width_allocation_conformance_matrix() {
        let cases = [
            ("cpu:12/2 ram:10 net.hum/3", 30, vec![16, 10, 4]),
            ("cpu:12/2+14 ram:10 net.hum/3", 30, vec![14, 10, 6]),
            ("cpu-8 ram-4", 6, vec![4, 2]),
            ("cpu:12/2+20-8 ram+14-6", 24, vec![16, 8]),
        ];

        for (spec, total, expected) in cases {
            let layout = crate::layout::parse_layout_spec(spec).unwrap();
            assert_eq!(
                split_weighted_width(total, &layout.rows()[0]),
                expected,
                "{spec}"
            );
        }
    }

    #[test]
    fn fixed_width_rows_compress_proportionally_when_too_narrow() {
        let widths = split_weighted_width(
            12,
            &[
                LayoutItem::new(MetricKind::Cpu, LayoutView::Default, Some(12), 0),
                LayoutItem::new(MetricKind::Memory, LayoutView::Default, Some(10), 0),
                LayoutItem::new(MetricKind::Net, LayoutView::Default, Some(8), 0),
            ],
        );

        assert_eq!(widths, vec![5, 4, 3]);
    }

    #[test]
    fn basis_overflow_compresses_fixed_items_before_grow_only_items() {
        let widths = split_weighted_width(
            18,
            &[
                LayoutItem::new(MetricKind::Cpu, LayoutView::Default, Some(12), 2),
                LayoutItem::new(MetricKind::Memory, LayoutView::Default, Some(10), 0),
                LayoutItem::new(MetricKind::Net, LayoutView::Default, None, 5),
            ],
        );

        assert_eq!(widths, vec![10, 8, 0]);
    }

    #[test]
    fn max_width_clamps_growth_and_redistributes_leftover_space() {
        let widths = split_weighted_width(
            30,
            &[
                LayoutItem::with_constraints(
                    MetricKind::Cpu,
                    LayoutView::Default,
                    Some(12),
                    2,
                    Some(14),
                    None,
                ),
                LayoutItem::with_constraints(
                    MetricKind::Memory,
                    LayoutView::Default,
                    Some(10),
                    1,
                    None,
                    None,
                ),
            ],
        );

        assert_eq!(widths, vec![14, 16]);
    }

    #[test]
    fn min_width_reserves_space_before_growth_when_room_exists() {
        let widths = split_weighted_width(
            24,
            &[
                LayoutItem::with_constraints(
                    MetricKind::Cpu,
                    LayoutView::Default,
                    None,
                    1,
                    None,
                    Some(8),
                ),
                LayoutItem::with_constraints(
                    MetricKind::Memory,
                    LayoutView::Default,
                    None,
                    1,
                    None,
                    None,
                ),
            ],
        );

        assert_eq!(widths, vec![16, 8]);
    }

    #[test]
    fn min_widths_still_compress_when_a_row_is_too_narrow() {
        let widths = split_weighted_width(
            6,
            &[
                LayoutItem::with_constraints(
                    MetricKind::Cpu,
                    LayoutView::Default,
                    None,
                    1,
                    None,
                    Some(8),
                ),
                LayoutItem::with_constraints(
                    MetricKind::Memory,
                    LayoutView::Default,
                    None,
                    1,
                    None,
                    Some(4),
                ),
            ],
        );

        assert_eq!(widths, vec![4, 2]);
    }

    #[test]
    fn very_narrow_fixed_width_rows_still_render_to_the_requested_width() {
        let layout = crate::layout::parse_layout_spec("cpu:12 ram:12 net:16").unwrap();
        let config = Config {
            history: 8,
            interval_ms: 1000,
            align: Align::Left,
            label: None,
            stream_labels: None,
            stream_layout: StreamLayout::Columns,
            space: Space::Graph,
            layout: layout.clone(),
            renderer: Renderer::Braille,
            color_mode: ColorMode::Never,
            output_mode: OutputMode::Terminal,
            width: Some(16),
            once: true,
            colors: None,
            force_stream_input: false,
            print_completion: None,
            debug_colors_steps: None,
            show_help: false,
        };
        let histories = HashMap::from([
            (
                MetricKind::Cpu,
                VecDeque::from(vec![MetricValue::Single(0.1), MetricValue::Single(0.2)]),
            ),
            (
                MetricKind::Memory,
                VecDeque::from(vec![MetricValue::Single(0.3), MetricValue::Single(0.4)]),
            ),
            (
                MetricKind::Net,
                VecDeque::from(vec![
                    MetricValue::Split {
                        upper: 0.5,
                        lower: 0.25,
                    },
                    MetricValue::Split {
                        upper: 0.6,
                        lower: 0.2,
                    },
                ]),
            ),
        ]);
        let values = HashMap::from([
            (MetricKind::Cpu, MetricValue::Single(0.2)),
            (MetricKind::Memory, MetricValue::Single(0.4)),
            (
                MetricKind::Net,
                MetricValue::Split {
                    upper: 0.6,
                    lower: 0.2,
                },
            ),
        ]);
        let headlines = HashMap::from([
            (MetricKind::Cpu, HeadlineValue::Scalar(0.2)),
            (MetricKind::Memory, HeadlineValue::Scalar(0.4)),
            (MetricKind::Net, HeadlineValue::Scalar(812.0 * 1024.0)),
        ]);

        let lines = render_lines_with_headlines(
            &config, 16, false, &histories, &layout, &values, &headlines,
        );

        assert_eq!(lines.len(), 1);
        assert_eq!(visible_width(&lines[0]), 16);
    }

    #[test]
    fn multiline_layout_renders_multiple_rows() {
        let layout = crate::layout::parse_layout_spec("all").unwrap();
        let config = Config {
            history: 8,
            interval_ms: 1000,
            align: Align::Left,
            label: None,
            stream_labels: None,
            stream_layout: StreamLayout::Columns,
            space: Space::Graph,
            layout: layout.clone(),
            renderer: Renderer::Braille,
            color_mode: ColorMode::Never,
            output_mode: OutputMode::Terminal,
            width: Some(80),
            once: true,
            colors: None,
            force_stream_input: false,
            print_completion: None,
            debug_colors_steps: None,
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
        let lines = render_lines_with_headlines(
            &config, 80, false, &histories, &layout, &values, &headlines,
        );

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
        assert!(segment.contains("38%"));
    }

    #[test]
    fn combined_net_uses_summed_rate_headline() {
        let layout = crate::layout::parse_layout_spec("net").unwrap();
        let config = Config {
            history: 8,
            interval_ms: 1000,
            align: Align::Left,
            label: None,
            stream_labels: None,
            stream_layout: StreamLayout::Columns,
            space: Space::Graph,
            layout: layout.clone(),
            renderer: Renderer::Braille,
            color_mode: ColorMode::Never,
            output_mode: OutputMode::Terminal,
            width: Some(24),
            once: true,
            colors: None,
            force_stream_input: false,
            print_completion: None,
            debug_colors_steps: None,
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
        let headlines = HashMap::from([(
            MetricKind::Net,
            HeadlineValue::Scalar(3.5 * 1024.0 * 1024.0),
        )]);

        let lines = render_lines_with_headlines(
            &config, 24, false, &histories, &layout, &values, &headlines,
        );

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
            None,
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
            None,
            false,
        );

        assert!(segment.contains("vram0%"));
    }

    #[test]
    fn rate_metrics_use_fixed_width_value_fields() {
        let io_small = render_segment_with_headline(
            item(MetricKind::Io),
            &VecDeque::new(),
            MetricValue::Split {
                upper: 0.0,
                lower: 0.0,
            },
            Some(HeadlineValue::Scalar(105.0 * 1024.0)),
            18,
            4,
            Align::Left,
            Renderer::Braille,
            None,
            false,
        );
        let io_large = render_segment_with_headline(
            item(MetricKind::Io),
            &VecDeque::new(),
            MetricValue::Split {
                upper: 0.0,
                lower: 0.0,
            },
            Some(HeadlineValue::Scalar(14.0 * 1024.0 * 1024.0)),
            18,
            4,
            Align::Left,
            Renderer::Braille,
            None,
            false,
        );

        assert!(io_small.starts_with("io 105K/s "));
        assert!(io_large.starts_with("io 14M/s "));
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
            None,
            false,
        );

        assert!(segment.contains("spc 512M/2.0G"));
    }

    #[test]
    fn explicit_view_override_segments_render_as_requested() {
        let cases = [
            (
                LayoutItem::new(MetricKind::Net, LayoutView::Pct, None, 1),
                MetricValue::Split {
                    upper: 0.5,
                    lower: 0.25,
                },
                Some(HeadlineValue::Scalar(3.5 * 1024.0 * 1024.0)),
                " 50%",
            ),
            (
                LayoutItem::new(MetricKind::Storage, LayoutView::Pct, None, 1),
                MetricValue::Single(0.5),
                Some(HeadlineValue::Storage {
                    used_bytes: 512 * 1024 * 1024,
                    total_bytes: 2 * 1024 * 1024 * 1024,
                }),
                " 50%",
            ),
            (
                LayoutItem::new(MetricKind::Storage, LayoutView::Hum, None, 1),
                MetricValue::Single(0.5),
                Some(HeadlineValue::Storage {
                    used_bytes: 512 * 1024 * 1024,
                    total_bytes: 2 * 1024 * 1024 * 1024,
                }),
                "512M/2.0G",
            ),
        ];

        for (item, value, headline, expected) in cases {
            let segment = render_segment_with_headline(
                item,
                &VecDeque::new(),
                value,
                headline,
                18,
                4,
                Align::Left,
                Renderer::Braille,
                None,
                false,
            );

            assert!(
                segment.contains(expected),
                "expected {expected:?} in {segment:?} for {:?} {:?}",
                item.metric(),
                item.view()
            );
        }
    }

    #[test]
    fn braille_cell_coloring_prefers_spike_over_average() {
        let graph = render_braille_graph(
            &[MetricValue::Single(0.05), MetricValue::Single(1.0)],
            1,
            MetricKind::Cpu,
            None,
            true,
        );
        let expected = crate::color::interpolate(crate::color::gradient_for(MetricKind::Cpu), 1.0);
        assert!(graph.contains(&format!(
            "\x1b[38;2;{};{};{}m",
            expected.r, expected.g, expected.b
        )));
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

        assert!(segment.starts_with("cpu 100% "));
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
            None,
            false,
        );
        assert_eq!(graph.chars().count(), 1);
        assert_ne!(graph, " ");
    }

    #[test]
    fn split_metric_normalizes_both_halves_by_shared_visible_max() {
        let graph = render_braille_graph(
            &[MetricValue::Split {
                upper: 0.5,
                lower: 0.25,
            }],
            1,
            MetricKind::Net,
            None,
            false,
        );

        assert_eq!(graph, "⠿");
    }

    #[test]
    fn net_uses_a_single_blended_color_for_combined_halves() {
        let graph = render_braille_graph(
            &[MetricValue::Split {
                upper: 1.0,
                lower: 1.0,
            }],
            1,
            MetricKind::Net,
            None,
            true,
        );

        let (upper_gradient, lower_gradient) = split_gradients_for(MetricKind::Net).unwrap();
        let expected = blend_split_color(
            4,
            4,
            interpolate(upper_gradient, 1.0),
            interpolate(lower_gradient, 1.0),
        );
        assert_eq!(visible_width(&graph), 1);
        assert!(graph.contains(&format!(
            "\x1b[38;2;{};{};{}m",
            expected.r, expected.g, expected.b
        )));
    }

    #[test]
    fn io_uses_a_single_blended_color_for_combined_halves() {
        let graph = render_braille_graph(
            &[MetricValue::Split {
                upper: 1.0,
                lower: 1.0,
            }],
            1,
            MetricKind::Io,
            None,
            true,
        );

        let (upper_gradient, lower_gradient) = split_gradients_for(MetricKind::Io).unwrap();
        let expected = blend_split_color(
            4,
            4,
            interpolate(upper_gradient, 1.0),
            interpolate(lower_gradient, 1.0),
        );
        assert_eq!(visible_width(&graph), 1);
        assert!(graph.contains(&format!(
            "\x1b[38;2;{};{};{}m",
            expected.r, expected.g, expected.b
        )));
    }

    #[test]
    fn sys_uses_a_single_blended_color_for_combined_halves() {
        let graph = render_braille_graph(
            &[MetricValue::Split {
                upper: 1.0,
                lower: 1.0,
            }],
            1,
            MetricKind::Sys,
            None,
            true,
        );

        let (upper_gradient, lower_gradient) = split_gradients_for(MetricKind::Sys).unwrap();
        let expected = blend_split_color(
            4,
            4,
            interpolate(upper_gradient, 1.0),
            interpolate(lower_gradient, 1.0),
        );
        assert_eq!(visible_width(&graph), 1);
        assert!(graph.contains(&format!(
            "\x1b[38;2;{};{};{}m",
            expected.r, expected.g, expected.b
        )));
    }

    #[test]
    fn gfx_uses_a_single_blended_color_for_combined_halves() {
        let graph = render_braille_graph(
            &[MetricValue::Split {
                upper: 1.0,
                lower: 1.0,
            }],
            1,
            MetricKind::Gfx,
            None,
            true,
        );

        let (upper_gradient, lower_gradient) = split_gradients_for(MetricKind::Gfx).unwrap();
        let expected = blend_split_color(
            4,
            4,
            interpolate(upper_gradient, 1.0),
            interpolate(lower_gradient, 1.0),
        );
        assert_eq!(visible_width(&graph), 1);
        assert!(graph.contains(&format!(
            "\x1b[38;2;{};{};{}m",
            expected.r, expected.g, expected.b
        )));
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
    fn shorter_labels_use_compact_prefixes_next_to_vram() {
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

        assert!(gpu.starts_with("gpu  50%"));
        assert!(vram.starts_with("vram50%"));
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
