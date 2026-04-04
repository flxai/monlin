use std::collections::{HashMap, VecDeque};
use std::env;

use clap::ValueEnum;

use crate::color::{
    gradient_for_with_hues, interpolate, metric_hues_for_visible_hue, paint,
    split_gradients_for_with_hues, visible_hues, BaseHues, ColorSpec,
};
use crate::config::{Align, Config, LayoutEngine, Space, StreamGroup, StreamItem, StreamLayout};
use crate::layout::{
    DisplayMode, Document, Item, Layout, LayoutItem, LayoutView, MetricKind, Source,
};
use crate::metrics::{CanonicalSample, CanonicalValue, HeadlineValue, MetricValue};

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum Renderer {
    Braille,
    Block,
}

const BLOCKS: &[char] = &[' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
const BRAILLE_LEFT_BITS: [u8; 4] = [6, 2, 1, 0];
const BRAILLE_RIGHT_BITS: [u8; 4] = [7, 5, 4, 3];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct GridColumnSpec {
    label_width: usize,
    value_width: usize,
    graph_width: usize,
    total_width: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
struct PackedFraction {
    num: usize,
    den: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
struct PackedSpan {
    start: PackedFraction,
    end: PackedFraction,
}

const UNAVAILABLE_GRAPH_BRAILLE: char = '⠄';
const UNAVAILABLE_GRAPH_BLOCK: char = '░';
const UNAVAILABLE_GRAPH_COLOR: crate::color::Rgb = crate::color::Rgb {
    r: 110,
    g: 110,
    b: 110,
};

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
    let effective_layout_engine = match config.layout_engine {
        LayoutEngine::Auto if layout.rows().len() > 1 => LayoutEngine::Grid,
        LayoutEngine::Auto => LayoutEngine::Flow,
        other => other,
    };

    if layout.rows().len() > 1 {
        match effective_layout_engine {
            LayoutEngine::Grid => {
                return render_grid_lines_with_headlines(
                    config,
                    width,
                    color_enabled,
                    histories,
                    layout,
                    values,
                    headline_values,
                );
            }
            LayoutEngine::Flex => {
                return render_flex_lines_with_headlines(
                    config,
                    width,
                    color_enabled,
                    histories,
                    layout,
                    values,
                    headline_values,
                );
            }
            LayoutEngine::Pack => {
                return render_pack_lines_with_headlines(
                    config,
                    width,
                    color_enabled,
                    histories,
                    layout,
                    values,
                    headline_values,
                );
            }
            LayoutEngine::Auto | LayoutEngine::Flow => {}
        }
    }

    let label_width = layout
        .rows()
        .iter()
        .flat_map(|items| items.iter())
        .map(|item| display_label_width(item.metric(), item.display()))
        .max()
        .unwrap_or(0);
    let stable_column_widths = matches!(config.space, Space::Stable).then(|| {
        stable_layout_column_widths(config, width, layout, values, headline_values, label_width)
    });
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
                stable_column_widths.as_deref(),
            )
        })
        .collect()
}

pub fn render_document_lines(
    config: &Config,
    width: usize,
    color_enabled: bool,
    histories: &HashMap<Source, VecDeque<CanonicalValue>>,
    document: &Document,
    sample: &CanonicalSample,
) -> Vec<String> {
    let total_items = document
        .rows()
        .iter()
        .map(|row| row.items().len())
        .sum::<usize>();
    let all_hues = visible_hues(total_items, config.colors.as_deref());
    let outer_prefix = config
        .label
        .as_ref()
        .map(|label| format!("{label} "))
        .unwrap_or_default();
    let mut offset = 0;
    let mut lines = Vec::new();

    for row in document.rows() {
        let row_hues = &all_hues[offset..offset + row.items().len()];
        offset += row.items().len();
        let prefix = match row.label() {
            Some(label) => format!("{outer_prefix}{label} "),
            None => outer_prefix.clone(),
        };
        lines.push(render_document_row(
            config,
            width,
            color_enabled,
            histories,
            sample,
            row.items(),
            &prefix,
            row_hues,
        ));
    }

    if lines.is_empty() {
        vec![String::new()]
    } else {
        lines
    }
}

fn render_pack_lines_with_headlines(
    config: &Config,
    width: usize,
    color_enabled: bool,
    histories: &HashMap<MetricKind, VecDeque<MetricValue>>,
    layout: &Layout,
    values: &HashMap<MetricKind, MetricValue>,
    headline_values: &HashMap<MetricKind, HeadlineValue>,
) -> Vec<String> {
    let prefix = config
        .label
        .as_ref()
        .map(|label| format!("{label} "))
        .unwrap_or_default();
    let packed_text_widths = packed_span_text_widths(
        layout,
        values,
        headline_values,
        matches!(config.space, Space::Stable),
    );
    let total_items = layout.rows().iter().map(|items| items.len()).sum::<usize>();
    let all_hues = visible_hues(total_items, config.colors.as_deref());
    let mut offset = 0;

    layout
        .rows()
        .iter()
        .map(|items| {
            let row_hues = &all_hues[offset..offset + items.len()];
            offset += items.len();
            let row_specs = pack_row_specs(
                config,
                width,
                &prefix,
                items,
                values,
                headline_values,
                &packed_text_widths,
            );
            let segments = items
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    let metric = item.metric();
                    let history = histories.get(&metric).cloned().unwrap_or_default();
                    let item_hues =
                        metric_hues_for_visible_hue(metric, row_hues[index % row_hues.len()]);
                    if let Some(value) = values.get(&metric).copied() {
                        render_grid_segment(
                            *item,
                            &history,
                            value,
                            headline_values.get(&metric).copied(),
                            row_specs[index],
                            config.renderer,
                            Some(&item_hues),
                            color_enabled,
                            matches!(config.space, Space::Stable),
                        )
                    } else {
                        render_unavailable_grid_segment(
                            *item,
                            row_specs[index],
                            config.renderer,
                            color_enabled,
                            matches!(config.space, Space::Stable),
                        )
                    }
                })
                .collect::<Vec<_>>();
            pad_or_trim_visible(&format!("{prefix}{}", segments.join(" ")), width)
        })
        .collect()
}

fn render_flex_lines_with_headlines(
    config: &Config,
    width: usize,
    color_enabled: bool,
    histories: &HashMap<MetricKind, VecDeque<MetricValue>>,
    layout: &Layout,
    values: &HashMap<MetricKind, MetricValue>,
    headline_values: &HashMap<MetricKind, HeadlineValue>,
) -> Vec<String> {
    let prefix = config
        .label
        .as_ref()
        .map(|label| format!("{label} "))
        .unwrap_or_default();
    let column_text_widths = shared_column_text_widths(
        layout,
        values,
        headline_values,
        matches!(config.space, Space::Stable),
    );
    let total_items = layout.rows().iter().map(|items| items.len()).sum::<usize>();
    let all_hues = visible_hues(total_items, config.colors.as_deref());
    let mut offset = 0;

    layout
        .rows()
        .iter()
        .map(|items| {
            let row_hues = &all_hues[offset..offset + items.len()];
            offset += items.len();
            let row_specs = flex_row_specs(
                config,
                width,
                &prefix,
                items,
                values,
                headline_values,
                &column_text_widths,
            );
            let segments = items
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    let metric = item.metric();
                    let history = histories.get(&metric).cloned().unwrap_or_default();
                    let item_hues =
                        metric_hues_for_visible_hue(metric, row_hues[index % row_hues.len()]);
                    if let Some(value) = values.get(&metric).copied() {
                        render_grid_segment(
                            *item,
                            &history,
                            value,
                            headline_values.get(&metric).copied(),
                            row_specs[index],
                            config.renderer,
                            Some(&item_hues),
                            color_enabled,
                            matches!(config.space, Space::Stable),
                        )
                    } else {
                        render_unavailable_grid_segment(
                            *item,
                            row_specs[index],
                            config.renderer,
                            color_enabled,
                            matches!(config.space, Space::Stable),
                        )
                    }
                })
                .collect::<Vec<_>>();
            pad_or_trim_visible(&format!("{prefix}{}", segments.join(" ")), width)
        })
        .collect()
}

fn render_grid_lines_with_headlines(
    config: &Config,
    width: usize,
    color_enabled: bool,
    histories: &HashMap<MetricKind, VecDeque<MetricValue>>,
    layout: &Layout,
    values: &HashMap<MetricKind, MetricValue>,
    headline_values: &HashMap<MetricKind, HeadlineValue>,
) -> Vec<String> {
    let prefix = config
        .label
        .as_ref()
        .map(|label| format!("{label} "))
        .unwrap_or_default();
    let column_specs = grid_column_specs(config, width, layout, values, headline_values);
    let total_items = layout.rows().iter().map(|items| items.len()).sum::<usize>();
    let all_hues = visible_hues(total_items, config.colors.as_deref());
    let mut offset = 0;

    layout
        .rows()
        .iter()
        .map(|items| {
            let row_hues = &all_hues[offset..offset + items.len()];
            offset += items.len();
            let segments = items
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    let metric = item.metric();
                    let history = histories.get(&metric).cloned().unwrap_or_default();
                    let item_hues =
                        metric_hues_for_visible_hue(metric, row_hues[index % row_hues.len()]);
                    if let Some(value) = values.get(&metric).copied() {
                        render_grid_segment(
                            *item,
                            &history,
                            value,
                            headline_values.get(&metric).copied(),
                            column_specs[index],
                            config.renderer,
                            Some(&item_hues),
                            color_enabled,
                            matches!(config.space, Space::Stable),
                        )
                    } else {
                        render_unavailable_grid_segment(
                            *item,
                            column_specs[index],
                            config.renderer,
                            color_enabled,
                            matches!(config.space, Space::Stable),
                        )
                    }
                })
                .collect::<Vec<_>>();
            pad_or_trim_visible(&format!("{prefix}{}", segments.join(" ")), width)
        })
        .collect()
}

