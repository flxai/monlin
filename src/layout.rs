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

    pub fn format_value(self, value: f64) -> String {
        match self {
            Self::Vram => format!("{:.0}%", value.clamp(0.0, 1.0) * 100.0),
            Self::Cpu
            | Self::Sys
            | Self::Gpu
            | Self::Gfx
            | Self::Memory
            | Self::Io
            | Self::Net
            | Self::Ingress
            | Self::Egress => format!("{:>3.0}%", value.clamp(0.0, 1.0) * 100.0),
        }
    }

    pub fn is_split(self) -> bool {
        matches!(self, Self::Sys | Self::Gfx | Self::Io | Self::Net)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Layout {
    rows: Vec<Vec<MetricKind>>,
    metrics: Vec<MetricKind>,
}

impl Default for Layout {
    fn default() -> Self {
        Self::from_rows(vec![vec![MetricKind::Cpu]])
    }
}

impl Layout {
    pub fn from_rows(rows: Vec<Vec<MetricKind>>) -> Self {
        let mut metrics = Vec::new();
        for row in &rows {
            for metric in row {
                push_unique(&mut metrics, *metric);
            }
        }

        Self { rows, metrics }
    }

    pub fn metrics(&self) -> &[MetricKind] {
        &self.metrics
    }

    pub fn rows(&self) -> &[Vec<MetricKind>] {
        &self.rows
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
    let mut hinted_rows = None;
    let mut parsed_rows = Vec::new();

    for row in rows {
        let mut parsed_row = Vec::new();
        for token in row.split_whitespace() {
            if let Some(count) = parse_all_rows(token)? {
                if explicit_rows {
                    return Err("all[:N] cannot be mixed with explicit row separators".to_owned());
                }
                hinted_rows = Some(count);
                for metric in all_metrics() {
                    push_unique(&mut flat, *metric);
                    push_unique(&mut parsed_row, *metric);
                }
                continue;
            }

            let metric =
                MetricKind::parse(token).ok_or_else(|| format!("unknown metric: {token}"))?;
            push_unique(&mut flat, metric);
            push_unique(&mut parsed_row, metric);
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
                    .filter(|metric| {
                        if seen.contains(metric) {
                            false
                        } else {
                            seen.push(*metric);
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
        split_even_rows(&flat, row_count.max(1))
    } else if flat.len() > 5 {
        flat.chunks(5).map(|row| row.to_vec()).collect()
    } else {
        vec![flat]
    };

    Ok(Layout::from_rows(rows))
}

fn push_unique(metrics: &mut Vec<MetricKind>, metric: MetricKind) {
    if !metrics.contains(&metric) {
        metrics.push(metric);
    }
}

fn parse_all_rows(token: &str) -> Result<Option<usize>, String> {
    if token == "all" {
        return Ok(Some(2));
    }
    if let Some(value) = token.strip_prefix("all:") {
        let rows = value
            .parse::<usize>()
            .map_err(|_| format!("invalid all row count: {value}"))?;
        if rows == 0 {
            return Err("all:N requires N > 0".to_owned());
        }
        return Ok(Some(rows));
    }

    Ok(None)
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

fn split_even_rows(metrics: &[MetricKind], rows: usize) -> Vec<Vec<MetricKind>> {
    let heights = split_even_width(metrics.len(), rows);
    let mut start = 0;

    heights
        .into_iter()
        .filter(|count| *count > 0)
        .map(|count| {
            let end = start + count;
            let row = metrics[start..end].to_vec();
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
            layout.rows()[0],
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
        assert_eq!(MetricKind::Memory.format_value(0.375), " 38%");
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
        let layout = parse_layout_spec("all:3").unwrap();
        assert_eq!(layout.rows().len(), 3);
        assert_eq!(layout.rows()[0].len(), 4);
        assert_eq!(layout.rows()[1].len(), 3);
        assert_eq!(layout.rows()[2].len(), 3);
    }

    #[test]
    fn explicit_row_breaks_are_preserved() {
        let layout = parse_layout_spec("cpu ram; gpu vram\nnet io").unwrap();
        assert_eq!(
            layout.rows(),
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
            layout.rows(),
            &[
                vec![MetricKind::Cpu, MetricKind::Memory],
                vec![MetricKind::Gpu],
            ]
        );
    }

    #[test]
    fn all_row_hint_cannot_mix_with_explicit_rows() {
        let error = parse_layout_spec("all:2; cpu").unwrap_err();
        assert!(error.contains("explicit row separators"));
    }
}
