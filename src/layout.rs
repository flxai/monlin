#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MetricKind {
    Cpu,
    Sys,
    Gpu,
    Vram,
    Gfx,
    Memory,
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
}

impl LayoutView {
    pub fn parse(token: &str) -> Option<Self> {
        match token {
            "pct" => Some(Self::Pct),
            "hum" => Some(Self::Hum),
            _ => None,
        }
    }
}

impl MetricKind {
    pub fn parse(token: &str) -> Option<Self> {
        match token {
            "cpu" => Some(Self::Cpu),
            "sys" => Some(Self::Sys),
            "gpu" => Some(Self::Gpu),
            "vram" => Some(Self::Vram),
            "gfx" => Some(Self::Gfx),
            "memory" | "mem" | "ram" => Some(Self::Memory),
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
            Self::Sys => "sys",
            Self::Gpu => "gpu",
            Self::Vram => "vram",
            Self::Gfx => "gfx",
            Self::Memory => "ram",
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
            Self::Cpu | Self::Sys | Self::Gpu | Self::Vram | Self::Gfx | Self::Memory => {
                LayoutView::Pct
            }
        }
    }

    pub fn format_value(self, view: LayoutView, normalized: f64, headline: f64) -> String {
        let resolved = match view {
            LayoutView::Default => self.default_view(),
            other => other,
        };

        match resolved {
            LayoutView::Pct => format_percent(self, normalized),
            LayoutView::Hum => match self {
                Self::Io | Self::Net | Self::Ingress | Self::Egress => humanize_rate(headline),
                _ => format_percent(self, normalized),
            },
            LayoutView::Default => unreachable!(),
        }
    }
}

fn format_percent(metric: MetricKind, value: f64) -> String {
    match metric {
        MetricKind::Vram => format!("{:.0}%", value.clamp(0.0, 1.0) * 100.0),
        MetricKind::Cpu
        | MetricKind::Sys
        | MetricKind::Gpu
        | MetricKind::Gfx
        | MetricKind::Memory
        | MetricKind::Io
        | MetricKind::Net
        | MetricKind::Ingress
        | MetricKind::Egress => format!("{:>3.0}%", value.clamp(0.0, 1.0) * 100.0),
    }
}

fn humanize_rate(bytes_per_second: f64) -> String {
    const UNITS: [&str; 5] = ["B/s", "K/s", "M/s", "G/s", "T/s"];

    let mut value = bytes_per_second.max(0.0);
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        return format!("{:.0}{}", value, UNITS[unit]);
    }
    if value >= 10.0 {
        return format!("{:.0}{}", value, UNITS[unit]);
    }

    format!("{:.1}{}", value, UNITS[unit])
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LayoutItem {
    metric: MetricKind,
    view: LayoutView,
    basis: Option<usize>,
    grow: usize,
}