fn grid_column_specs(
    config: &Config,
    width: usize,
    layout: &Layout,
    values: &HashMap<MetricKind, MetricValue>,
    headline_values: &HashMap<MetricKind, HeadlineValue>,
) -> Vec<GridColumnSpec> {
    let prefix = config
        .label
        .as_ref()
        .map(|label| format!("{label} "))
        .unwrap_or_default();
    let inner_width = width.saturating_sub(prefix.chars().count());
    let column_count = layout
        .rows()
        .iter()
        .map(|items| items.len())
        .max()
        .unwrap_or(0);
    if column_count == 0 {
        return Vec::new();
    }

    let stable_layout = matches!(config.space, Space::Stable);
    let mut label_widths = vec![0; column_count];
    let mut value_widths = vec![0; column_count];
    let mut column_items = vec![
        LayoutItem::with_constraints(
            MetricKind::Cpu,
            LayoutView::Default,
            DisplayMode::Full,
            None,
            1,
            None,
            None
        );
        column_count
    ];
    let mut column_item_seen = vec![false; column_count];

    for items in layout.rows() {
        for (index, item) in items.iter().enumerate() {
            let metric = item.metric();
            label_widths[index] =
                label_widths[index].max(display_label_width(metric, item.display()));
            value_widths[index] =
                value_widths[index].max(if let Some(value) = values.get(&metric).copied() {
                    display_value_width(
                        metric,
                        item.view(),
                        item.display(),
                        value,
                        headline_values.get(&metric).copied(),
                        stable_layout,
                    )
                } else {
                    unavailable_display_value_width(
                        metric,
                        item.view(),
                        item.display(),
                        stable_layout,
                    )
                });
            let merged_basis = match (
                column_item_seen[index],
                column_items[index].basis(),
                item.basis(),
            ) {
                (false, _, basis) => basis,
                (true, Some(left), Some(right)) => Some(left.max(right)),
                (true, Some(left), None) => Some(left),
                (true, None, Some(right)) => Some(right),
                (true, None, None) => None,
            };
            let item_grow_hint = if item.grow() == 0 {
                item.basis().unwrap_or(0)
            } else {
                item.grow()
            };
            let merged_grow = if column_item_seen[index] {
                column_items[index].grow().max(item_grow_hint)
            } else {
                item_grow_hint
            };
            let merged_max = match (
                column_item_seen[index],
                column_items[index].max_width(),
                item.max_width(),
            ) {
                (false, _, max) => max,
                (true, Some(left), Some(right)) => Some(left.max(right)),
                (true, Some(left), None) => Some(left),
                (true, None, Some(right)) => Some(right),
                (true, None, None) => None,
            };
            let merged_min = match (
                column_item_seen[index],
                column_items[index].min_width(),
                item.min_width(),
            ) {
                (false, _, min) => min,
                (true, Some(left), Some(right)) => Some(left.max(right)),
                (true, Some(left), None) => Some(left),
                (true, None, Some(right)) => Some(right),
                (true, None, None) => None,
            };
            column_items[index] = LayoutItem::with_constraints(
                metric,
                item.view(),
                item.display(),
                merged_basis,
                merged_grow,
                merged_max,
                merged_min,
            );
            column_item_seen[index] = true;
        }
    }

    let fixed_widths = label_widths
        .iter()
        .zip(&value_widths)
        .map(|(label, value)| label + 1 + value)
        .collect::<Vec<_>>();
    let separators = column_count.saturating_sub(1);
    let segment_space = inner_width.saturating_sub(separators);
    let fixed_total = fixed_widths.iter().sum::<usize>();
    let graph_widths = if segment_space >= fixed_total + column_count {
        split_weighted_width(
            segment_space - fixed_total - column_count,
            &normalized_items_for_sizing(&column_items),
        )
    } else {
        vec![0; column_count]
    };

    fixed_widths
        .into_iter()
        .zip(label_widths)
        .zip(value_widths)
        .zip(graph_widths)
        .map(
            |(((fixed, label_width), value_width), graph_width)| GridColumnSpec {
                label_width,
                value_width,
                graph_width,
                total_width: if graph_width > 0 {
                    fixed + 1 + graph_width
                } else {
                    fixed
                },
            },
        )
        .collect()
}

fn shared_column_text_widths(
    layout: &Layout,
    values: &HashMap<MetricKind, MetricValue>,
    headline_values: &HashMap<MetricKind, HeadlineValue>,
    stable_layout: bool,
) -> Vec<(usize, usize)> {
    let column_count = layout
        .rows()
        .iter()
        .map(|items| items.len())
        .max()
        .unwrap_or(0);
    let mut text_widths = vec![(0, 0); column_count];

    for items in layout.rows() {
        for (index, item) in items.iter().enumerate() {
            let metric = item.metric();
            text_widths[index].0 = text_widths[index]
                .0
                .max(display_label_width(metric, item.display()));
            text_widths[index].1 =
                text_widths[index]
                    .1
                    .max(if let Some(value) = values.get(&metric).copied() {
                        display_value_width(
                            metric,
                            item.view(),
                            item.display(),
                            value,
                            headline_values.get(&metric).copied(),
                            stable_layout,
                        )
                    } else {
                        unavailable_display_value_width(
                            metric,
                            item.view(),
                            item.display(),
                            stable_layout,
                        )
                    });
        }
    }

    text_widths
}

fn packed_span_text_widths(
    layout: &Layout,
    values: &HashMap<MetricKind, MetricValue>,
    headline_values: &HashMap<MetricKind, HeadlineValue>,
    stable_layout: bool,
) -> HashMap<PackedSpan, (usize, usize)> {
    let mut text_widths = HashMap::new();

    for items in layout.rows() {
        for (item, span) in items.iter().zip(packed_row_spans(items)) {
            let metric = item.metric();
            let label_width = display_label_width(metric, item.display());
            let value_width = if let Some(value) = values.get(&metric).copied() {
                display_value_width(
                    metric,
                    item.view(),
                    item.display(),
                    value,
                    headline_values.get(&metric).copied(),
                    stable_layout,
                )
            } else {
                unavailable_display_value_width(metric, item.view(), item.display(), stable_layout)
            };
            let entry = text_widths.entry(span).or_insert((0, 0));
            entry.0 = entry.0.max(label_width);
            entry.1 = entry.1.max(value_width);
        }
    }

    text_widths
}

fn flex_row_specs(
    config: &Config,
    width: usize,
    prefix: &str,
    items: &[LayoutItem],
    values: &HashMap<MetricKind, MetricValue>,
    headline_values: &HashMap<MetricKind, HeadlineValue>,
    column_text_widths: &[(usize, usize)],
) -> Vec<GridColumnSpec> {
    let count = items.len();
    if count == 0 {
        return Vec::new();
    }

    let stable_layout = matches!(config.space, Space::Stable);
    let inner_width = width.saturating_sub(prefix.chars().count());
    let label_widths = items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            column_text_widths
                .get(index)
                .map(|(label_width, _)| *label_width)
                .unwrap_or_else(|| display_label_width(item.metric(), item.display()))
        })
        .collect::<Vec<_>>();
    let value_widths = items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            column_text_widths
                .get(index)
                .map(|(_, value_width)| *value_width)
                .unwrap_or_else(|| {
                    let metric = item.metric();
                    if let Some(value) = values.get(&metric).copied() {
                        display_value_width(
                            metric,
                            item.view(),
                            item.display(),
                            value,
                            headline_values.get(&metric).copied(),
                            stable_layout,
                        )
                    } else {
                        unavailable_display_value_width(
                            metric,
                            item.view(),
                            item.display(),
                            stable_layout,
                        )
                    }
                })
        })
        .collect::<Vec<_>>();
    let fixed_widths = label_widths
        .iter()
        .zip(&value_widths)
        .map(|(label, value)| label + 1 + value)
        .collect::<Vec<_>>();
    let separators = count.saturating_sub(1);
    let segment_space = inner_width.saturating_sub(separators);
    let total_widths = split_weighted_width(segment_space, &normalized_items_for_sizing(items));

    fixed_widths
        .into_iter()
        .zip(label_widths)
        .zip(value_widths)
        .zip(total_widths)
        .map(|(((fixed, label_width), value_width), total_width)| {
            let graph_width = if total_width > fixed {
                total_width.saturating_sub(fixed + 1)
            } else {
                0
            };
            GridColumnSpec {
                label_width,
                value_width,
                graph_width,
                total_width,
            }
        })
        .collect()
}

fn pack_row_specs(
    config: &Config,
    width: usize,
    prefix: &str,
    items: &[LayoutItem],
    values: &HashMap<MetricKind, MetricValue>,
    headline_values: &HashMap<MetricKind, HeadlineValue>,
    packed_text_widths: &HashMap<PackedSpan, (usize, usize)>,
) -> Vec<GridColumnSpec> {
    let count = items.len();
    if count == 0 {
        return Vec::new();
    }

    let stable_layout = matches!(config.space, Space::Stable);
    let inner_width = width.saturating_sub(prefix.chars().count());
    let spans = packed_row_spans(items);
    let label_widths = items
        .iter()
        .zip(spans.iter())
        .map(|(item, span)| {
            packed_text_widths
                .get(span)
                .map(|(label_width, _)| *label_width)
                .unwrap_or_else(|| display_label_width(item.metric(), item.display()))
        })
        .collect::<Vec<_>>();
    let value_widths = items
        .iter()
        .zip(spans.iter())
        .map(|(item, span)| {
            packed_text_widths
                .get(span)
                .map(|(_, value_width)| *value_width)
                .unwrap_or_else(|| {
                    let metric = item.metric();
                    if let Some(value) = values.get(&metric).copied() {
                        display_value_width(
                            metric,
                            item.view(),
                            item.display(),
                            value,
                            headline_values.get(&metric).copied(),
                            stable_layout,
                        )
                    } else {
                        unavailable_display_value_width(
                            metric,
                            item.view(),
                            item.display(),
                            stable_layout,
                        )
                    }
                })
        })
        .collect::<Vec<_>>();
    let fixed_widths = label_widths
        .iter()
        .zip(&value_widths)
        .map(|(label, value)| label + 1 + value)
        .collect::<Vec<_>>();
    let separators = count.saturating_sub(1);
    let segment_space = inner_width.saturating_sub(separators);
    let total_widths = split_weighted_width(segment_space, &normalized_items_for_sizing(items));

    fixed_widths
        .into_iter()
        .zip(label_widths)
        .zip(value_widths)
        .zip(total_widths)
        .map(|(((fixed, label_width), value_width), total_width)| {
            let graph_width = if total_width > fixed {
                total_width.saturating_sub(fixed + 1)
            } else {
                0
            };
            GridColumnSpec {
                label_width,
                value_width,
                graph_width,
                total_width,
            }
        })
        .collect()
}

fn packed_row_spans(items: &[LayoutItem]) -> Vec<PackedSpan> {
    let normalized = normalized_items_for_sizing(items);
    let total = normalized
        .iter()
        .map(|item| item.grow().max(1))
        .sum::<usize>()
        .max(1);
    let mut start = 0usize;

    normalized
        .iter()
        .map(|item| {
            let width = item.grow().max(1);
            let end = start + width;
            let span = PackedSpan {
                start: reduce_fraction(start, total),
                end: reduce_fraction(end, total),
            };
            start = end;
            span
        })
        .collect()
}

fn reduce_fraction(num: usize, den: usize) -> PackedFraction {
    let gcd = gcd(num, den).max(1);
    PackedFraction {
        num: num / gcd,
        den: den / gcd,
    }
}

fn gcd(mut a: usize, mut b: usize) -> usize {
    while b != 0 {
        let r = a % b;
        a = b;
        b = r;
    }
    a
}

fn stable_layout_column_widths(
    config: &Config,
    width: usize,
    layout: &Layout,
    values: &HashMap<MetricKind, MetricValue>,
    headline_values: &HashMap<MetricKind, HeadlineValue>,
    label_width: usize,
) -> Vec<usize> {
    let prefix = config
        .label
        .as_ref()
        .map(|label| format!("{label} "))
        .unwrap_or_default();
    let inner_width = width.saturating_sub(prefix.chars().count());
    let column_count = layout
        .rows()
        .iter()
        .map(|items| items.len())
        .max()
        .unwrap_or(0);
    if column_count == 0 {
        return Vec::new();
    }

    let mut fixed_widths = vec![0; column_count];
    for items in layout.rows() {
        for (index, item) in items.iter().enumerate() {
            let metric = item.metric();
            fixed_widths[index] =
                fixed_widths[index].max(if let Some(value) = values.get(&metric).copied() {
                    segment_fixed_width(
                        metric,
                        item.view(),
                        item.display(),
                        value,
                        headline_values.get(&metric).copied(),
                        label_width,
                        true,
                    )
                } else {
                    unavailable_segment_fixed_width(
                        metric,
                        item.view(),
                        item.display(),
                        label_width,
                        true,
                    )
                });
        }
    }

    let separators = column_count.saturating_sub(1);
    let segment_space = inner_width.saturating_sub(separators);
    let fixed_total = fixed_widths.iter().sum::<usize>();
    if segment_space >= fixed_total + column_count {
        let graph_widths =
            split_stream_widths(segment_space - fixed_total - column_count, column_count);
        fixed_widths
            .into_iter()
            .zip(graph_widths)
            .map(|(fixed, graph)| fixed + 1 + graph)
            .collect()
    } else {
        split_stream_widths(segment_space, column_count)
    }
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

    if let Some(groups) = &config.stream_groups {
        return render_stream_group_lines(config, width, color_enabled, histories, values, groups);
    }

    match config.stream_layout {
        StreamLayout::Columns => vec![render_stream_columns_line(
            config,
            width,
            color_enabled,
            histories,
            values,
        )],
        StreamLayout::Lines => render_stream_rows(config, width, color_enabled, histories, values),
    }
}

