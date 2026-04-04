#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MetricKind {
    Cpu,
    Rnd,
    Sys,
    Gpu,
    Vram,
    Gfx,
    Memory,
    Storage,
    Io,
    Net,
    Ingress,
    Egress,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LayoutView {
    Default,
    Pct,
    Hum,
    Free,
}

impl LayoutView {
    pub fn parse(token: &str) -> Option<Self> {
        match token {
            "pct" => Some(Self::Pct),
            "hum" => Some(Self::Hum),
            "free" => Some(Self::Free),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisplayMode {
    Full,
    Value,
    Bare,
}

impl DisplayMode {
    pub fn parse(token: &str) -> Option<Self> {
        match token {
            "full" => Some(Self::Full),
            "value" => Some(Self::Value),
            "bare" => Some(Self::Bare),
            _ => None,
        }
    }
}

impl MetricKind {
    pub fn parse(token: &str) -> Option<Self> {
        match token {
            "cpu" => Some(Self::Cpu),
            "rnd" | "random" => Some(Self::Rnd),
            "sys" => Some(Self::Sys),
            "gpu" => Some(Self::Gpu),
            "vram" => Some(Self::Vram),
            "gfx" => Some(Self::Gfx),
            "memory" | "mem" | "ram" => Some(Self::Memory),
            "storage" | "disk" | "space" | "spc" => Some(Self::Storage),
            "io" => Some(Self::Io),
            "net" => Some(Self::Net),
            "ingress" | "in" => Some(Self::Ingress),
            "egress" | "out" => Some(Self::Egress),
            _ => None,
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Rnd => "rnd",
            Self::Sys => "sys",
            Self::Gpu => "gpu",
            Self::Vram => "vram",
            Self::Gfx => "gfx",
            Self::Memory => "ram",
            Self::Storage => "spc",
            Self::Io => "io",
            Self::Net => "net",
            Self::Ingress => "in",
            Self::Egress => "out",
        }
    }

    pub fn is_split(self) -> bool {
        matches!(self, Self::Sys | Self::Gfx | Self::Io | Self::Net)
    }

    pub fn default_view(self) -> LayoutView {
        match self {
            Self::Io | Self::Net | Self::Ingress | Self::Egress => LayoutView::Hum,
            Self::Memory | Self::Storage => LayoutView::Free,
            Self::Cpu | Self::Rnd | Self::Sys | Self::Gpu | Self::Vram | Self::Gfx => {
                LayoutView::Pct
            }
        }
    }

    pub fn format_value(
        self,
        view: LayoutView,
        normalized: f64,
        headline: &crate::metrics::HeadlineValue,
    ) -> String {
        let resolved = match view {
            LayoutView::Default => self.default_view(),
            other => other,
        };

        match resolved {
            LayoutView::Pct => format_percent(self, normalized),
            LayoutView::Hum => match self {
                Self::Rnd => humanize_bytes(headline.scalar().unwrap_or(0.0)),
                Self::Io | Self::Net | Self::Ingress | Self::Egress => {
                    humanize_rate(headline.scalar().unwrap_or(0.0))
                }
                Self::Memory => match headline {
                    crate::metrics::HeadlineValue::Memory {
                        used_bytes,
                        total_bytes,
                        ..
                    } => format!(
                        "{}/{}",
                        humanize_bytes(*used_bytes as f64),
                        humanize_bytes(*total_bytes as f64)
                    ),
                    _ => format_percent(self, normalized),
                },
                Self::Storage => match headline {
                    crate::metrics::HeadlineValue::Storage {
                        used_bytes,
                        total_bytes,
                    } => format!(
                        "{}/{}",
                        humanize_bytes(*used_bytes as f64),
                        humanize_bytes(*total_bytes as f64)
                    ),
                    _ => format_percent(self, normalized),
                },
                _ => format_percent(self, normalized),
            },
            LayoutView::Free => match self {
                Self::Rnd => humanize_bytes(headline.scalar().unwrap_or(0.0)),
                Self::Memory => match headline {
                    crate::metrics::HeadlineValue::Memory {
                        available_bytes, ..
                    } => humanize_bytes(*available_bytes as f64),
                    _ => format_percent(self, normalized),
                },
                Self::Storage => match headline {
                    crate::metrics::HeadlineValue::Storage {
                        used_bytes,
                        total_bytes,
                    } => humanize_bytes(total_bytes.saturating_sub(*used_bytes) as f64),
                    _ => format_percent(self, normalized),
                },
                _ => format_percent(self, normalized),
            },
            LayoutView::Default => unreachable!(),
        }
    }
}

fn format_percent(metric: MetricKind, value: f64) -> String {
    let percent = format!("{:>3.0}%", value.clamp(0.0, 1.0) * 100.0);
    match metric {
        MetricKind::Vram => percent,
        MetricKind::Cpu
        | MetricKind::Rnd
        | MetricKind::Sys
        | MetricKind::Gpu
        | MetricKind::Gfx
        | MetricKind::Memory
        | MetricKind::Storage
        | MetricKind::Io
        | MetricKind::Net
        | MetricKind::Ingress
        | MetricKind::Egress => percent,
    }
}

fn humanize_rate(bytes_per_second: f64) -> String {
    const UNITS: [&str; 5] = ["B", "K", "M", "G", "T"];
    humanize_compact_si(bytes_per_second, &UNITS)
}

fn humanize_bytes(bytes: f64) -> String {
    const UNITS: [&str; 5] = ["B", "K", "M", "G", "T"];
    humanize_compact_si(bytes, &UNITS)
}

fn humanize_compact_si(value: f64, units: &[&str]) -> String {
    let mut value = value.max(0.0);
    let mut unit = 0;
    loop {
        if value >= 1024.0 && unit + 1 < units.len() {
            value /= 1024.0;
            unit += 1;
            continue;
        }

        let rendered = if unit == 0 {
            format!("{:.0}{}", value, units[unit])
        } else if value < 9.95 {
            format!("{:.1}{}", value, units[unit])
        } else {
            format!("{:.0}{}", value, units[unit])
        };

        if rendered.chars().count() <= 4 || unit + 1 >= units.len() {
            return format!("{rendered:>4}");
        }

        value /= 1024.0;
        unit += 1;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LayoutItem {
    metric: MetricKind,
    view: LayoutView,
    display: DisplayMode,
    basis: Option<usize>,
    grow: usize,
    max_width: Option<usize>,
    min_width: Option<usize>,
}

impl LayoutItem {
    pub fn new(metric: MetricKind, view: LayoutView, basis: Option<usize>, grow: usize) -> Self {
        Self {
            metric,
            view,
            display: DisplayMode::Full,
            basis,
            grow,
            max_width: None,
            min_width: None,
        }
    }

    pub fn with_constraints(
        metric: MetricKind,
        view: LayoutView,
        display: DisplayMode,
        basis: Option<usize>,
        grow: usize,
        max_width: Option<usize>,
        min_width: Option<usize>,
    ) -> Self {
        Self {
            metric,
            view,
            display,
            basis,
            grow,
            max_width,
            min_width,
        }
    }

    pub fn metric(&self) -> MetricKind {
        self.metric
    }

    pub fn view(&self) -> LayoutView {
        self.view
    }

    pub fn basis(&self) -> Option<usize> {
        self.basis
    }

    pub fn display(&self) -> DisplayMode {
        self.display
    }

    pub fn grow(&self) -> usize {
        self.grow
    }

    pub fn max_width(&self) -> Option<usize> {
        self.max_width
    }

    pub fn min_width(&self) -> Option<usize> {
        self.min_width
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Layout {
    rows: Vec<Vec<LayoutItem>>,
    metrics: Vec<MetricKind>,
    explicit_rows: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Self::from_rows(split_even_rows(all_metrics(), 2))
    }
}

impl Layout {
    pub fn from_rows(rows: Vec<Vec<LayoutItem>>) -> Self {
        Self::from_rows_with_mode(rows, false)
    }

    fn from_explicit_rows(rows: Vec<Vec<LayoutItem>>) -> Self {
        Self::from_rows_with_mode(rows, true)
    }

    fn from_rows_with_mode(rows: Vec<Vec<LayoutItem>>, explicit_rows: bool) -> Self {
        let mut metrics = Vec::new();
        for row in &rows {
            for item in row {
                push_unique(&mut metrics, item.metric());
            }
        }

        Self {
            rows,
            metrics,
            explicit_rows,
        }
    }

    pub fn metrics(&self) -> &[MetricKind] {
        &self.metrics
    }

    pub fn rows(&self) -> &[Vec<LayoutItem>] {
        &self.rows
    }

    pub fn retain_available<F>(&self, mut keep: F) -> Self
    where
        F: FnMut(MetricKind) -> bool,
    {
        if self.explicit_rows {
            let rows = self
                .rows
                .iter()
                .map(|row| {
                    row.iter()
                        .copied()
                        .filter(|item| keep(item.metric()))
                        .collect::<Vec<_>>()
                })
                .filter(|row| !row.is_empty())
                .collect::<Vec<_>>();
            return Self::from_explicit_rows(rows);
        }

        let items = self
            .rows
            .iter()
            .flat_map(|row| row.iter().copied())
            .filter(|item| keep(item.metric()))
            .collect::<Vec<_>>();

        let rows = split_even_rows_items(&items, self.rows.len().max(1));
        Self::from_rows(rows)
    }
}

pub fn all_metrics() -> &'static [MetricKind] {
    &[
        MetricKind::Cpu,
        MetricKind::Memory,
        MetricKind::Storage,
        MetricKind::Io,
        MetricKind::Net,
        MetricKind::Sys,
        MetricKind::Gfx,
        MetricKind::Ingress,
        MetricKind::Egress,
        MetricKind::Gpu,
        MetricKind::Vram,
    ]
}

pub fn parse_layout_spec(spec: &str) -> Result<Layout, String> {
    if spec.contains(';') {
        return Err("invalid row separator ';'; use ',' or a literal newline".to_owned());
    }

    let explicit_rows = spec.contains(',') || spec.contains('\n');
    let mut rows = spec
        .split(|ch| ch == ',' || ch == '\n')
        .filter_map(|row| {
            let trimmed = row.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        })
        .collect::<Vec<_>>();
    if rows.is_empty() {
        rows.push(spec.trim());
    }

    let mut flat = Vec::new();
    let mut flat_items = Vec::new();
    let mut hinted_rows = None;
    let mut parsed_rows = Vec::new();

    for row in rows {
        let mut parsed_row = Vec::new();
        for token in row.split_whitespace() {
            if let Some((count, items)) = parse_all_items(token)? {
                if explicit_rows {
                    return Err("all cannot be mixed with explicit row separators".to_owned());
                }
                hinted_rows = Some(count);
                for item in items {
                    push_unique(&mut flat, item.metric());
                    flat_items.push(item);
                    parsed_row.push(item);
                }
                continue;
            }

            let item = parse_layout_item(token)?;
            push_unique(&mut flat, item.metric());
            flat_items.push(item);
            parsed_row.push(item);
        }
        if !parsed_row.is_empty() {
            parsed_rows.push(parsed_row);
        }
    }

    if flat.is_empty() {
        return Err("layout must contain at least one metric".to_owned());
    }

    if explicit_rows {
        return Ok(Layout::from_explicit_rows(parsed_rows));
    }

    let rows = if let Some(row_count) = hinted_rows {
        split_even_rows_items(&flat_items, row_count.max(1))
    } else if flat_items.len() > 5 {
        flat_items.chunks(5).map(|row| row.to_vec()).collect()
    } else {
        vec![flat_items]
    };

    Ok(Layout::from_rows(rows))
}

fn push_unique(metrics: &mut Vec<MetricKind>, metric: MetricKind) {
    if !metrics.contains(&metric) {
        metrics.push(metric);
    }
}

fn parse_all_items(token: &str) -> Result<Option<(usize, Vec<LayoutItem>)>, String> {
    if token.starts_with("all/") || token.starts_with("all:") {
        return Err("invalid all syntax; use 'all' or write explicit rows".to_owned());
    }

    let view = if token == "all" {
        LayoutView::Default
    } else if let Some(view_token) = token.strip_prefix("all.") {
        if view_token.contains(':')
            || view_token.contains('/')
            || view_token.contains('+')
            || view_token.contains('-')
        {
            return Err("all does not support size or width constraints".to_owned());
        }
        LayoutView::parse(view_token).ok_or_else(|| format!("unknown metric view: {view_token}"))?
    } else {
        return Ok(None);
    };

    Ok(Some((
        2,
        all_metrics()
            .iter()
            .copied()
            .map(|metric| LayoutItem::new(metric, view, Some(1), 1))
            .collect(),
    )))
}

fn parse_layout_item(token: &str) -> Result<LayoutItem, String> {
    if token.contains('/') {
        return Err("invalid metric syntax: use ':size' only".to_owned());
    }
    let (head, min_width) = parse_positive_suffix(token, '-', "metric min width")?;
    let (head, max_width) = parse_positive_suffix(head, '+', "metric max width")?;

    let (head, basis) = match head.split_once(':') {
        Some((head, basis)) => {
            let parsed = basis
                .parse::<usize>()
                .map_err(|_| format!("invalid metric size: {basis}"))?;
            if parsed == 0 {
                return Err("metric sizes must be greater than zero".to_owned());
            }
            (head, Some(parsed))
        }
        None => (head, Some(1)),
    };

    let mut parts = head.split('.');
    let metric_token = parts.next().unwrap_or_default();
    let mut view = LayoutView::Default;
    let mut display = DisplayMode::Full;
    for suffix in parts {
        if let Some(parsed_view) = LayoutView::parse(suffix) {
            if view != LayoutView::Default {
                return Err(format!("duplicate metric view: {suffix}"));
            }
            view = parsed_view;
            continue;
        }
        if let Some(parsed_display) = DisplayMode::parse(suffix) {
            if display != DisplayMode::Full {
                return Err(format!("duplicate display mode: {suffix}"));
            }
            display = parsed_display;
            continue;
        }
        return Err(format!("unknown metric suffix: {suffix}"));
    }

    let metric =
        MetricKind::parse(metric_token).ok_or_else(|| format!("unknown metric: {metric_token}"))?;
    if let (Some(min_width), Some(max_width)) = (min_width, max_width) {
        if min_width > max_width {
            return Err("metric min width cannot exceed max width".to_owned());
        }
    }
    if let (Some(basis), Some(max_width)) = (basis, max_width) {
        if basis > max_width {
            return Err("metric size cannot exceed max width".to_owned());
        }
    }

    let grow = basis.unwrap_or(1);

    Ok(LayoutItem::with_constraints(
        metric, view, display, basis, grow, max_width, min_width,
    ))
}

fn parse_positive_suffix<'a>(
    token: &'a str,
    delimiter: char,
    label: &str,
) -> Result<(&'a str, Option<usize>), String> {
    match token.rsplit_once(delimiter) {
        Some((head, value)) => {
            let parsed = value
                .parse::<usize>()
                .map_err(|_| format!("invalid {label}: {value}"))?;
            if parsed == 0 {
                return Err(format!("{label}s must be greater than zero"));
            }
            Ok((head, Some(parsed)))
        }
        None => Ok((token, None)),
    }
}

pub fn split_even_width(total: usize, count: usize) -> Vec<usize> {
    if count == 0 {
        return Vec::new();
    }
    let base = total / count;
    let extra = total % count;
    (0..count)
        .map(|index| if index < extra { base + 1 } else { base })
        .collect()
}

fn split_even_rows(metrics: &[MetricKind], rows: usize) -> Vec<Vec<LayoutItem>> {
    let heights = split_even_width(metrics.len(), rows);
    let mut start = 0;

    heights
        .into_iter()
        .filter(|count| *count > 0)
        .map(|count| {
            let end = start + count;
            let row = metrics[start..end]
                .iter()
                .copied()
                .map(|metric| LayoutItem::new(metric, LayoutView::Default, Some(1), 1))
                .collect::<Vec<_>>();
            start = end;
            row
        })
        .collect()
}

fn split_even_rows_items(items: &[LayoutItem], rows: usize) -> Vec<Vec<LayoutItem>> {
    let heights = split_even_width(items.len(), rows);
    let mut start = 0;

    heights
        .into_iter()
        .filter(|count| *count > 0)
        .map(|count| {
            let end = start + count;
            let row = items[start..end].to_vec();
            start = end;
            row
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_layout() {
        let layout = parse_layout_spec("sys cpu gfx gpu vram mem net").unwrap();
        assert_eq!(
            layout.metrics(),
            &[
                MetricKind::Sys,
                MetricKind::Cpu,
                MetricKind::Gfx,
                MetricKind::Gpu,
                MetricKind::Vram,
                MetricKind::Memory,
                MetricKind::Net,
            ]
        );
        assert_eq!(layout.rows().len(), 2);
        assert_eq!(
            layout.rows()[0]
                .iter()
                .map(|item| item.metric())
                .collect::<Vec<_>>(),
            [
                MetricKind::Sys,
                MetricKind::Cpu,
                MetricKind::Gfx,
                MetricKind::Gpu,
                MetricKind::Vram,
            ]
        );
    }

    #[test]
    fn default_layout_matches_all() {
        assert_eq!(Layout::default(), parse_layout_spec("all").unwrap());
    }

    #[test]
    fn rejects_unknown_metrics() {
        let error = parse_layout_spec("cpu watts").unwrap_err();
        assert!(error.contains("unknown metric"));
    }

    #[test]
    fn splits_evenly_with_remainder() {
        assert_eq!(split_even_width(10, 3), vec![4, 3, 3]);
    }

    #[test]
    fn memory_pct_view_formats_as_percent() {
        assert_eq!(
            MetricKind::Memory.format_value(
                LayoutView::Pct,
                0.375,
                &crate::metrics::HeadlineValue::Memory {
                    used_bytes: 1536,
                    available_bytes: 2560,
                    total_bytes: 4096,
                },
            ),
            " 38%"
        );
    }

    #[test]
    fn ingress_values_are_humanized_as_rates() {
        assert_eq!(
            MetricKind::Ingress.format_value(
                LayoutView::Default,
                0.0,
                &crate::metrics::HeadlineValue::Scalar(1536.0),
            ),
            "1.5K"
        );
    }

    #[test]
    fn net_values_are_humanized_as_rates() {
        assert_eq!(
            MetricKind::Net.format_value(
                LayoutView::Default,
                0.0,
                &crate::metrics::HeadlineValue::Scalar(3.0 * 1024.0 * 1024.0),
            ),
            "3.0M"
        );
    }

    #[test]
    fn storage_human_view_formats_used_over_total() {
        assert_eq!(
            MetricKind::Storage.format_value(
                LayoutView::Hum,
                0.5,
                &crate::metrics::HeadlineValue::Storage {
                    used_bytes: 512 * 1024 * 1024,
                    total_bytes: 2 * 1024 * 1024 * 1024,
                },
            ),
            "512M/2.0G"
        );
    }

    #[test]
    fn memory_human_view_formats_used_over_total() {
        assert_eq!(
            MetricKind::Memory.format_value(
                LayoutView::Hum,
                0.5,
                &crate::metrics::HeadlineValue::Memory {
                    used_bytes: 1536 * 1024 * 1024,
                    available_bytes: 512 * 1024 * 1024,
                    total_bytes: 2 * 1024 * 1024 * 1024,
                },
            ),
            "1.5G/2.0G"
        );
    }

    #[test]
    fn memory_and_gpu_use_new_labels() {
        assert_eq!(MetricKind::Memory.short_label(), "ram");
        assert_eq!(MetricKind::Storage.short_label(), "spc");
        assert_eq!(MetricKind::Rnd.short_label(), "rnd");
        assert_eq!(MetricKind::Sys.short_label(), "sys");
        assert_eq!(MetricKind::Gpu.short_label(), "gpu");
        assert_eq!(MetricKind::Vram.short_label(), "vram");
        assert_eq!(MetricKind::Gfx.short_label(), "gfx");
    }

    #[test]
    fn parses_known_views_and_rejects_unknown_ones() {
        assert_eq!(LayoutView::parse("pct"), Some(LayoutView::Pct));
        assert_eq!(LayoutView::parse("hum"), Some(LayoutView::Hum));
        assert_eq!(LayoutView::parse("free"), Some(LayoutView::Free));
        assert_eq!(LayoutView::parse("wat"), None);
    }

    #[test]
    fn default_views_match_metric_families() {
        assert_eq!(MetricKind::Cpu.default_view(), LayoutView::Pct);
        assert_eq!(MetricKind::Rnd.default_view(), LayoutView::Pct);
        assert_eq!(MetricKind::Memory.default_view(), LayoutView::Free);
        assert_eq!(MetricKind::Storage.default_view(), LayoutView::Free);
        assert_eq!(MetricKind::Net.default_view(), LayoutView::Hum);
        assert_eq!(MetricKind::Ingress.default_view(), LayoutView::Hum);
    }

    #[test]
    fn humanize_bytes_uses_expected_units() {
        assert_eq!(humanize_bytes(999.0), "999B");
        assert_eq!(humanize_bytes(1024.0), "1.0K");
        assert_eq!(humanize_bytes(12.0 * 1024.0 * 1024.0), " 12M");
        assert_eq!(humanize_bytes(700.0 * 1024.0 * 1024.0), "700M");
        assert_eq!(humanize_bytes(1006.0 * 1024.0 * 1024.0 * 1024.0), "1.0T");
    }

    #[test]
    fn rnd_metric_parses_and_formats_as_both_percent_and_bytes() {
        let layout = parse_layout_spec("rnd rnd.hum rnd.free").unwrap();
        assert_eq!(layout.metrics(), &[MetricKind::Rnd]);
        assert_eq!(layout.rows()[0].len(), 3);
        assert_eq!(
            MetricKind::Rnd.format_value(
                LayoutView::Pct,
                0.42,
                &crate::metrics::HeadlineValue::Scalar(2048.0)
            ),
            " 42%"
        );
        assert_eq!(
            MetricKind::Rnd.format_value(
                LayoutView::Hum,
                0.42,
                &crate::metrics::HeadlineValue::Scalar(2048.0)
            ),
            "2.0K"
        );
        assert_eq!(
            MetricKind::Rnd.format_value(
                LayoutView::Free,
                0.42,
                &crate::metrics::HeadlineValue::Scalar(2048.0)
            ),
            "2.0K"
        );
    }

    #[test]
    fn all_expands_to_the_full_metric_set() {
        let layout = parse_layout_spec("all").unwrap();
        assert_eq!(layout.metrics(), all_metrics());
        assert_eq!(layout.rows().len(), 2);
        assert_eq!(layout.rows()[0].len(), 6);
        assert_eq!(layout.rows()[1].len(), 5);
    }

    #[test]
    fn all_view_expands_to_the_full_metric_set_with_that_view() {
        let layout = parse_layout_spec("all.pct").unwrap();
        assert_eq!(layout.metrics(), all_metrics());
        assert_eq!(layout.rows().len(), 2);
        assert!(layout
            .rows()
            .iter()
            .flat_map(|row| row.iter())
            .all(|item| item.view() == LayoutView::Pct));
    }

    #[test]
    fn all_macro_conformance_matrix() {
        let cases = [
            (
                "all",
                vec![
                    MetricKind::Cpu,
                    MetricKind::Memory,
                    MetricKind::Storage,
                    MetricKind::Io,
                    MetricKind::Net,
                    MetricKind::Sys,
                    MetricKind::Gfx,
                    MetricKind::Ingress,
                    MetricKind::Egress,
                    MetricKind::Gpu,
                    MetricKind::Vram,
                ],
                vec![6, 5],
            ),
            (
                "cpu all ram",
                vec![
                    MetricKind::Cpu,
                    MetricKind::Memory,
                    MetricKind::Storage,
                    MetricKind::Io,
                    MetricKind::Net,
                    MetricKind::Sys,
                    MetricKind::Gfx,
                    MetricKind::Ingress,
                    MetricKind::Egress,
                    MetricKind::Gpu,
                    MetricKind::Vram,
                ],
                vec![7, 6],
            ),
            (
                "all cpu ram net cpu",
                vec![
                    MetricKind::Cpu,
                    MetricKind::Memory,
                    MetricKind::Storage,
                    MetricKind::Io,
                    MetricKind::Net,
                    MetricKind::Sys,
                    MetricKind::Gfx,
                    MetricKind::Ingress,
                    MetricKind::Egress,
                    MetricKind::Gpu,
                    MetricKind::Vram,
                ],
                vec![8, 7],
            ),
        ];

        for (spec, expected_metrics, expected_row_lengths) in cases {
            let layout = parse_layout_spec(spec).unwrap();
            let row_lengths = layout.rows().iter().map(Vec::len).collect::<Vec<_>>();

            assert_eq!(layout.metrics(), expected_metrics.as_slice(), "{spec}");
            assert_eq!(row_lengths, expected_row_lengths, "{spec}");
        }
    }

    #[test]
    fn all_and_explicit_metrics_preserve_duplicates_in_rows() {
        let layout = parse_layout_spec("cpu all ram net cpu").unwrap();
        assert_eq!(
            layout.metrics(),
            &[
                MetricKind::Cpu,
                MetricKind::Memory,
                MetricKind::Storage,
                MetricKind::Io,
                MetricKind::Net,
                MetricKind::Sys,
                MetricKind::Gfx,
                MetricKind::Ingress,
                MetricKind::Egress,
                MetricKind::Gpu,
                MetricKind::Vram,
            ]
        );
        assert_eq!(layout.rows().len(), 2);
        assert_eq!(layout.rows()[0].len(), 8);
        assert_eq!(layout.rows()[1].len(), 7);
    }

    #[test]
    fn all_row_count_suffix_is_rejected() {
        let error = parse_layout_spec("all/3").unwrap_err();
        assert!(error.contains("invalid all syntax"));
    }

    #[test]
    fn all_view_with_constraints_is_rejected() {
        let error = parse_layout_spec("all.pct/3").unwrap_err();
        assert!(error.contains("does not support size or width constraints"));
    }

    #[test]
    fn explicit_row_breaks_are_preserved() {
        let layout = parse_layout_spec("cpu ram, gpu vram\nnet io").unwrap();
        assert_eq!(
            layout
                .rows()
                .iter()
                .map(|row| row.iter().map(|item| item.metric()).collect::<Vec<_>>())
                .collect::<Vec<_>>(),
            &[
                vec![MetricKind::Cpu, MetricKind::Memory],
                vec![MetricKind::Gpu, MetricKind::Vram],
                vec![MetricKind::Net, MetricKind::Io],
            ]
        );
    }

    #[test]
    fn explicit_row_breaks_preserve_duplicates() {
        let layout = parse_layout_spec("cpu ram, ram gpu cpu").unwrap();
        assert_eq!(
            layout
                .rows()
                .iter()
                .map(|row| row.iter().map(|item| item.metric()).collect::<Vec<_>>())
                .collect::<Vec<_>>(),
            &[
                vec![MetricKind::Cpu, MetricKind::Memory],
                vec![MetricKind::Memory, MetricKind::Gpu, MetricKind::Cpu],
            ]
        );
    }

    #[test]
    fn all_cannot_mix_with_explicit_rows() {
        let error = parse_layout_spec("all, cpu").unwrap_err();
        assert!(error.contains("explicit row separators"));
    }

    #[test]
    fn can_filter_unavailable_metrics_from_layout() {
        let layout = parse_layout_spec("sys gfx io net, cpu gpu vram").unwrap();
        let filtered = layout.retain_available(|metric| metric != MetricKind::Vram);

        assert_eq!(
            filtered
                .rows()
                .iter()
                .map(|row| row.iter().map(|item| item.metric()).collect::<Vec<_>>())
                .collect::<Vec<_>>(),
            &[
                vec![
                    MetricKind::Sys,
                    MetricKind::Gfx,
                    MetricKind::Io,
                    MetricKind::Net
                ],
                vec![MetricKind::Cpu, MetricKind::Gpu],
            ]
        );
    }

    #[test]
    fn retain_available_preserves_explicit_rows() {
        let layout = parse_layout_spec("cpu ram spc, io").unwrap();
        let filtered = layout.retain_available(|metric| metric != MetricKind::Memory);

        assert_eq!(
            filtered
                .rows()
                .iter()
                .map(|row| row.iter().map(|item| item.metric()).collect::<Vec<_>>())
                .collect::<Vec<_>>(),
            &[
                vec![MetricKind::Cpu, MetricKind::Storage],
                vec![MetricKind::Io],
            ]
        );
    }

    #[test]
    fn parses_size_and_view_tokens() {
        let layout = parse_layout_spec("cpu:12 ram.pct:10 net.hum").unwrap();
        assert_eq!(layout.rows().len(), 1);
        assert_eq!(layout.rows()[0][0].metric(), MetricKind::Cpu);
        assert_eq!(layout.rows()[0][0].basis(), Some(12));
        assert_eq!(layout.rows()[0][0].grow(), 12);
        assert_eq!(layout.rows()[0][1].metric(), MetricKind::Memory);
        assert_eq!(layout.rows()[0][1].view(), LayoutView::Pct);
        assert_eq!(layout.rows()[0][1].basis(), Some(10));
        assert_eq!(layout.rows()[0][1].grow(), 10);
        assert_eq!(layout.rows()[0][2].metric(), MetricKind::Net);
        assert_eq!(layout.rows()[0][2].view(), LayoutView::Hum);
        assert_eq!(layout.rows()[0][2].basis(), Some(1));
        assert_eq!(layout.rows()[0][2].grow(), 1);
    }

    #[test]
    fn mixed_item_syntax_conformance_matrix() {
        let cases = vec![
            (
                "cpu:12 ram:10 net.hum+24",
                vec![vec![
                    (
                        MetricKind::Cpu,
                        LayoutView::Default,
                        Some(12),
                        12,
                        None,
                        None,
                    ),
                    (
                        MetricKind::Memory,
                        LayoutView::Default,
                        Some(10),
                        10,
                        None,
                        None,
                    ),
                    (MetricKind::Net, LayoutView::Hum, Some(1), 1, Some(24), None),
                ]],
            ),
            (
                "cpu:12+20-8 ram+14-6, spc.hum-9",
                vec![
                    vec![
                        (
                            MetricKind::Cpu,
                            LayoutView::Default,
                            Some(12),
                            12,
                            Some(20),
                            Some(8),
                        ),
                        (
                            MetricKind::Memory,
                            LayoutView::Default,
                            Some(1),
                            1,
                            Some(14),
                            Some(6),
                        ),
                    ],
                    vec![(
                        MetricKind::Storage,
                        LayoutView::Hum,
                        Some(1),
                        1,
                        None,
                        Some(9),
                    )],
                ],
            ),
            (
                "ram.free spc.pct spc.hum spc.free net.pct net.hum",
                vec![
                    vec![
                        (MetricKind::Memory, LayoutView::Free, Some(1), 1, None, None),
                        (MetricKind::Storage, LayoutView::Pct, Some(1), 1, None, None),
                        (MetricKind::Storage, LayoutView::Hum, Some(1), 1, None, None),
                        (
                            MetricKind::Storage,
                            LayoutView::Free,
                            Some(1),
                            1,
                            None,
                            None,
                        ),
                        (MetricKind::Net, LayoutView::Pct, Some(1), 1, None, None),
                    ],
                    vec![(MetricKind::Net, LayoutView::Hum, Some(1), 1, None, None)],
                ],
            ),
        ];

        for (spec, expected_rows) in cases {
            let layout = parse_layout_spec(spec).unwrap();
            let actual_rows = layout
                .rows()
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|item| {
                            (
                                item.metric(),
                                item.view(),
                                item.basis(),
                                item.grow(),
                                item.max_width(),
                                item.min_width(),
                            )
                        })
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();

            assert_eq!(actual_rows, expected_rows, "{spec}");
        }
    }

    #[test]
    fn parses_max_and_min_constraints() {
        let layout = parse_layout_spec("cpu:12+20-8 ram+14-6").unwrap();
        assert_eq!(layout.rows()[0][0].grow(), 12);
        assert_eq!(layout.rows()[0][0].max_width(), Some(20));
        assert_eq!(layout.rows()[0][0].min_width(), Some(8));
        assert_eq!(layout.rows()[0][1].grow(), 1);
        assert_eq!(layout.rows()[0][1].max_width(), Some(14));
        assert_eq!(layout.rows()[0][1].min_width(), Some(6));
    }

    #[test]
    fn rejects_old_row_separator() {
        let error = parse_layout_spec("cpu; ram").unwrap_err();
        assert!(error.contains("invalid row separator"));
    }

    #[test]
    fn rejects_grow_syntax() {
        let error = parse_layout_spec("cpu:12/3").unwrap_err();
        assert!(error.contains("invalid metric syntax"));
        let error = parse_layout_spec("cpu*3").unwrap_err();
        assert!(error.contains("unknown metric"));
    }

    #[test]
    fn rejects_invalid_constraint_ranges() {
        let error = parse_layout_spec("cpu:12+10").unwrap_err();
        assert!(error.contains("size cannot exceed max"));
        let error = parse_layout_spec("cpu+10-12").unwrap_err();
        assert!(error.contains("min width cannot exceed max"));
    }

    #[test]
    fn same_metric_with_different_views_can_coexist() {
        let layout = parse_layout_spec("spc.pct spc.hum spc.free").unwrap();
        assert_eq!(layout.rows().len(), 1);
        assert_eq!(layout.rows()[0].len(), 3);
        assert_eq!(layout.rows()[0][0].metric(), MetricKind::Storage);
        assert_eq!(layout.rows()[0][0].view(), LayoutView::Pct);
        assert_eq!(layout.rows()[0][1].metric(), MetricKind::Storage);
        assert_eq!(layout.rows()[0][1].view(), LayoutView::Hum);
        assert_eq!(layout.rows()[0][2].metric(), MetricKind::Storage);
        assert_eq!(layout.rows()[0][2].view(), LayoutView::Free);
        assert_eq!(layout.metrics(), &[MetricKind::Storage]);
    }

    #[test]
    fn memory_free_view_formats_available_bytes() {
        assert_eq!(
            MetricKind::Memory.format_value(
                LayoutView::Free,
                0.5,
                &crate::metrics::HeadlineValue::Memory {
                    used_bytes: 1536 * 1024 * 1024,
                    available_bytes: 512 * 1024 * 1024,
                    total_bytes: 2 * 1024 * 1024 * 1024,
                },
            ),
            "512M"
        );
    }

    #[test]
    fn exact_duplicate_items_are_preserved() {
        let layout = parse_layout_spec("spc.hum spc.hum").unwrap();
        assert_eq!(layout.rows().len(), 1);
        assert_eq!(layout.rows()[0].len(), 2);
        assert_eq!(layout.metrics(), &[MetricKind::Storage]);
    }

    #[test]
    fn explicit_view_override_matrix() {
        let cases = [
            (
                MetricKind::Net,
                LayoutView::Pct,
                crate::metrics::HeadlineValue::Scalar(3.0 * 1024.0 * 1024.0),
                " 50%",
            ),
            (
                MetricKind::Net,
                LayoutView::Hum,
                crate::metrics::HeadlineValue::Scalar(3.0 * 1024.0 * 1024.0),
                "3.0M",
            ),
            (
                MetricKind::Storage,
                LayoutView::Pct,
                crate::metrics::HeadlineValue::Storage {
                    used_bytes: 512 * 1024 * 1024,
                    total_bytes: 2 * 1024 * 1024 * 1024,
                },
                " 50%",
            ),
            (
                MetricKind::Storage,
                LayoutView::Hum,
                crate::metrics::HeadlineValue::Storage {
                    used_bytes: 512 * 1024 * 1024,
                    total_bytes: 2 * 1024 * 1024 * 1024,
                },
                "512M/2.0G",
            ),
            (
                MetricKind::Storage,
                LayoutView::Free,
                crate::metrics::HeadlineValue::Storage {
                    used_bytes: 512 * 1024 * 1024,
                    total_bytes: 2 * 1024 * 1024 * 1024,
                },
                "1.5G",
            ),
            (
                MetricKind::Memory,
                LayoutView::Free,
                crate::metrics::HeadlineValue::Memory {
                    used_bytes: 512 * 1024 * 1024,
                    available_bytes: 1536 * 1024 * 1024,
                    total_bytes: 2 * 1024 * 1024 * 1024,
                },
                "1.5G",
            ),
            (
                MetricKind::Memory,
                LayoutView::Hum,
                crate::metrics::HeadlineValue::Memory {
                    used_bytes: 512 * 1024 * 1024,
                    available_bytes: 1536 * 1024 * 1024,
                    total_bytes: 2 * 1024 * 1024 * 1024,
                },
                "512M/2.0G",
            ),
        ];

        for (metric, view, headline, expected) in cases {
            assert_eq!(
                metric.format_value(view, 0.5, &headline),
                expected,
                "{metric:?} {view:?}"
            );
        }
    }
}