impl LayoutItem {
    pub fn new(metric: MetricKind, view: LayoutView, basis: Option<usize>, grow: usize) -> Self {
        Self {
            metric,
            view,
            basis,
            grow,
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

    pub fn grow(&self) -> usize {
        self.grow
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Layout {
    rows: Vec<Vec<LayoutItem>>,
    metrics: Vec<MetricKind>,
}

impl Default for Layout {
    fn default() -> Self {
        Self::from_rows(split_even_rows(all_metrics(), 2))
    }
}

impl Layout {
    pub fn from_rows(rows: Vec<Vec<LayoutItem>>) -> Self {
        let mut metrics = Vec::new();
        for row in &rows {
            for item in row {
                push_unique(&mut metrics, item.metric());
            }
        }

        Self { rows, metrics }
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

        Self::from_rows(rows)
    }
}

pub fn all_metrics() -> &'static [MetricKind] {
    &[
        MetricKind::Sys,
        MetricKind::Cpu,
        MetricKind::Memory,
        MetricKind::Gfx,
        MetricKind::Gpu,
        MetricKind::Vram,
        MetricKind::Io,
        MetricKind::Net,
        MetricKind::Ingress,
        MetricKind::Egress,
    ]
}

pub fn parse_layout_spec(spec: &str) -> Result<Layout, String> {
    let explicit_rows = spec.contains(';') || spec.contains('\n');
    let mut rows = spec
        .split(|ch| ch == ';' || ch == '\n')
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
            if let Some(count) = parse_all_rows(token)? {
                if explicit_rows {
                    return Err("all[/N] cannot be mixed with explicit row separators".to_owned());
                }
                hinted_rows = Some(count);
                for metric in all_metrics() {
                    push_unique(&mut flat, *metric);
                    let item = LayoutItem::new(*metric, LayoutView::Default, None, 1);
                    push_unique_item(&mut flat_items, item);
                    push_unique_item(&mut parsed_row, item);
                }
                continue;
            }

            let item = parse_layout_item(token)?;
            push_unique(&mut flat, item.metric());
            push_unique_item(&mut flat_items, item);
            push_unique_item(&mut parsed_row, item);
        }
        if !parsed_row.is_empty() {
            parsed_rows.push(parsed_row);
        }
    }

    if flat.is_empty() {
        return Err("layout must contain at least one metric".to_owned());
    }

    if explicit_rows {
        let mut seen = Vec::new();
        let rows = parsed_rows
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .filter(|item| {
                        if seen.contains(&item.metric()) {
                            false
                        } else {
                            seen.push(item.metric());
                            true
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .filter(|row| !row.is_empty())
            .collect::<Vec<_>>();
        return Ok(Layout::from_rows(rows));
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

fn push_unique_item(items: &mut Vec<LayoutItem>, item: LayoutItem) {
    if !items.iter().any(|existing| existing.metric() == item.metric()) {
        items.push(item);
    }
}

fn parse_all_rows(token: &str) -> Result<Option<usize>, String> {
    if token == "all" {
        return Ok(Some(2));
    }
    if let Some(value) = token.strip_prefix("all/").or_else(|| token.strip_prefix("all:")) {
        let rows = value
            .parse::<usize>()
            .map_err(|_| format!("invalid all row count: {value}"))?;
        if rows == 0 {
            return Err("all/N requires N > 0".to_owned());
        }
        return Ok(Some(rows));
    }

    Ok(None)
}

fn parse_layout_item(token: &str) -> Result<LayoutItem, String> {
    let (head, grow) = match token.split_once('+').or_else(|| token.split_once('*')) {
        Some((head, grow)) => {
            let parsed = grow
                .parse::<usize>()
                .map_err(|_| format!("invalid metric grow value: {grow}"))?;
            if parsed == 0 {
                return Err("metric grow values must be greater than zero".to_owned());
            }
            (head, Some(parsed))
        }
        None => (token, None),
    };

    let (head, basis) = match head.split_once(':') {
        Some((head, basis)) => {
            let parsed = basis
                .parse::<usize>()
                .map_err(|_| format!("invalid metric basis: {basis}"))?;
            if parsed == 0 {
                return Err("metric bases must be greater than zero".to_owned());
            }
            (head, Some(parsed))
        }
        None => (head, None),
    };

    let (metric_token, view) = match head.split_once('.') {
        Some((metric, view)) => (
            metric,
            LayoutView::parse(view).ok_or_else(|| format!("unknown metric view: {view}"))?,
        ),
        None => (head, LayoutView::Default),
    };

    let metric =
        MetricKind::parse(metric_token).ok_or_else(|| format!("unknown metric: {metric_token}"))?;
    let grow = match (basis, grow) {
        (_, Some(grow)) => grow,
        (Some(_), None) => 0,
        (None, None) => 1,
    };

    Ok(LayoutItem::new(metric, view, basis, grow))
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
                .map(|metric| LayoutItem::new(metric, LayoutView::Default, None, 1))
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
    fn memory_values_are_formatted_as_percent() {
        assert_eq!(
            MetricKind::Memory.format_value(LayoutView::Default, 0.375, 0.375),
            " 38%"
        );
    }

    #[test]
    fn ingress_values_are_humanized_as_rates() {
        assert_eq!(
            MetricKind::Ingress.format_value(LayoutView::Default, 0.0, 1536.0),
            "1.5K/s"
        );
    }

    #[test]
    fn net_values_are_humanized_as_rates() {
        assert_eq!(
            MetricKind::Net.format_value(
                LayoutView::Default,
                0.0,
                3.0 * 1024.0 * 1024.0
            ),
            "3.0M/s"
        );
    }

    #[test]
    fn memory_and_gpu_use_new_labels() {
        assert_eq!(MetricKind::Memory.short_label(), "ram");
        assert_eq!(MetricKind::Sys.short_label(), "sys");
        assert_eq!(MetricKind::Gpu.short_label(), "gpu");
        assert_eq!(MetricKind::Vram.short_label(), "vram");
        assert_eq!(MetricKind::Gfx.short_label(), "gfx");
    }

    #[test]
    fn all_expands_to_the_full_metric_set() {
        let layout = parse_layout_spec("all").unwrap();
        assert_eq!(layout.metrics(), all_metrics());
        assert_eq!(layout.rows().len(), 2);
        assert_eq!(layout.rows()[0].len(), 5);
        assert_eq!(layout.rows()[1].len(), 5);
    }

    #[test]
    fn all_and_explicit_metrics_are_deduped() {
        let layout = parse_layout_spec("cpu all ram net cpu").unwrap();
        assert_eq!(
            layout.metrics(),
            &[
                MetricKind::Cpu,
                MetricKind::Sys,
                MetricKind::Memory,
                MetricKind::Gfx,
                MetricKind::Gpu,
                MetricKind::Vram,
                MetricKind::Io,
                MetricKind::Net,
                MetricKind::Ingress,
                MetricKind::Egress,
            ]
        );
        assert_eq!(layout.rows().len(), 2);
    }

    #[test]
    fn all_with_explicit_row_count_is_balanced() {
        let layout = parse_layout_spec("all/3").unwrap();
        assert_eq!(layout.rows().len(), 3);
        assert_eq!(layout.rows()[0].len(), 4);
        assert_eq!(layout.rows()[1].len(), 3);
        assert_eq!(layout.rows()[2].len(), 3);
    }

    #[test]
    fn explicit_row_breaks_are_preserved() {
        let layout = parse_layout_spec("cpu ram; gpu vram\nnet io").unwrap();
        assert_eq!(
            layout.rows()
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
    fn explicit_row_breaks_dedupe_globally() {
        let layout = parse_layout_spec("cpu ram; ram gpu cpu").unwrap();
        assert_eq!(
            layout.rows()
                .iter()
                .map(|row| row.iter().map(|item| item.metric()).collect::<Vec<_>>())
                .collect::<Vec<_>>(),
            &[
                vec![MetricKind::Cpu, MetricKind::Memory],
                vec![MetricKind::Gpu],
            ]
        );
    }

    #[test]
    fn all_row_hint_cannot_mix_with_explicit_rows() {
        let error = parse_layout_spec("all/2; cpu").unwrap_err();
        assert!(error.contains("explicit row separators"));
    }

    #[test]
    fn can_filter_unavailable_metrics_from_layout() {
        let layout = parse_layout_spec("sys gfx io net; cpu gpu vram").unwrap();
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
                    MetricKind::Net,
                ],
                vec![MetricKind::Cpu, MetricKind::Gpu],
            ]
        );
    }

    #[test]
    fn parses_basis_grow_and_view_tokens() {
        let layout = parse_layout_spec("cpu:12+2 ram.pct:10 net.hum+3").unwrap();
        assert_eq!(layout.rows().len(), 1);
        assert_eq!(layout.rows()[0][0].metric(), MetricKind::Cpu);
        assert_eq!(layout.rows()[0][0].basis(), Some(12));
        assert_eq!(layout.rows()[0][0].grow(), 2);
        assert_eq!(layout.rows()[0][1].metric(), MetricKind::Memory);
        assert_eq!(layout.rows()[0][1].view(), LayoutView::Pct);
        assert_eq!(layout.rows()[0][1].basis(), Some(10));
        assert_eq!(layout.rows()[0][1].grow(), 0);
        assert_eq!(layout.rows()[0][2].metric(), MetricKind::Net);
        assert_eq!(layout.rows()[0][2].view(), LayoutView::Hum);
        assert_eq!(layout.rows()[0][2].grow(), 3);
    }

    #[test]
    fn legacy_weight_syntax_still_parses_as_grow() {
        let layout = parse_layout_spec("cpu*3 ram*4").unwrap();
        assert_eq!(layout.rows()[0][0].grow(), 3);
        assert_eq!(layout.rows()[0][1].grow(), 4);
    }
}