fn render_stream_group_lines(
    config: &Config,
    width: usize,
    color_enabled: bool,
    histories: &[VecDeque<f64>],
    values: &[f64],
    groups: &[StreamGroup],
) -> Vec<String> {
    let max_ref = groups
        .iter()
        .flat_map(|group| group.rows.iter())
        .flat_map(|row| row.iter())
        .map(|item| item.column_index + 1)
        .max()
        .unwrap_or(values.len())
        .max(values.len());
    let stream_hues = visible_hues(max_ref, config.colors.as_deref());
    let outer_prefix = config
        .label
        .as_ref()
        .map(|label| format!("{label} "))
        .unwrap_or_default();

    let mut lines = Vec::new();
    for group in groups {
        let group_prefix = match &group.label {
            Some(label) => format!("{outer_prefix}{label} "),
            None => outer_prefix.clone(),
        };
        for row in &group.rows {
            lines.push(render_stream_group_row(
                config,
                width,
                color_enabled,
                histories,
                values,
                row,
                &group_prefix,
                &stream_hues,
            ));
        }
    }
    if lines.is_empty() {
        vec![String::new()]
    } else {
        lines
    }
}

fn render_stream_group_row(
    config: &Config,
    width: usize,
    color_enabled: bool,
    histories: &[VecDeque<f64>],
    values: &[f64],
    items: &[StreamItem],
    prefix: &str,
    stream_hues: &[ColorSpec],
) -> String {
    let inner_width = width.saturating_sub(prefix.chars().count());
    let segments = items
        .iter()
        .map(|item| {
            let label = match item.display {
                DisplayMode::Full => item.label.clone().unwrap_or_default(),
                DisplayMode::Value | DisplayMode::Bare => String::new(),
            };
            let usage_text = match item.display {
                DisplayMode::Full | DisplayMode::Value => format!(
                    "{:>3.0}%",
                    values[item.column_index].clamp(0.0, 1.0) * 100.0
                ),
                DisplayMode::Bare => String::new(),
            };
            let separator = if label.is_empty() || usage_text.is_empty() {
                ""
            } else {
                " "
            };
            let fixed =
                label.chars().count() + separator.chars().count() + usage_text.chars().count();

            (item, label, separator, usage_text, fixed)
        })
        .collect::<Vec<_>>();

    let sizing_items = items
        .iter()
        .map(|item| {
            LayoutItem::with_constraints(
                MetricKind::Cpu,
                LayoutView::Default,
                DisplayMode::Full,
                Some(item.basis),
                item.basis,
                item.max_width,
                item.min_width,
            )
        })
        .collect::<Vec<_>>();
    let count = items.len();
    let separators = count.saturating_sub(1);
    let segment_space = inner_width.saturating_sub(separators);

    let (segment_widths, graph_widths) = match config.space {
        Space::Stable => {
            let fixed_total = segments
                .iter()
                .map(|(_, label, separator, usage_text, _)| {
                    label.chars().count()
                        + separator.chars().count()
                        + stable_stream_usage_width(usage_text)
                        + 1
                })
                .sum::<usize>();
            let graph_space = segment_space.saturating_sub(fixed_total);
            let graph_widths = split_weighted_width(graph_space, &sizing_items);
            let segment_widths = segments
                .iter()
                .zip(graph_widths.iter().copied())
                .map(|((_, label, separator, usage_text, _), graph_width)| {
                    label.chars().count()
                        + separator.chars().count()
                        + stable_stream_usage_width(usage_text)
                        + usize::from(graph_width > 0)
                        + graph_width
                })
                .collect::<Vec<_>>();
            (segment_widths, graph_widths)
        }
        Space::Graph => {
            let fixed_total = segments
                .iter()
                .map(|(_, _, _, _, fixed)| fixed + 1)
                .sum::<usize>();
            let graph_space = segment_space.saturating_sub(fixed_total);
            let graph_widths = split_weighted_width(graph_space, &sizing_items);
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
            let segment_widths = split_weighted_width(segment_space, &sizing_items);
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
    };

    let rendered = segments
        .into_iter()
        .zip(segment_widths.into_iter().zip(graph_widths))
        .map(
            |((item, label, separator, usage_text, _fixed), (segment_width, graph_width))| {
                let metric = stream_metric_for(item.column_index);
                let metric_history = histories
                    .get(item.column_index)
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
                    stream_hues[item.column_index % stream_hues.len()],
                );
                let display_usage_text = match config.space {
                    Space::Stable if !usage_text.is_empty() => {
                        format!(
                            "{usage_text:>width$}",
                            width = stable_stream_usage_width(&usage_text)
                        )
                    }
                    Space::Stable | Space::Graph | Space::Segment => usage_text.clone(),
                };
                let graph = match config.renderer {
                    Renderer::Braille => render_braille_graph(
                        &metric_history,
                        graph_width,
                        metric,
                        Some(&item_hues),
                        color_enabled,
                    ),
                    Renderer::Block => render_block_graph(
                        &metric_history,
                        graph_width,
                        metric,
                        Some(&item_hues),
                        color_enabled,
                    ),
                };
                let text_only = format!("{label}{separator}{display_usage_text}");

                if graph_width == 0 {
                    return pad_or_trim_visible(&text_only, segment_width);
                }

                pad_or_trim_visible(
                    &match config.align {
                        Align::Left => {
                            if text_only.is_empty() {
                                graph.clone()
                            } else {
                                format!("{label}{separator}{display_usage_text} {graph}")
                            }
                        }
                        Align::Right => {
                            if label.is_empty() {
                                if display_usage_text.is_empty() {
                                    graph.clone()
                                } else {
                                    format!("{graph} {display_usage_text}")
                                }
                            } else {
                                format!("{label} {graph} {display_usage_text}")
                            }
                        }
                    },
                    segment_width,
                )
            },
        )
        .collect::<Vec<_>>();

    pad_or_trim_visible(&format!("{prefix}{}", rendered.join(" ")), width)
}

