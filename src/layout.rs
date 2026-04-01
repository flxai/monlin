#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MetricKind {
    Cpu,
    Gpu,
    Memory,
    Io,
    Ingress,
    Egress,
}

impl MetricKind {
    pub fn parse(token: &str) -> Option<Self> {
        match token {
            "cpu" => Some(Self::Cpu),
            "gpu" => Some(Self::Gpu),
            "memory" | "mem" => Some(Self::Memory),
            "io" => Some(Self::Io),
            "ingress" | "in" => Some(Self::Ingress),
            "egress" | "out" => Some(Self::Egress),
            _ => None,
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Gpu => "gpu",
            Self::Memory => "mem",
            Self::Io => "io",
            Self::Ingress => "in",
            Self::Egress => "out",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Layout {
    metrics: Vec<MetricKind>,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            metrics: vec![MetricKind::Cpu],
        }
    }
}

impl Layout {
    pub fn metrics(&self) -> &[MetricKind] {
        &self.metrics
    }
}

pub fn parse_layout_spec(spec: &str) -> Result<Layout, String> {
    let metrics = spec
        .split_whitespace()
        .map(|token| MetricKind::parse(token).ok_or_else(|| format!("unknown metric: {token}")))
        .collect::<Result<Vec<_>, _>>()?;

    if metrics.is_empty() {
        return Err("layout must contain at least one metric".to_owned());
    }

    Ok(Layout { metrics })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_layout() {
        let layout = parse_layout_spec("cpu gpu mem").unwrap();
        assert_eq!(
            layout.metrics(),
            &[MetricKind::Cpu, MetricKind::Gpu, MetricKind::Memory]
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
}