fn render_document_row(
    config: &Config,
    width: usize,
    color_enabled: bool,
    histories: &HashMap<Source, VecDeque<CanonicalValue>>,
    sample: &CanonicalSample,
    items: &[Item],
    prefix: &str,
    row_hues: &[ColorSpec],
) -> String {
    let inner_width = width.saturating_sub(prefix.chars().count());
    let segments = items
        .iter()
        .map(|item| {
            let label = document_item_label(item);
            let usage_text = document_item_usage_text(
                item,
                sample.values.get(item.source()).copied(),
                sample.headlines.get(item.source()).copied(),
            );
            let separator = if label.is_empty() || usage_text.is_empty() {
                ""
            } else {
                " "
            };
            let fixed =
                visible_width(&label) + separator.chars().count() + visible_width(&usage_text);
            (item, label, separator, usage_text, fixed)
        })
        .collect::<Vec<_>>();

    let sizing_items = items
        .iter()
        .map(|item| {
            LayoutItem::with_constraints(
                MetricKind::Cpu,
                LayoutView::Default,
                DisplayMode::Full,
                Some(item.size()),
                item.size(),
                item.max_width(),
                item.min_width(),
            )
        })
        .collect::<Vec<_>>();
    let separators = items.len().saturating_sub(1);
    let segment_space = inner_width.saturating_sub(separators);

    let (segment_widths, graph_widths) = match config.space {
        Space::Stable | Space::Graph => {
            let fixed_total = segments
                .iter()
                .map(|(_, _, _, _, fixed)| fixed + 1)
                .sum::<usize>();
            let graph_space = segment_space.saturating_sub(fixed_total);
            let graph_widths = split_weighted_width(graph_space, &sizing_items);
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
            let segment_widths = split_weighted_width(segment_space, &sizing_items);
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
    };

    let rendered = segments
        .into_iter()
        .zip(segment_widths.into_iter().zip(graph_widths))
        .enumerate()
        .map(
            |(
                index,
                ((item, label, separator, usage_text, fixed), (segment_width, graph_width)),
            )| {
                let render_metric = document_render_metric(item.source());
                let metric_history = histories
                    .get(item.source())
                    .map(|history| {
                        history
                            .iter()
                            .copied()
                            .filter_map(CanonicalValue::normalized_metric_value)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let item_hues =
                    metric_hues_for_visible_hue(render_metric, row_hues[index % row_hues.len()]);
                let graph = match config.renderer {
                    Renderer::Braille => render_braille_graph(
                        &metric_history,
                        graph_width,
                        render_metric,
                        Some(&item_hues),
                        color_enabled,
                    ),
                    Renderer::Block => render_block_graph(
                        &metric_history,
                        graph_width,
                        render_metric,
                        Some(&item_hues),
                        color_enabled,
                    ),
                };
                let text_only = format!("{label}{separator}{usage_text}");
                let value = sample.values.get(item.source()).copied();

                let text = if matches!(value, Some(CanonicalValue::Unavailable) | None) {
                    let label = paint_unavailable_text(&label, color_enabled);
                    let usage_text = paint_unavailable_text(&usage_text, color_enabled);
                    let graph =
                        render_unavailable_graph(graph_width, config.renderer, color_enabled);
                    if graph_width == 0 {
                        format!("{label}{separator}{usage_text}")
                    } else {
                        match config.align {
                            Align::Left => {
                                if text_only.is_empty() {
                                    graph
                                } else {
                                    format!("{label}{separator}{usage_text} {graph}")
                                }
                            }
                            Align::Right => {
                                if label.is_empty() {
                                    if usage_text.is_empty() {
                                        graph
                                    } else {
                                        format!("{graph} {usage_text}")
                                    }
                                } else {
                                    format!("{label} {graph} {usage_text}")
                                }
                            }
                        }
                    }
                } else if graph_width == 0 {
                    pad_or_trim_visible(&text_only, segment_width.max(fixed))
                } else {
                    pad_or_trim_visible(
                        &match config.align {
                            Align::Left => {
                                if text_only.is_empty() {
                                    graph.clone()
                                } else {
                                    format!("{label}{separator}{usage_text} {graph}")
                                }
                            }
                            Align::Right => {
                                if label.is_empty() {
                                    if usage_text.is_empty() {
                                        graph.clone()
                                    } else {
                                        format!("{graph} {usage_text}")
                                    }
                                } else {
                                    format!("{label} {graph} {usage_text}")
                                }
                            }
                        },
                        segment_width.max(fixed),
                    )
                };

                if matches!(value, Some(CanonicalValue::Unavailable) | None) {
                    pad_or_trim_visible(&text, segment_width.max(fixed))
                } else {
                    text
                }
            },
        )
        .collect::<Vec<_>>();

    pad_or_trim_visible(&format!("{prefix}{}", rendered.join(" ")), width)
}

fn document_item_label(item: &Item) -> String {
    match item.display() {
        DisplayMode::Full => {
            item.label()
                .map(str::to_owned)
                .unwrap_or_else(|| match item.source() {
                    Source::Metric(metric) => metric.short_label().to_owned(),
                    Source::SplitMetric(_, _) => String::new(),
                    Source::StreamColumn(_) | Source::File(_) | Source::Process(_) => String::new(),
                })
        }
        DisplayMode::Value | DisplayMode::Bare => String::new(),
    }
}

fn document_item_usage_text(
    item: &Item,
    value: Option<CanonicalValue>,
    headline_value: Option<HeadlineValue>,
) -> String {
    if !matches!(item.display(), DisplayMode::Full | DisplayMode::Value) {
        return String::new();
    }

    match item.source() {
        Source::Metric(metric) => value
            .and_then(CanonicalValue::normalized_metric_value)
            .map(|value| {
                metric.format_value(
                    item.view(),
                    value.headline_value(),
                    &headline_value
                        .unwrap_or_else(|| HeadlineValue::Scalar(value.headline_value())),
                )
            })
            .unwrap_or_else(|| "N/A".to_owned()),
        Source::SplitMetric(_, _) => String::new(),
        Source::StreamColumn(_) | Source::File(_) | Source::Process(_) => String::new(),
    }
}

fn document_render_metric(source: &Source) -> MetricKind {
    match source {
        Source::Metric(metric) => *metric,
        Source::SplitMetric(_, _) => MetricKind::Sys,
        Source::StreamColumn(_) | Source::File(_) | Source::Process(_) => MetricKind::Cpu,
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
            let fixed =
                prefix.chars().count() + row_label.chars().count() + usage_text.chars().count();

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
            let item_hues =
                metric_hues_for_visible_hue(metric, stream_hues[index % stream_hues.len()]);
            let graph = match config.renderer {
                Renderer::Braille => render_braille_graph(
                    &metric_history,
                    graph_width,
                    metric,
                    Some(&item_hues),
                    color_enabled,
                ),
                Renderer::Block => render_block_graph(
                    &metric_history,
                    graph_width,
                    metric,
                    Some(&item_hues),
                    color_enabled,
                ),
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
            let fixed =
                label.chars().count() + separator.chars().count() + usage_text.chars().count();

            (index, label, separator, usage_text, fixed)
        })
        .collect::<Vec<_>>();
    let (segment_widths, graph_widths) = stream_column_widths(config.space, inner_width, &segments);

    let segments = segments
        .into_iter()
        .zip(segment_widths.into_iter().zip(graph_widths))
        .map(
            |((index, label, separator, usage_text, _fixed), (segment_width, graph_width))| {
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
                let item_hues =
                    metric_hues_for_visible_hue(metric, stream_hues[index % stream_hues.len()]);
                let display_usage_text = match config.space {
                    Space::Stable => {
                        format!(
                            "{usage_text:>width$}",
                            width = stable_stream_usage_width(&usage_text)
                        )
                    }
                    Space::Graph | Space::Segment => usage_text.clone(),
                };
                let graph = match config.renderer {
                    Renderer::Braille => render_braille_graph(
                        &metric_history,
                        graph_width,
                        metric,
                        Some(&item_hues),
                        color_enabled,
                    ),
                    Renderer::Block => render_block_graph(
                        &metric_history,
                        graph_width,
                        metric,
                        Some(&item_hues),
                        color_enabled,
                    ),
                };
                let text_only = format!("{label}{separator}{display_usage_text}");

                if graph_width == 0 {
                    return pad_or_trim_visible(&text_only, segment_width);
                }

                pad_or_trim_visible(
                    &match config.align {
                        Align::Left => format!("{label}{separator}{display_usage_text} {graph}"),
                        Align::Right => {
                            if label.is_empty() {
                                format!("{graph} {display_usage_text}")
                            } else {
                                format!("{label} {graph} {display_usage_text}")
                            }
                        }
                    },
                    segment_width,
                )
            },
        )
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
        Space::Stable => {
            let fixed_total = segments
                .iter()
                .map(|(_, label, separator, usage_text, _)| {
                    label.chars().count()
                        + separator.chars().count()
                        + stable_stream_usage_width(usage_text)
                        + 1
                })
                .sum::<usize>();
            let graph_space = inner_width
                .saturating_sub(separators)
                .saturating_sub(fixed_total);
            let graph_widths = split_stream_widths(graph_space, count);
            let segment_widths = segments
                .iter()
                .zip(graph_widths.iter().copied())
                .map(|((_, label, separator, usage_text, _), graph_width)| {
                    label.chars().count()
                        + separator.chars().count()
                        + stable_stream_usage_width(usage_text)
                        + usize::from(graph_width > 0)
                        + graph_width
                })
                .collect::<Vec<_>>();
            (segment_widths, graph_widths)
        }
        Space::Graph => {
            let fixed_total = segments
                .iter()
                .map(|(_, _, _, _, fixed)| fixed + 1)
                .sum::<usize>();
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

fn stable_stream_usage_width(usage_text: &str) -> usize {
    usage_text.chars().count().max(4)
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
    stable_column_widths: Option<&[usize]>,
) -> String {
    let prefix = config
        .label
        .as_ref()
        .map(|label| format!("{label} "))
        .unwrap_or_default();
    let stable_layout = matches!(config.space, Space::Stable);
    let inner_width = width.saturating_sub(prefix.chars().count());
    let separators = items.len().saturating_sub(1);
    let segment_space = inner_width.saturating_sub(separators);

    let (widths, graph_widths) = if let Some(stable_column_widths) = stable_column_widths {
        let widths = stable_column_widths[..items.len()].to_vec();
        let graph_widths = items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let metric = item.metric();
                let fixed = if let Some(value) = values.get(&metric).copied() {
                    segment_fixed_width(
                        metric,
                        item.view(),
                        item.display(),
                        value,
                        headline_values.get(&metric).copied(),
                        label_width,
                        stable_layout,
                    )
                } else {
                    unavailable_segment_fixed_width(
                        metric,
                        item.view(),
                        item.display(),
                        label_width,
                        stable_layout,
                    )
                };
                widths[index].saturating_sub(fixed + 1)
            })
            .collect::<Vec<_>>();
        (widths, graph_widths)
    } else {
        let fixed_widths = items
            .iter()
            .map(|item| {
                let metric = item.metric();
                if let Some(value) = values.get(&metric).copied() {
                    segment_fixed_width(
                        metric,
                        item.view(),
                        item.display(),
                        value,
                        headline_values.get(&metric).copied(),
                        label_width,
                        stable_layout,
                    )
                } else {
                    unavailable_segment_fixed_width(
                        metric,
                        item.view(),
                        item.display(),
                        label_width,
                        stable_layout,
                    )
                }
            })
            .collect::<Vec<_>>();
        let fixed_total = fixed_widths.iter().sum::<usize>();

        let graph_widths = if !items.is_empty() && segment_space >= fixed_total + items.len() {
            split_weighted_width(
                segment_space - fixed_total - items.len(),
                &normalized_items_for_sizing(items),
            )
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
        (widths, graph_widths)
    };

    let segments = items
        .iter()
        .zip(widths)
        .zip(graph_widths)
        .enumerate()
        .map(|(index, ((item, width), graph_width))| {
            let metric = item.metric();
            let history = histories.get(&metric).cloned().unwrap_or_default();
            let item_hues = metric_hues_for_visible_hue(metric, row_hues[index % row_hues.len()]);
            if let Some(value) = values.get(&metric).copied() {
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
                        stable_layout,
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
                        stable_layout,
                    )
                }
            } else {
                render_unavailable_segment(
                    *item,
                    width,
                    label_width,
                    graph_width,
                    config.align,
                    config.renderer,
                    color_enabled,
                    stable_layout,
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
        false,
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

fn normalized_items_for_sizing(items: &[LayoutItem]) -> Vec<LayoutItem> {
    items
        .iter()
        .map(|item| {
            let grow = if item.grow() == 0 {
                item.basis().unwrap_or(0)
            } else {
                item.grow()
            };
            LayoutItem::with_constraints(
                item.metric(),
                item.view(),
                item.display(),
                item.basis(),
                grow,
                item.max_width(),
                item.min_width(),
            )
        })
        .collect()
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
    stable_layout: bool,
) -> String {
    let metric = item.metric();
    let (label, label_usage_separator, usage_text, fixed) = segment_text_parts(
        metric,
        item.view(),
        item.display(),
        value,
        headline_value,
        label_width,
        stable_layout,
    );

    if width <= fixed {
        return pad_or_trim_visible(
            &format!("{label}{label_usage_separator}{usage_text}"),
            width,
        );
    }

    let graph_width = width.saturating_sub(fixed + 1);
    let samples = history.iter().copied().collect::<Vec<_>>();
    let graph = match renderer {
        Renderer::Braille => {
            render_braille_graph(&samples, graph_width, metric, hues, color_enabled)
        }
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
    stable_layout: bool,
) -> String {
    let metric = item.metric();
    let (label, label_usage_separator, usage_text, fixed) = segment_text_parts(
        metric,
        item.view(),
        item.display(),
        value,
        headline_value,
        label_width,
        stable_layout,
    );
    let samples = history.iter().copied().collect::<Vec<_>>();
    let graph = match renderer {
        Renderer::Braille => {
            render_braille_graph(&samples, graph_width, metric, hues, color_enabled)
        }
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

fn render_unavailable_segment(
    item: LayoutItem,
    width: usize,
    label_width: usize,
    graph_width: usize,
    align: Align,
    renderer: Renderer,
    color_enabled: bool,
    stable_layout: bool,
) -> String {
    let metric = item.metric();
    let (label, label_usage_separator, usage_text, fixed) = unavailable_segment_text_parts(
        metric,
        item.view(),
        item.display(),
        label_width,
        stable_layout,
    );
    let label = paint_unavailable_text(&label, color_enabled);
    let usage_text = paint_unavailable_text(&usage_text, color_enabled);
    let graph = render_unavailable_graph(graph_width, renderer, color_enabled);

    let text = if graph_width > 0 {
        match align {
            Align::Left => format!("{label}{label_usage_separator}{usage_text} {graph}"),
            Align::Right => format!("{label} {graph} {usage_text}"),
        }
    } else {
        format!("{label}{label_usage_separator}{usage_text}")
    };

    pad_or_trim_visible(&text, width.max(fixed))
}

fn render_grid_segment(
    item: LayoutItem,
    history: &VecDeque<MetricValue>,
    value: MetricValue,
    headline_value: Option<HeadlineValue>,
    spec: GridColumnSpec,
    renderer: Renderer,
    hues: Option<&BaseHues>,
    color_enabled: bool,
    stable_layout: bool,
) -> String {
    let metric = item.metric();
    let usage_text = segment_usage_text(metric, item.view(), value, headline_value, stable_layout);
    let samples = history.iter().copied().collect::<Vec<_>>();
    let graph = match renderer {
        Renderer::Braille => {
            render_braille_graph(&samples, spec.graph_width, metric, hues, color_enabled)
        }
        Renderer::Block => {
            render_block_graph(&samples, spec.graph_width, metric, hues, color_enabled)
        }
    };

    let text = render_grid_text(
        item.display(),
        metric.short_label(),
        &usage_text,
        spec,
        Some(&graph),
    );

    pad_or_trim_visible(&text, spec.total_width)
}

fn render_unavailable_grid_segment(
    item: LayoutItem,
    spec: GridColumnSpec,
    renderer: Renderer,
    color_enabled: bool,
    stable_layout: bool,
) -> String {
    let metric = item.metric();
    let usage_text = unavailable_usage_text(metric, item.view(), stable_layout);
    let graph = render_unavailable_graph(spec.graph_width, renderer, color_enabled);
    let text = render_grid_text(
        item.display(),
        &paint_unavailable_text(metric.short_label(), color_enabled),
        &paint_unavailable_text(&usage_text, color_enabled),
        spec,
        Some(&graph),
    );

    pad_or_trim_visible(&text, spec.total_width)
}

fn render_grid_text(
    display: DisplayMode,
    label: &str,
    usage_text: &str,
    spec: GridColumnSpec,
    graph: Option<&str>,
) -> String {
    match display {
        DisplayMode::Full => {
            let label = format!("{label:<label_width$}", label_width = spec.label_width);
            let usage_text = format!("{usage_text:>value_width$}", value_width = spec.value_width);
            match (graph, spec.graph_width > 0) {
                (Some(graph), true) => format!("{label} {usage_text} {graph}"),
                _ => format!("{label} {usage_text}"),
            }
        }
        DisplayMode::Value => {
            let usage_text = format!("{usage_text:>value_width$}", value_width = spec.value_width);
            match (graph, spec.graph_width > 0) {
                (Some(graph), true) => format!("{usage_text} {graph}"),
                _ => usage_text,
            }
        }
        DisplayMode::Bare => match (graph, spec.graph_width > 0) {
            (Some(graph), true) => graph.to_owned(),
            _ => String::new(),
        },
    }
}

fn segment_text_parts(
    metric: MetricKind,
    view: LayoutView,
    display: DisplayMode,
    value: MetricValue,
    headline_value: Option<HeadlineValue>,
    _label_width: usize,
    stable_layout: bool,
) -> (String, &'static str, String, usize) {
    let label = match display {
        DisplayMode::Full => metric.short_label().to_owned(),
        DisplayMode::Value | DisplayMode::Bare => String::new(),
    };
    let usage_text = match display {
        DisplayMode::Full | DisplayMode::Value => {
            segment_usage_text(metric, view, value, headline_value, stable_layout)
        }
        DisplayMode::Bare => String::new(),
    };
    let label_usage_separator = match (display, metric) {
        (DisplayMode::Full, MetricKind::Vram) => "",
        (DisplayMode::Full, _) => " ",
        (DisplayMode::Value | DisplayMode::Bare, _) => "",
    };
    let fixed =
        label.chars().count() + label_usage_separator.chars().count() + usage_text.chars().count();
    (label, label_usage_separator, usage_text, fixed)
}

fn unavailable_segment_text_parts(
    metric: MetricKind,
    view: LayoutView,
    display: DisplayMode,
    _label_width: usize,
    stable_layout: bool,
) -> (String, &'static str, String, usize) {
    let label = match display {
        DisplayMode::Full => metric.short_label().to_owned(),
        DisplayMode::Value | DisplayMode::Bare => String::new(),
    };
    let usage_text = match display {
        DisplayMode::Full | DisplayMode::Value => {
            unavailable_usage_text(metric, view, stable_layout)
        }
        DisplayMode::Bare => String::new(),
    };
    let label_usage_separator = match (display, metric) {
        (DisplayMode::Full, MetricKind::Vram) => "",
        (DisplayMode::Full, _) => " ",
        (DisplayMode::Value | DisplayMode::Bare, _) => "",
    };
    let fixed =
        visible_width(&label) + label_usage_separator.chars().count() + visible_width(&usage_text);
    (label, label_usage_separator, usage_text, fixed)
}

fn segment_usage_text(
    metric: MetricKind,
    view: LayoutView,
    value: MetricValue,
    headline_value: Option<HeadlineValue>,
    stable_layout: bool,
) -> String {
    let usage_text = metric.format_value(
        view,
        value.headline_value(),
        &headline_value.unwrap_or_else(|| HeadlineValue::Scalar(value.headline_value())),
    );
    if stable_layout {
        format!(
            "{usage_text:>width$}",
            width = stable_layout_usage_width(metric, view)
        )
    } else {
        usage_text
    }
}

fn unavailable_usage_text(metric: MetricKind, view: LayoutView, stable_layout: bool) -> String {
    let text = match effective_view(metric, view) {
        LayoutView::Pct => "N/A",
        LayoutView::Hum => "N/A",
        LayoutView::Free => "N/A",
        LayoutView::Default => unreachable!(),
    };
    if stable_layout {
        format!(
            "{text:>width$}",
            width = stable_layout_usage_width(metric, view)
        )
    } else {
        text.to_owned()
    }
}

fn display_label_width(metric: MetricKind, display: DisplayMode) -> usize {
    match display {
        DisplayMode::Full => metric.short_label().chars().count(),
        DisplayMode::Value | DisplayMode::Bare => 0,
    }
}

fn display_value_width(
    metric: MetricKind,
    view: LayoutView,
    display: DisplayMode,
    value: MetricValue,
    headline_value: Option<HeadlineValue>,
    stable_layout: bool,
) -> usize {
    match display {
        DisplayMode::Full | DisplayMode::Value => {
            segment_usage_text(metric, view, value, headline_value, stable_layout)
                .chars()
                .count()
        }
        DisplayMode::Bare => 0,
    }
}

fn unavailable_display_value_width(
    metric: MetricKind,
    view: LayoutView,
    display: DisplayMode,
    stable_layout: bool,
) -> usize {
    match display {
        DisplayMode::Full | DisplayMode::Value => {
            unavailable_usage_text(metric, view, stable_layout)
                .chars()
                .count()
        }
        DisplayMode::Bare => 0,
    }
}

fn segment_fixed_width(
    metric: MetricKind,
    view: LayoutView,
    display: DisplayMode,
    value: MetricValue,
    headline_value: Option<HeadlineValue>,
    label_width: usize,
    stable_layout: bool,
) -> usize {
    segment_text_parts(
        metric,
        view,
        display,
        value,
        headline_value,
        label_width,
        stable_layout,
    )
    .3
}

fn unavailable_segment_fixed_width(
    metric: MetricKind,
    view: LayoutView,
    display: DisplayMode,
    label_width: usize,
    stable_layout: bool,
) -> usize {
    unavailable_segment_text_parts(metric, view, display, label_width, stable_layout).3
}

fn effective_view(metric: MetricKind, view: LayoutView) -> LayoutView {
    match view {
        LayoutView::Default => metric.default_view(),
        other => other,
    }
}

fn stable_layout_usage_width(metric: MetricKind, view: LayoutView) -> usize {
    let effective_view = effective_view(metric, view);

    match effective_view {
        LayoutView::Pct => 4,
        LayoutView::Hum => match metric {
            MetricKind::Io | MetricKind::Net | MetricKind::Ingress | MetricKind::Egress => 4,
            MetricKind::Memory | MetricKind::Storage => 9,
            _ => 4,
        },
        LayoutView::Free => match metric {
            MetricKind::Memory | MetricKind::Storage => 4,
            _ => 4,
        },
        LayoutView::Default => unreachable!(),
    }
}

fn render_unavailable_graph(width: usize, renderer: Renderer, color_enabled: bool) -> String {
    let glyph = match renderer {
        Renderer::Braille => UNAVAILABLE_GRAPH_BRAILLE,
        Renderer::Block => UNAVAILABLE_GRAPH_BLOCK,
    };
    let graph = glyph.to_string().repeat(width);
    paint(&graph, UNAVAILABLE_GRAPH_COLOR, color_enabled)
}

fn paint_unavailable_text(text: &str, color_enabled: bool) -> String {
    if !color_enabled {
        return text.to_owned();
    }

    format!("\x1b[30;48;2;210;210;210m{text}\x1b[0m")
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

pub(crate) fn visible_width(text: &str) -> usize {
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
        let upper_intensity = uppers.get(index).copied().unwrap_or(0.0);
        let lower_intensity = lowers.get(index).copied().unwrap_or(0.0);
        let upper = quantize_visible_half_level(upper_intensity);
        let lower = quantize_visible_half_level(lower_intensity);

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

fn quantize_visible_half_level(sample: f64) -> usize {
    let level = quantize_half_level(sample);
    if sample > 0.0 && level == 0 {
        1
    } else {
        level
    }
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
        let ch = braille_half_cell(1, 1).to_string();
        let upper_color = interpolate(upper_gradient, 0.0);
        let lower_color = interpolate(lower_gradient, 0.0);
        let color = blend_split_color(1, 1, upper_color, lower_color);
        return paint(&ch, color, color_enabled);
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
            document: None,
            history: 8,
            interval_ms: 1000,
            align: Align::Left,
            label: None,
            stream_labels: None,
            stream_groups: None,
            stream_layout: StreamLayout::Columns,
            space: Space::Graph,
            layout_engine: LayoutEngine::Flow,
            layout: Layout::default(),
            renderer: Renderer::Braille,
            color_mode: ColorMode::Never,
            output_mode: OutputMode::Terminal,
            width: Some(40),
            once: true,
            colors: None,
            force_stream_input: false,
            external_input: None,
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
                document: None,
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
            document: None,
            history: 8,
            interval_ms: 1000,
            align: Align::Left,
            label: None,
            stream_labels: None,
            stream_groups: None,
            stream_layout: StreamLayout::Columns,
            space: Space::Graph,
            layout_engine: LayoutEngine::Flow,
            layout: layout.clone(),
            renderer: Renderer::Block,
            color_mode: ColorMode::Never,
            output_mode: OutputMode::Terminal,
            width: Some(18),
            once: true,
            colors: None,
            force_stream_input: false,
            external_input: None,
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
            document: None,
            history: 8,
            interval_ms: 1000,
            align: Align::Left,
            label: Some("host".to_owned()),
            stream_labels: None,
            stream_groups: None,
            stream_layout: StreamLayout::Columns,
            space: Space::Graph,
            layout_engine: LayoutEngine::Flow,
            layout: crate::layout::parse_layout_spec("cpu gpu").unwrap(),
            renderer: Renderer::Braille,
            color_mode: ColorMode::Never,
            output_mode: OutputMode::Terminal,
            width: Some(40),
            once: true,
            colors: None,
            force_stream_input: false,
            external_input: None,
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
            document: None,
            history: 8,
            interval_ms: 1000,
            align: Align::Left,
            label: None,
            stream_labels: None,
            stream_groups: None,
            stream_layout: StreamLayout::Columns,
            space: Space::Graph,
            layout_engine: LayoutEngine::Flow,
            layout: crate::layout::parse_layout_spec("cpu ram spc io net").unwrap(),
            renderer: Renderer::Braille,
            color_mode: ColorMode::Never,
            output_mode: OutputMode::Terminal,
            width: Some(80),
            once: true,
            colors: None,
            force_stream_input: false,
            external_input: None,
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
            (
                MetricKind::Memory,
                HeadlineValue::Memory {
                    used_bytes: 15 * 1024 * 1024 * 1024,
                    available_bytes: 17 * 1024 * 1024 * 1024,
                    total_bytes: 32 * 1024 * 1024 * 1024,
                },
            ),
            (
                MetricKind::Storage,
                HeadlineValue::Storage {
                    used_bytes: 12,
                    total_bytes: 1024,
                },
            ),
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
        assert!(line.contains("net  14K "), "unexpected row: {line}");
    }

    #[test]
    fn stream_columns_default_to_single_row_without_series_labels() {
        let config = Config {
            document: None,
            history: 8,
            interval_ms: 1000,
            align: Align::Left,
            label: None,
            stream_labels: None,
            stream_groups: None,
            stream_layout: StreamLayout::Columns,
            space: Space::Graph,
            layout_engine: LayoutEngine::Flow,
            layout: Layout::default(),
            renderer: Renderer::Braille,
            color_mode: ColorMode::Never,
            output_mode: OutputMode::Terminal,
            width: Some(24),
            once: true,
            colors: None,
            force_stream_input: false,
            external_input: None,
            print_completion: None,
            debug_colors_steps: None,
            show_help: false,
        };
        let histories = vec![
            VecDeque::from(vec![0.0, 0.25]),
            VecDeque::from(vec![0.5, 0.75]),
        ];
        let values = vec![0.25, 0.75];

        let lines = render_stream_lines(&config, 24, false, &histories, &values);

        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("25%"), "unexpected row: {}", lines[0]);
        assert!(lines[0].contains("75%"), "unexpected row: {}", lines[0]);
    }

    #[test]
    fn stream_columns_use_explicit_labels_when_given() {
        let config = Config {
            document: None,
            history: 8,
            interval_ms: 1000,
            align: Align::Left,
            label: None,
            stream_labels: Some(vec!["wifi".to_owned(), "vpn".to_owned()]),
            stream_groups: None,
            stream_layout: StreamLayout::Columns,
            space: Space::Graph,
            layout_engine: LayoutEngine::Flow,
            layout: Layout::default(),
            renderer: Renderer::Braille,
            color_mode: ColorMode::Never,
            output_mode: OutputMode::Terminal,
            width: Some(28),
            once: true,
            colors: None,
            force_stream_input: false,
            external_input: None,
            print_completion: None,
            debug_colors_steps: None,
            show_help: false,
        };
        let histories = vec![
            VecDeque::from(vec![0.0, 0.25]),
            VecDeque::from(vec![0.5, 0.75]),
        ];
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
            document: None,
            history: 8,
            interval_ms: 1000,
            align: Align::Left,
            label: None,
            stream_labels: Some(vec![
                "a".to_owned(),
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
            ]),
            stream_groups: None,
            stream_layout: StreamLayout::Columns,
            space: Space::Graph,
            layout_engine: LayoutEngine::Flow,
            layout: Layout::default(),
            renderer: Renderer::Braille,
            color_mode: ColorMode::Never,
            output_mode: OutputMode::Terminal,
            width: Some(120),
            once: true,
            colors: None,
            force_stream_input: false,
            external_input: None,
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

        let (segment_widths_stable, graph_widths_stable) =
            stream_column_widths(Space::Stable, 120, &segments);
        let (_, graph_widths_graph) = stream_column_widths(Space::Graph, 120, &segments);
        let (_, graph_widths_segment) = stream_column_widths(Space::Segment, 120, &segments);

        assert_eq!(graph_widths_stable[0], graph_widths_stable[1]);
        assert!(segment_widths_stable[1] > segment_widths_stable[0]);
        assert_eq!(graph_widths_graph[0], graph_widths_graph[1]);
        assert!(graph_widths_segment[0] > graph_widths_segment[1]);
    }

    #[test]
    fn stable_stream_spacing_reserves_prefix_width_for_short_percentages() {
        let config = Config {
            document: None,
            history: 8,
            interval_ms: 1000,
            align: Align::Left,
            label: None,
            stream_labels: Some(vec!["a".to_owned(), "b".to_owned()]),
            stream_groups: None,
            stream_layout: StreamLayout::Columns,
            space: Space::Stable,
            layout_engine: LayoutEngine::Flow,
            layout: Layout::default(),
            renderer: Renderer::Braille,
            color_mode: ColorMode::Never,
            output_mode: OutputMode::Terminal,
            width: Some(40),
            once: true,
            colors: None,
            force_stream_input: false,
            external_input: None,
            print_completion: None,
            debug_colors_steps: None,
            show_help: false,
        };
        let histories = vec![
            VecDeque::from(vec![0.09, 0.09]),
            VecDeque::from(vec![1.0, 1.0]),
        ];
        let values = vec![0.09, 1.0];

        let line = render_stream_columns_line(&config, 40, false, &histories, &values);
        assert!(line.contains("a   9% "));
        assert!(line.contains("b 100% "));
    }

    #[test]
    fn stable_native_rows_align_columns_across_rows() {
        fn visible_offset(line: &str, token: &str) -> usize {
            let byte_index = line
                .find(token)
                .unwrap_or_else(|| panic!("missing token {token:?} in {line:?}"));
            visible_width(&line[..byte_index])
        }

        let layout = crate::layout::parse_layout_spec("cpu ram spc io, net sys rx tx").unwrap();
        let config = Config {
            document: None,
            history: 8,
            interval_ms: 1000,
            align: Align::Left,
            label: None,
            stream_labels: None,
            stream_groups: None,
            stream_layout: StreamLayout::Columns,
            space: Space::Stable,
            layout_engine: LayoutEngine::Flow,
            layout: layout.clone(),
            renderer: Renderer::Braille,
            color_mode: ColorMode::Never,
            output_mode: OutputMode::Terminal,
            width: Some(80),
            once: true,
            colors: None,
            force_stream_input: false,
            external_input: None,
            print_completion: None,
            debug_colors_steps: None,
            show_help: false,
        };
        let histories = HashMap::from([
            (
                MetricKind::Cpu,
                VecDeque::from(vec![MetricValue::Single(0.07)]),
            ),
            (
                MetricKind::Memory,
                VecDeque::from(vec![MetricValue::Single(0.44)]),
            ),
            (
                MetricKind::Storage,
                VecDeque::from(vec![MetricValue::Single(0.89)]),
            ),
            (
                MetricKind::Io,
                VecDeque::from(vec![MetricValue::Split {
                    upper: 0.2,
                    lower: 0.1,
                }]),
            ),
            (
                MetricKind::Net,
                VecDeque::from(vec![MetricValue::Split {
                    upper: 0.3,
                    lower: 0.2,
                }]),
            ),
            (
                MetricKind::Sys,
                VecDeque::from(vec![MetricValue::Split {
                    upper: 0.4,
                    lower: 0.4,
                }]),
            ),
            (
                MetricKind::Ingress,
                VecDeque::from(vec![MetricValue::Single(0.25)]),
            ),
            (
                MetricKind::Egress,
                VecDeque::from(vec![MetricValue::Single(0.2)]),
            ),
        ]);
        let values = HashMap::from([
            (MetricKind::Cpu, MetricValue::Single(0.07)),
            (MetricKind::Memory, MetricValue::Single(0.44)),
            (MetricKind::Storage, MetricValue::Single(0.89)),
            (
                MetricKind::Io,
                MetricValue::Split {
                    upper: 0.2,
                    lower: 0.1,
                },
            ),
            (
                MetricKind::Net,
                MetricValue::Split {
                    upper: 0.3,
                    lower: 0.2,
                },
            ),
            (
                MetricKind::Sys,
                MetricValue::Split {
                    upper: 0.4,
                    lower: 0.4,
                },
            ),
            (MetricKind::Ingress, MetricValue::Single(0.25)),
            (MetricKind::Egress, MetricValue::Single(0.2)),
        ]);
        let headlines = HashMap::from([
            (
                MetricKind::Memory,
                HeadlineValue::Memory {
                    used_bytes: 15 * 1024 * 1024 * 1024,
                    available_bytes: 17 * 1024 * 1024 * 1024,
                    total_bytes: 32 * 1024 * 1024 * 1024,
                },
            ),
            (
                MetricKind::Storage,
                HeadlineValue::Storage {
                    used_bytes: 18,
                    total_bytes: 1024,
                },
            ),
            (MetricKind::Io, HeadlineValue::Scalar(32.0 * 1024.0)),
            (MetricKind::Net, HeadlineValue::Scalar(1.2 * 1024.0)),
            (MetricKind::Ingress, HeadlineValue::Scalar(1.0 * 1024.0)),
            (MetricKind::Egress, HeadlineValue::Scalar(200.0)),
        ]);

        let lines = render_lines_with_headlines(
            &config, 80, false, &histories, &layout, &values, &headlines,
        );

        assert_eq!(
            visible_offset(&lines[0], " ram "),
            visible_offset(&lines[1], " sys ")
        );
        assert_eq!(
            visible_offset(&lines[0], " spc "),
            visible_offset(&lines[1], " rx ")
        );
        assert_eq!(
            visible_offset(&lines[0], " io "),
            visible_offset(&lines[1], " tx ")
        );
    }

    #[test]
    fn grid_engine_aligns_graph_starts_across_rows() {
        fn graph_start(line: &str, token: &str) -> usize {
            let byte_index = line
                .find(token)
                .unwrap_or_else(|| panic!("missing token {token:?} in {line:?}"));
            visible_width(&line[..byte_index]) + visible_width(token)
        }

        let layout = crate::layout::parse_layout_spec("cpu ram spc io, net sys rx tx").unwrap();
        let config = Config {
            document: None,
            history: 8,
            interval_ms: 1000,
            align: Align::Left,
            label: None,
            stream_labels: None,
            stream_groups: None,
            stream_layout: StreamLayout::Columns,
            space: Space::Stable,
            layout_engine: LayoutEngine::Grid,
            layout: layout.clone(),
            renderer: Renderer::Braille,
            color_mode: ColorMode::Never,
            output_mode: OutputMode::Terminal,
            width: Some(80),
            once: true,
            colors: None,
            force_stream_input: false,
            external_input: None,
            print_completion: None,
            debug_colors_steps: None,
            show_help: false,
        };
        let histories = HashMap::from([
            (
                MetricKind::Cpu,
                VecDeque::from(vec![MetricValue::Single(0.07)]),
            ),
            (
                MetricKind::Memory,
                VecDeque::from(vec![MetricValue::Single(0.44)]),
            ),
            (
                MetricKind::Storage,
                VecDeque::from(vec![MetricValue::Single(0.89)]),
            ),
            (
                MetricKind::Io,
                VecDeque::from(vec![MetricValue::Split {
                    upper: 0.2,
                    lower: 0.1,
                }]),
            ),
            (
                MetricKind::Net,
                VecDeque::from(vec![MetricValue::Split {
                    upper: 0.3,
                    lower: 0.2,
                }]),
            ),
            (
                MetricKind::Sys,
                VecDeque::from(vec![MetricValue::Split {
                    upper: 0.4,
                    lower: 0.4,
                }]),
            ),
            (
                MetricKind::Ingress,
                VecDeque::from(vec![MetricValue::Single(0.25)]),
            ),
            (
                MetricKind::Egress,
                VecDeque::from(vec![MetricValue::Single(0.2)]),
            ),
        ]);
        let values = HashMap::from([
            (MetricKind::Cpu, MetricValue::Single(0.07)),
            (MetricKind::Memory, MetricValue::Single(0.44)),
            (MetricKind::Storage, MetricValue::Single(0.89)),
            (
                MetricKind::Io,
                MetricValue::Split {
                    upper: 0.2,
                    lower: 0.1,
                },
            ),
            (
                MetricKind::Net,
                MetricValue::Split {
                    upper: 0.3,
                    lower: 0.2,
                },
            ),
            (
                MetricKind::Sys,
                MetricValue::Split {
                    upper: 0.4,
                    lower: 0.4,
                },
            ),
            (MetricKind::Ingress, MetricValue::Single(0.25)),
            (MetricKind::Egress, MetricValue::Single(0.2)),
        ]);
        let headlines = HashMap::from([
            (
                MetricKind::Memory,
                HeadlineValue::Memory {
                    used_bytes: 15 * 1024 * 1024 * 1024,
                    available_bytes: 17 * 1024 * 1024 * 1024,
                    total_bytes: 32 * 1024 * 1024 * 1024,
                },
            ),
            (
                MetricKind::Storage,
                HeadlineValue::Storage {
                    used_bytes: 18,
                    total_bytes: 1024,
                },
            ),
            (MetricKind::Io, HeadlineValue::Scalar(32.0 * 1024.0)),
            (MetricKind::Net, HeadlineValue::Scalar(12.0 * 1024.0)),
            (MetricKind::Ingress, HeadlineValue::Scalar(526.0)),
            (MetricKind::Egress, HeadlineValue::Scalar(12.0 * 1024.0)),
        ]);

        let lines = render_lines_with_headlines(
            &config, 80, false, &histories, &layout, &values, &headlines,
        );

        assert_eq!(
            graph_start(&lines[0], "ram  17G "),
            graph_start(&lines[1], "sys  40% ")
        );
        assert_eq!(
            graph_start(&lines[0], "spc 1.0K "),
            graph_start(&lines[1], "rx  526B ")
        );
        assert_eq!(
            graph_start(&lines[0], "io  32K "),
            graph_start(&lines[1], "tx  12K ")
        );
    }

    #[test]
    fn auto_engine_uses_grid_for_multiline_layouts() {
        let layout = crate::layout::parse_layout_spec("cpu ram spc io, net sys in out").unwrap();
        let grid_config = Config {
            document: None,
            history: 8,
            interval_ms: 1000,
            align: Align::Left,
            label: None,
            stream_labels: None,
            stream_groups: None,
            stream_layout: StreamLayout::Columns,
            space: Space::Stable,
            layout_engine: LayoutEngine::Grid,
            layout: layout.clone(),
            renderer: Renderer::Braille,
            color_mode: ColorMode::Never,
            output_mode: OutputMode::Terminal,
            width: Some(80),
            once: true,
            colors: None,
            force_stream_input: false,
            external_input: None,
            print_completion: None,
            debug_colors_steps: None,
            show_help: false,
        };
        let auto_config = Config {
            document: None,
            layout_engine: LayoutEngine::Auto,
            ..grid_config.clone()
        };
        let histories = HashMap::from([
            (
                MetricKind::Cpu,
                VecDeque::from(vec![MetricValue::Single(0.07)]),
            ),
            (
                MetricKind::Memory,
                VecDeque::from(vec![MetricValue::Single(0.44)]),
            ),
            (
                MetricKind::Storage,
                VecDeque::from(vec![MetricValue::Single(0.89)]),
            ),
            (
                MetricKind::Io,
                VecDeque::from(vec![MetricValue::Split {
                    upper: 0.2,
                    lower: 0.1,
                }]),
            ),
            (
                MetricKind::Net,
                VecDeque::from(vec![MetricValue::Split {
                    upper: 0.6,
                    lower: 0.4,
                }]),
            ),
            (
                MetricKind::Sys,
                VecDeque::from(vec![MetricValue::Single(0.45)]),
            ),
            (
                MetricKind::Ingress,
                VecDeque::from(vec![MetricValue::Single(0.3)]),
            ),
            (
                MetricKind::Egress,
                VecDeque::from(vec![MetricValue::Single(0.2)]),
            ),
        ]);
        let values = HashMap::from([
            (MetricKind::Cpu, MetricValue::Single(0.07)),
            (MetricKind::Memory, MetricValue::Single(0.44)),
            (MetricKind::Storage, MetricValue::Single(0.89)),
            (
                MetricKind::Io,
                MetricValue::Split {
                    upper: 0.2,
                    lower: 0.1,
                },
            ),
            (
                MetricKind::Net,
                MetricValue::Split {
                    upper: 0.6,
                    lower: 0.4,
                },
            ),
            (MetricKind::Sys, MetricValue::Single(0.45)),
            (MetricKind::Ingress, MetricValue::Single(0.3)),
            (MetricKind::Egress, MetricValue::Single(0.2)),
        ]);
        let gib = 1024_u64 * 1024 * 1024;
        let headline_values = HashMap::from([
            (MetricKind::Cpu, HeadlineValue::Scalar(7.0)),
            (
                MetricKind::Memory,
                HeadlineValue::Memory {
                    used_bytes: 47 * gib,
                    available_bytes: 17 * gib,
                    total_bytes: 64 * gib,
                },
            ),
            (
                MetricKind::Storage,
                HeadlineValue::Storage {
                    used_bytes: 18 * gib,
                    total_bytes: 1024 * gib,
                },
            ),
            (MetricKind::Io, HeadlineValue::Scalar(0.0)),
            (MetricKind::Net, HeadlineValue::Scalar(5300.0)),
            (MetricKind::Sys, HeadlineValue::Scalar(45.0)),
            (MetricKind::Ingress, HeadlineValue::Scalar(3100.0)),
            (MetricKind::Egress, HeadlineValue::Scalar(2300.0)),
        ]);

        let auto_lines = render_lines_with_headlines(
            &auto_config,
            80,
            false,
            &histories,
            &layout,
            &values,
            &headline_values,
        );
        let grid_lines = render_lines_with_headlines(
            &grid_config,
            80,
            false,
            &histories,
            &layout,
            &values,
            &headline_values,
        );

        assert_eq!(auto_lines, grid_lines);
    }

    #[test]
    fn auto_engine_uses_flow_for_single_line_layouts() {
        let layout = crate::layout::parse_layout_spec("cpu ram spc io").unwrap();
        let base_config = Config {
            document: None,
            history: 8,
            interval_ms: 1000,
            align: Align::Left,
            label: None,
            stream_labels: None,
            stream_groups: None,
            stream_layout: StreamLayout::Columns,
            space: Space::Stable,
            layout_engine: LayoutEngine::Flow,
            layout: layout.clone(),
            renderer: Renderer::Braille,
            color_mode: ColorMode::Never,
            output_mode: OutputMode::Terminal,
            width: Some(80),
            once: true,
            colors: None,
            force_stream_input: false,
            external_input: None,
            print_completion: None,
            debug_colors_steps: None,
            show_help: false,
        };
        let auto_config = Config {
            document: None,
            layout_engine: LayoutEngine::Auto,
            ..base_config.clone()
        };
        let histories = HashMap::from([
            (
                MetricKind::Cpu,
                VecDeque::from(vec![MetricValue::Single(0.07)]),
            ),
            (
                MetricKind::Memory,
                VecDeque::from(vec![MetricValue::Single(0.44)]),
            ),
            (
                MetricKind::Storage,
                VecDeque::from(vec![MetricValue::Single(0.89)]),
            ),
            (
                MetricKind::Io,
                VecDeque::from(vec![MetricValue::Split {
                    upper: 0.2,
                    lower: 0.1,
                }]),
            ),
        ]);
        let values = HashMap::from([
            (MetricKind::Cpu, MetricValue::Single(0.07)),
            (MetricKind::Memory, MetricValue::Single(0.44)),
            (MetricKind::Storage, MetricValue::Single(0.89)),
            (
                MetricKind::Io,
                MetricValue::Split {
                    upper: 0.2,
                    lower: 0.1,
                },
            ),
        ]);
        let gib = 1024_u64 * 1024 * 1024;
        let headline_values = HashMap::from([
            (MetricKind::Cpu, HeadlineValue::Scalar(7.0)),
            (
                MetricKind::Memory,
                HeadlineValue::Memory {
                    used_bytes: 47 * gib,
                    available_bytes: 17 * gib,
                    total_bytes: 64 * gib,
                },
            ),
            (
                MetricKind::Storage,
                HeadlineValue::Storage {
                    used_bytes: 18 * gib,
                    total_bytes: 1024 * gib,
                },
            ),
            (MetricKind::Io, HeadlineValue::Scalar(0.0)),
        ]);

        let auto_lines = render_lines_with_headlines(
            &auto_config,
            80,
            false,
            &histories,
            &layout,
            &values,
            &headline_values,
        );
        let flow_lines = render_lines_with_headlines(
            &base_config,
            80,
            false,
            &histories,
            &layout,
            &values,
            &headline_values,
        );

        assert_eq!(auto_lines, flow_lines);
    }

    #[test]
    fn flex_engine_spans_underfull_rows_across_tracks() {
        let layout = crate::layout::parse_layout_spec("cpu ram spc, io").unwrap();
        let base_config = Config {
            document: None,
            history: 8,
            interval_ms: 1000,
            align: Align::Left,
            label: None,
            stream_labels: None,
            stream_groups: None,
            stream_layout: StreamLayout::Columns,
            space: Space::Stable,
            layout_engine: LayoutEngine::Grid,
            layout: layout.clone(),
            renderer: Renderer::Braille,
            color_mode: ColorMode::Never,
            output_mode: OutputMode::Terminal,
            width: Some(60),
            once: true,
            colors: None,
            force_stream_input: false,
            external_input: None,
            print_completion: None,
            debug_colors_steps: None,
            show_help: false,
        };
        let flex_config = Config {
            document: None,
            layout_engine: LayoutEngine::Flex,
            ..base_config.clone()
        };
        let histories = HashMap::from([
            (
                MetricKind::Cpu,
                VecDeque::from(vec![MetricValue::Single(0.07)]),
            ),
            (
                MetricKind::Memory,
                VecDeque::from(vec![MetricValue::Single(0.44)]),
            ),
            (
                MetricKind::Storage,
                VecDeque::from(vec![MetricValue::Single(0.89)]),
            ),
            (
                MetricKind::Io,
                VecDeque::from(vec![MetricValue::Split {
                    upper: 0.2,
                    lower: 0.1,
                }]),
            ),
        ]);
        let values = HashMap::from([
            (MetricKind::Cpu, MetricValue::Single(0.07)),
            (MetricKind::Memory, MetricValue::Single(0.44)),
            (MetricKind::Storage, MetricValue::Single(0.89)),
            (
                MetricKind::Io,
                MetricValue::Split {
                    upper: 0.2,
                    lower: 0.1,
                },
            ),
        ]);
        let gib = 1024_u64 * 1024 * 1024;
        let headline_values = HashMap::from([
            (MetricKind::Cpu, HeadlineValue::Scalar(7.0)),
            (
                MetricKind::Memory,
                HeadlineValue::Memory {
                    used_bytes: 47 * gib,
                    available_bytes: 17 * gib,
                    total_bytes: 64 * gib,
                },
            ),
            (
                MetricKind::Storage,
                HeadlineValue::Storage {
                    used_bytes: 18 * gib,
                    total_bytes: 1024 * gib,
                },
            ),
            (MetricKind::Io, HeadlineValue::Scalar(0.0)),
        ]);

        let grid_lines = render_lines_with_headlines(
            &base_config,
            60,
            false,
            &histories,
            &layout,
            &values,
            &headline_values,
        );
        let flex_lines = render_lines_with_headlines(
            &flex_config,
            60,
            false,
            &histories,
            &layout,
            &values,
            &headline_values,
        );

        assert_eq!(grid_lines.len(), 2);
        assert_eq!(flex_lines.len(), 2);
        assert!(visible_width(flex_lines[1].trim_end()) > visible_width(grid_lines[1].trim_end()));
    }

    #[test]
    fn flex_engine_honors_basis_hints_for_first_row_tracks() {
        let layout = crate::layout::parse_layout_spec("cpu:4 ram:8 spc:16, io, net").unwrap();
        let config = Config {
            document: None,
            history: 8,
            interval_ms: 1000,
            align: Align::Left,
            label: None,
            stream_labels: None,
            stream_groups: None,
            stream_layout: StreamLayout::Columns,
            space: Space::Stable,
            layout_engine: LayoutEngine::Flex,
            layout: layout.clone(),
            renderer: Renderer::Braille,
            color_mode: ColorMode::Never,
            output_mode: OutputMode::Terminal,
            width: Some(80),
            once: true,
            colors: None,
            force_stream_input: false,
            external_input: None,
            print_completion: None,
            debug_colors_steps: None,
            show_help: false,
        };
        let values = HashMap::from([
            (MetricKind::Cpu, MetricValue::Single(0.07)),
            (MetricKind::Memory, MetricValue::Single(0.44)),
            (MetricKind::Storage, MetricValue::Single(0.89)),
            (
                MetricKind::Io,
                MetricValue::Split {
                    upper: 0.2,
                    lower: 0.1,
                },
            ),
            (
                MetricKind::Net,
                MetricValue::Split {
                    upper: 0.3,
                    lower: 0.2,
                },
            ),
        ]);
        let gib = 1024_u64 * 1024 * 1024;
        let headline_values = HashMap::from([
            (MetricKind::Cpu, HeadlineValue::Scalar(7.0)),
            (
                MetricKind::Memory,
                HeadlineValue::Memory {
                    used_bytes: 47 * gib,
                    available_bytes: 17 * gib,
                    total_bytes: 64 * gib,
                },
            ),
            (
                MetricKind::Storage,
                HeadlineValue::Storage {
                    used_bytes: 18 * gib,
                    total_bytes: 1024 * gib,
                },
            ),
            (MetricKind::Io, HeadlineValue::Scalar(0.0)),
            (MetricKind::Net, HeadlineValue::Scalar(150.0)),
        ]);

        let specs = grid_column_specs(&config, 80, &layout, &values, &headline_values);
        assert_eq!(specs.len(), 3);
        assert!(specs[0].graph_width < specs[1].graph_width);
        assert!(specs[1].graph_width < specs[2].graph_width);
    }

    #[test]
    fn flex_engine_shares_column_text_widths_across_rows() {
        let layout =
            crate::layout::parse_layout_spec("cpu:3 ram:1, io:1 net:1, cpu:1 ram:1").unwrap();
        let config = Config {
            document: None,
            history: 8,
            interval_ms: 1000,
            align: Align::Left,
            label: None,
            stream_labels: None,
            stream_groups: None,
            stream_layout: StreamLayout::Columns,
            space: Space::Stable,
            layout_engine: LayoutEngine::Flex,
            layout: layout.clone(),
            renderer: Renderer::Braille,
            color_mode: ColorMode::Never,
            output_mode: OutputMode::Terminal,
            width: Some(100),
            once: true,
            colors: None,
            force_stream_input: false,
            external_input: None,
            print_completion: None,
            debug_colors_steps: None,
            show_help: false,
        };
        let values = HashMap::from([
            (MetricKind::Cpu, MetricValue::Single(0.04)),
            (MetricKind::Memory, MetricValue::Single(0.44)),
            (
                MetricKind::Io,
                MetricValue::Split {
                    upper: 0.0,
                    lower: 0.0,
                },
            ),
            (
                MetricKind::Net,
                MetricValue::Split {
                    upper: 0.1,
                    lower: 0.2,
                },
            ),
        ]);
        let gib = 1024_u64 * 1024 * 1024;
        let headline_values = HashMap::from([
            (MetricKind::Cpu, HeadlineValue::Scalar(4.0)),
            (
                MetricKind::Memory,
                HeadlineValue::Memory {
                    used_bytes: 46 * gib,
                    available_bytes: 18 * gib,
                    total_bytes: 64 * gib,
                },
            ),
            (MetricKind::Io, HeadlineValue::Scalar(0.0)),
            (MetricKind::Net, HeadlineValue::Scalar(2560.0)),
        ]);
        let prefix = String::new();
        let shared_text_widths =
            shared_column_text_widths(&layout, &values, &headline_values, true);
        let row_specs = flex_row_specs(
            &config,
            100,
            &prefix,
            &layout.rows()[1],
            &values,
            &headline_values,
            &shared_text_widths,
        );

        assert_eq!(row_specs.len(), 2);
        assert_eq!(row_specs[0].label_width, 3);
        assert_eq!(row_specs[0].value_width, 4);
        assert_eq!(row_specs[1].label_width, 3);
        assert_eq!(row_specs[1].value_width, 4);
    }

    #[test]
    fn flex_engine_aligns_equal_normalized_spans_across_rows() {
        fn visible_offset(line: &str, token: &str) -> usize {
            let byte_index = line
                .find(token)
                .unwrap_or_else(|| panic!("missing token {token:?} in {line:?}"));
            visible_width(&line[..byte_index])
        }

        let layout = crate::layout::parse_layout_spec("cpu:3 ram:1, ram:1 cpu:2 io:1").unwrap();
        let config = Config {
            document: None,
            history: 8,
            interval_ms: 1000,
            align: Align::Left,
            label: None,
            stream_labels: None,
            stream_groups: None,
            stream_layout: StreamLayout::Columns,
            space: Space::Stable,
            layout_engine: LayoutEngine::Flex,
            layout: layout.clone(),
            renderer: Renderer::Braille,
            color_mode: ColorMode::Never,
            output_mode: OutputMode::Terminal,
            width: Some(100),
            once: true,
            colors: None,
            force_stream_input: false,
            external_input: None,
            print_completion: None,
            debug_colors_steps: None,
            show_help: false,
        };
        let histories = HashMap::from([
            (
                MetricKind::Cpu,
                VecDeque::from(vec![MetricValue::Single(0.02)]),
            ),
            (
                MetricKind::Memory,
                VecDeque::from(vec![MetricValue::Single(0.44)]),
            ),
            (
                MetricKind::Io,
                VecDeque::from(vec![MetricValue::Split {
                    upper: 0.0,
                    lower: 0.0,
                }]),
            ),
        ]);
        let values = HashMap::from([
            (MetricKind::Cpu, MetricValue::Single(0.02)),
            (MetricKind::Memory, MetricValue::Single(0.44)),
            (
                MetricKind::Io,
                MetricValue::Split {
                    upper: 0.0,
                    lower: 0.0,
                },
            ),
        ]);
        let gib = 1024_u64 * 1024 * 1024;
        let headline_values = HashMap::from([
            (MetricKind::Cpu, HeadlineValue::Scalar(2.0)),
            (
                MetricKind::Memory,
                HeadlineValue::Memory {
                    used_bytes: 47 * gib,
                    available_bytes: 17 * gib,
                    total_bytes: 64 * gib,
                },
            ),
            (MetricKind::Io, HeadlineValue::Scalar(0.0)),
        ]);

        let lines = render_lines_with_headlines(
            &config,
            100,
            false,
            &histories,
            &layout,
            &values,
            &headline_values,
        );

        assert_eq!(lines.len(), 2);
        assert_eq!(
            visible_offset(&lines[0], "ram "),
            visible_offset(&lines[1], "io ")
        );
    }

    #[test]
    fn unavailable_metrics_remain_visible_in_layout() {
        let layout = crate::layout::parse_layout_spec("gpu gfx vram, cpu gpu").unwrap();
        let config = Config {
            document: None,
            history: 8,
            interval_ms: 1000,
            align: Align::Left,
            label: None,
            stream_labels: None,
            stream_groups: None,
            stream_layout: StreamLayout::Columns,
            space: Space::Stable,
            layout_engine: LayoutEngine::Grid,
            layout: layout.clone(),
            renderer: Renderer::Braille,
            color_mode: ColorMode::Always,
            output_mode: OutputMode::Terminal,
            width: Some(80),
            once: true,
            colors: None,
            force_stream_input: false,
            external_input: None,
            print_completion: None,
            debug_colors_steps: None,
            show_help: false,
        };
        let histories = HashMap::from([(
            MetricKind::Cpu,
            VecDeque::from(vec![MetricValue::Single(0.07)]),
        )]);
        let values = HashMap::from([(MetricKind::Cpu, MetricValue::Single(0.07))]);
        let headlines = HashMap::from([(MetricKind::Cpu, HeadlineValue::Scalar(7.0))]);

        let lines = render_lines_with_headlines(
            &config, 80, true, &histories, &layout, &values, &headlines,
        );

        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("gpu"));
        assert!(lines[0].contains("gfx"));
        assert!(lines[0].contains("vram"));
        assert!(lines[1].contains("cpu"));
        assert!(lines[1].contains("gpu"));
        assert!(lines[0].contains("\x1b[30;48;2;210;210;210m"));
        assert!(lines[0].contains("N/A"));
    }

    #[test]
    fn stream_lines_layout_renders_one_row_per_series() {
        let config = Config {
            document: None,
            history: 8,
            interval_ms: 1000,
            align: Align::Left,
            label: None,
            stream_labels: Some(vec!["wifi".to_owned(), "vpn".to_owned()]),
            stream_groups: None,
            stream_layout: StreamLayout::Lines,
            space: Space::Graph,
            layout_engine: LayoutEngine::Flow,
            layout: Layout::default(),
            renderer: Renderer::Braille,
            color_mode: ColorMode::Never,
            output_mode: OutputMode::Terminal,
            width: Some(28),
            once: true,
            colors: None,
            force_stream_input: false,
            external_input: None,
            print_completion: None,
            debug_colors_steps: None,
            show_help: false,
        };
        let histories = vec![
            VecDeque::from(vec![0.0, 0.25]),
            VecDeque::from(vec![0.5, 0.75]),
        ];
        let values = vec![0.25, 0.75];

        let lines = render_stream_lines(&config, 28, false, &histories, &values);

        assert_eq!(lines.len(), 2);
        assert!(
            lines[0].starts_with("wifi  25%"),
            "unexpected first row: {}",
            lines[0]
        );
        assert!(
            lines[1].starts_with(" vpn  75%"),
            "unexpected second row: {}",
            lines[1]
        );
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
    fn slash_grow_syntax_is_rejected() {
        let error = crate::layout::parse_layout_spec("cpu:12/2 ram:10 net.hum").unwrap_err();
        assert!(error.contains("invalid metric syntax"));
    }

    #[test]
    fn width_allocation_conformance_matrix() {
        let cases = [
            ("cpu:12 ram:10 net.hum", 30, vec![16, 13, 1]),
            ("cpu:12+14 ram:10 net.hum", 30, vec![14, 14, 2]),
            ("cpu-8 ram-4", 6, vec![4, 2]),
            ("cpu:12+20-8 ram+14-6", 24, vec![18, 6]),
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
                    DisplayMode::Full,
                    Some(12),
                    2,
                    Some(14),
                    None,
                ),
                LayoutItem::with_constraints(
                    MetricKind::Memory,
                    LayoutView::Default,
                    DisplayMode::Full,
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
                    DisplayMode::Full,
                    None,
                    1,
                    None,
                    Some(8),
                ),
                LayoutItem::with_constraints(
                    MetricKind::Memory,
                    LayoutView::Default,
                    DisplayMode::Full,
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
                    DisplayMode::Full,
                    None,
                    1,
                    None,
                    Some(8),
                ),
                LayoutItem::with_constraints(
                    MetricKind::Memory,
                    LayoutView::Default,
                    DisplayMode::Full,
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
            document: None,
            history: 8,
            interval_ms: 1000,
            align: Align::Left,
            label: None,
            stream_labels: None,
            stream_groups: None,
            stream_layout: StreamLayout::Columns,
            space: Space::Graph,
            layout_engine: LayoutEngine::Flow,
            layout: layout.clone(),
            renderer: Renderer::Braille,
            color_mode: ColorMode::Never,
            output_mode: OutputMode::Terminal,
            width: Some(16),
            once: true,
            colors: None,
            force_stream_input: false,
            external_input: None,
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
            document: None,
            history: 8,
            interval_ms: 1000,
            align: Align::Left,
            label: None,
            stream_labels: None,
            stream_groups: None,
            stream_layout: StreamLayout::Columns,
            space: Space::Graph,
            layout_engine: LayoutEngine::Flow,
            layout: layout.clone(),
            renderer: Renderer::Braille,
            color_mode: ColorMode::Never,
            output_mode: OutputMode::Terminal,
            width: Some(80),
            once: true,
            colors: None,
            force_stream_input: false,
            external_input: None,
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
            document: None,
            history: 8,
            interval_ms: 1000,
            align: Align::Left,
            label: None,
            stream_labels: None,
            stream_groups: None,
            stream_layout: StreamLayout::Columns,
            space: Space::Graph,
            layout_engine: LayoutEngine::Flow,
            layout: layout.clone(),
            renderer: Renderer::Braille,
            color_mode: ColorMode::Never,
            output_mode: OutputMode::Terminal,
            width: Some(24),
            once: true,
            colors: None,
            force_stream_input: false,
            external_input: None,
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

        assert!(lines[0].contains("3.5M"));
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
            false,
        );

        assert!(segment.contains("net   0B"));
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
            false,
        );

        assert!(segment.contains("vram  0%"));
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
            false,
        );

        assert!(io_small.starts_with("io 105K "));
        assert!(io_large.starts_with("io  14M "));
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
    fn tiny_nonzero_split_values_do_not_disappear() {
        let graph = render_braille_graph(
            &[MetricValue::Split {
                upper: 0.01,
                lower: 0.0,
            }],
            1,
            MetricKind::Net,
            None,
            false,
        );

        assert_ne!(graph, "⠀");
    }

    #[test]
    fn zero_split_values_render_with_darkest_split_color() {
        let graph = render_braille_graph(
            &[MetricValue::Split {
                upper: 0.0,
                lower: 0.0,
            }],
            1,
            MetricKind::Net,
            None,
            true,
        );

        let (upper_gradient, lower_gradient) = split_gradients_for(MetricKind::Net).unwrap();
        let expected = blend_split_color(
            1,
            1,
            interpolate(upper_gradient, 0.0),
            interpolate(lower_gradient, 0.0),
        );
        assert_ne!(graph, "⠀");
        assert!(graph.contains(&format!(
            "\x1b[38;2;{};{};{}m",
            expected.r, expected.g, expected.b
        )));
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

        assert!(gpu.starts_with("gpu"));
        assert!(gpu.contains("50%"));
        assert!(vram.starts_with("vram"));
        assert!(vram.contains("50%"));
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
