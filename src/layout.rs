use std::path::PathBuf;

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
    In,
    Out,
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
            "vram" | "vrm" => Some(Self::Vram),
            "gfx" => Some(Self::Gfx),
            "memory" | "ram" => Some(Self::Memory),
            "storage" | "disk" | "space" | "spc" => Some(Self::Storage),
            "io" => Some(Self::Io),
            "net" => Some(Self::Net),
            "in" => Some(Self::In),
            "out" => Some(Self::Out),
            "rx" | "ingress" => Some(Self::Ingress),
            "tx" | "egress" => Some(Self::Egress),
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
            Self::In => "in",
            Self::Out => "out",
            Self::Ingress => "rx",
            Self::Egress => "tx",
        }
    }

    pub fn is_split(self) -> bool {
        matches!(self, Self::Sys | Self::Gfx | Self::Io | Self::Net)
    }

    pub fn default_view(self) -> LayoutView {
        match self {
            Self::Io | Self::Net | Self::In | Self::Out | Self::Ingress | Self::Egress => {
                LayoutView::Hum
            }
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
                Self::Io | Self::Net | Self::In | Self::Out | Self::Ingress | Self::Egress => {
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
        | MetricKind::In
        | MetricKind::Out
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

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Source {
    Metric(MetricKind),
    SplitMetric(MetricKind, MetricKind),
    StreamColumn(usize),
    File(PathBuf),
    Process(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Item {
    label: Option<String>,
    source: Source,
    view: LayoutView,
    display: DisplayMode,
    size: usize,
    max_width: Option<usize>,
    min_width: Option<usize>,
}

impl Item {
    pub fn metric(
        metric: MetricKind,
        view: LayoutView,
        display: DisplayMode,
        size: usize,
        max_width: Option<usize>,
        min_width: Option<usize>,
    ) -> Self {
        Self {
            label: None,
            source: Source::Metric(metric),
            view,
            display,
            size,
            max_width,
            min_width,
        }
    }

    pub fn stream_column(
        label: Option<String>,
        column_index: usize,
        display: DisplayMode,
        size: usize,
        max_width: Option<usize>,
        min_width: Option<usize>,
    ) -> Self {
        Self {
            label,
            source: Source::StreamColumn(column_index),
            view: LayoutView::Default,
            display,
            size,
            max_width,
            min_width,
        }
    }

    pub fn file(
        label: Option<String>,
        path: PathBuf,
        display: DisplayMode,
        size: usize,
        max_width: Option<usize>,
        min_width: Option<usize>,
    ) -> Self {
        Self {
            label,
            source: Source::File(path),
            view: LayoutView::Default,
            display,
            size,
            max_width,
            min_width,
        }
    }

    pub fn process(
        label: Option<String>,
        command: String,
        display: DisplayMode,
        size: usize,
        max_width: Option<usize>,
        min_width: Option<usize>,
    ) -> Self {
        Self {
            label,
            source: Source::Process(command),
            view: LayoutView::Default,
            display,
            size,
            max_width,
            min_width,
        }
    }

    pub fn split_metric(
        label: Option<String>,
        upper: MetricKind,
        lower: MetricKind,
        display: DisplayMode,
        size: usize,
        max_width: Option<usize>,
        min_width: Option<usize>,
    ) -> Self {
        Self {
            label,
            source: Source::SplitMetric(upper, lower),
            view: LayoutView::Default,
            display,
            size,
            max_width,
            min_width,
        }
    }

    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }

    pub fn source(&self) -> &Source {
        &self.source
    }

    pub fn view(&self) -> LayoutView {
        self.view
    }

    pub fn display(&self) -> DisplayMode {
        self.display
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn max_width(&self) -> Option<usize> {
        self.max_width
    }

    pub fn min_width(&self) -> Option<usize> {
        self.min_width
    }

    fn lower(&self) -> Result<LayoutItem, String> {
        match self.source {
            Source::Metric(metric) => Ok(LayoutItem::with_constraints(
                metric,
                self.view,
                self.display,
                Some(self.size),
                self.size,
                self.max_width,
                self.min_width,
            )),
            Source::StreamColumn(index) => Err(format!(
                "stream source @{} cannot yet be lowered into a native layout",
                index + 1
            )),
            Source::SplitMetric(upper, lower) => Err(format!(
                "split source {}+{} cannot yet be lowered into a native layout",
                upper.short_label(),
                lower.short_label()
            )),
            Source::File(ref path) => Err(format!(
                "file source {} cannot yet be lowered into a native layout",
                path.display()
            )),
            Source::Process(ref command) => Err(format!(
                "process source {command:?} cannot yet be lowered into a native layout"
            )),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Row {
    label: Option<String>,
    items: Vec<Item>,
}

impl Row {
    pub fn new(label: Option<String>, items: Vec<Item>) -> Self {
        Self { label, items }
    }

    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }

    pub fn items(&self) -> &[Item] {
        &self.items
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Document {
    rows: Vec<Row>,
    explicit_rows: bool,
    filter_available: bool,
}

impl Document {
    pub fn new(rows: Vec<Row>, explicit_rows: bool, filter_available: bool) -> Self {
        Self {
            rows,
            explicit_rows,
            filter_available,
        }
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub fn explicit_rows(&self) -> bool {
        self.explicit_rows
    }

    pub fn filter_available(&self) -> bool {
        self.filter_available
    }

    pub fn sources(&self) -> Vec<&Source> {
        let mut sources = Vec::new();
        for row in &self.rows {
            for item in &row.items {
                if !sources.iter().any(|source| *source == &item.source) {
                    sources.push(&item.source);
                }
            }
        }
        sources
    }

    pub fn uses_stream_columns(&self) -> bool {
        self.rows.iter().any(|row| {
            row.items
                .iter()
                .any(|item| matches!(item.source, Source::StreamColumn(_)))
        })
    }

    pub fn uses_external_sources(&self) -> bool {
        self.rows.iter().any(|row| {
            row.items.iter().any(|item| {
                matches!(item.source, Source::File(_) | Source::Process(_))
            })
        })
    }

    pub fn is_native_only(&self) -> bool {
        self.rows.iter().all(|row| {
            row.items
                .iter()
                .all(|item| matches!(item.source, Source::Metric(_) | Source::SplitMetric(_, _)))
        })
    }

    pub fn uses_split_metrics(&self) -> bool {
        self.rows.iter().any(|row| {
            row.items
                .iter()
                .any(|item| matches!(item.source, Source::SplitMetric(_, _)))
        })
    }

    pub fn has_row_labels(&self) -> bool {
        self.rows.iter().any(|row| row.label.is_some())
    }

    fn lower(&self) -> Result<Layout, String> {
        let rows = self
            .rows
            .iter()
            .map(|row| {
                row.items
                    .iter()
                    .map(Item::lower)
                    .collect::<Result<Vec<_>, _>>()
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(if self.explicit_rows {
            Layout::from_rows_with_mode(rows, true, self.filter_available)
        } else {
            Layout::from_rows_with_mode(rows, false, self.filter_available)
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Layout {
    rows: Vec<Vec<LayoutItem>>,
    metrics: Vec<MetricKind>,
    explicit_rows: bool,
    filter_available: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Self::from_rows_with_mode(default_avail_layout_rows(LayoutView::Default), true, true)
    }
}

impl Layout {
    pub fn from_rows(rows: Vec<Vec<LayoutItem>>) -> Self {
        Self::from_rows_with_mode(rows, false, false)
    }

    fn from_rows_with_mode(
        rows: Vec<Vec<LayoutItem>>,
        explicit_rows: bool,
        filter_available: bool,
    ) -> Self {
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
            filter_available,
        }
    }

    pub fn metrics(&self) -> &[MetricKind] {
        &self.metrics
    }

    pub fn rows(&self) -> &[Vec<LayoutItem>] {
        &self.rows
    }

    pub fn filter_available(&self) -> bool {
        self.filter_available
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
            return Self::from_rows_with_mode(rows, true, self.filter_available);
        }

        let items = self
            .rows
            .iter()
            .flat_map(|row| row.iter().copied())
            .filter(|item| keep(item.metric()))
            .collect::<Vec<_>>();

        let rows = split_even_rows_items(&items, self.rows.len().max(1));
        Self::from_rows_with_mode(rows, false, self.filter_available)
    }
}

pub fn all_metrics() -> &'static [MetricKind] {
    &[
        MetricKind::Cpu,
        MetricKind::Memory,
        MetricKind::Storage,
        MetricKind::Io,
        MetricKind::Net,
        MetricKind::In,
        MetricKind::Out,
        MetricKind::Ingress,
        MetricKind::Egress,
        MetricKind::Gpu,
        MetricKind::Vram,
    ]
}

pub fn parse_layout_spec(spec: &str) -> Result<Layout, String> {
    parse_layout_document(spec)?.lower()
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum LayoutToken {
    Word(String),
    Equals,
    Comma,
    LParen,
    RParen,
}

fn tokenize_layout_spec(spec: &str) -> Result<Vec<LayoutToken>, String> {
    let mut tokens = Vec::new();
    let chars = spec.chars().collect::<Vec<_>>();
    let mut index = 0;

    while index < chars.len() {
        let ch = chars[index];
        if ch == '\n' {
            tokens.push(LayoutToken::Comma);
            index += 1;
            continue;
        }
        if ch.is_whitespace() {
            index += 1;
            continue;
        }

        match ch {
            '=' => {
                tokens.push(LayoutToken::Equals);
                index += 1;
            }
            ',' => {
                tokens.push(LayoutToken::Comma);
                index += 1;
            }
            '(' => {
                tokens.push(LayoutToken::LParen);
                index += 1;
            }
            ')' => {
                tokens.push(LayoutToken::RParen);
                index += 1;
            }
            _ => {
                let mut word = String::new();
                while index < chars.len() {
                    let ch = chars[index];
                    if ch.is_whitespace() || matches!(ch, '=' | ',' | '(' | ')') {
                        break;
                    }
                    if matches!(ch, '"' | '\'') {
                        let quote = ch;
                        index += 1;
                        let start = index;
                        while index < chars.len() && chars[index] != quote {
                            index += 1;
                        }
                        if index >= chars.len() {
                            return Err("unterminated quoted string in layout".to_owned());
                        }
                        word.extend(chars[start..index].iter());
                        index += 1;
                        continue;
                    }

                    word.push(ch);
                    index += 1;
                }

                if word.is_empty() {
                    return Err("invalid empty token in layout".to_owned());
                }
                tokens.push(LayoutToken::Word(word));
            }
        }
    }

    Ok(tokens)
}

struct LayoutParser {
    tokens: Vec<LayoutToken>,
    index: usize,
}

impl LayoutParser {
    fn new(tokens: Vec<LayoutToken>) -> Self {
        Self { tokens, index: 0 }
    }

    fn peek(&self) -> Option<&LayoutToken> {
        self.tokens.get(self.index)
    }

    fn peek_n(&self, offset: usize) -> Option<&LayoutToken> {
        self.tokens.get(self.index + offset)
    }

    fn next(&mut self) -> Option<LayoutToken> {
        let token = self.tokens.get(self.index).cloned();
        if token.is_some() {
            self.index += 1;
        }
        token
    }

    fn at_end(&self) -> bool {
        self.index >= self.tokens.len()
    }
}

pub fn parse_layout_document(spec: &str) -> Result<Document, String> {
    if spec.contains(';') {
        return Err("invalid row separator ';'; use ',' or a literal newline".to_owned());
    }

    let tokens = tokenize_layout_spec(spec)?;
    if tokens.is_empty() {
        return Err("layout must contain at least one metric".to_owned());
    }
    let contains_group = tokens
        .iter()
        .any(|token| matches!(token, LayoutToken::LParen | LayoutToken::RParen));
    let explicit_rows = spec.contains(',') || spec.contains('\n') || contains_group;
    let mut force_explicit_rows = false;

    let mut parser = LayoutParser::new(tokens);
    let mut flat = Vec::new();
    let mut flat_items = Vec::new();
    let mut hinted_rows = None;
    let mut filter_available = false;
    let rows = parse_document_rows(
        &mut parser,
        false,
        None,
        explicit_rows,
        &mut flat,
        &mut flat_items,
        &mut hinted_rows,
        &mut filter_available,
        &mut force_explicit_rows,
    )?;
    if !parser.at_end() {
        return Err("unexpected trailing tokens in layout".to_owned());
    }
    if flat_items.is_empty() {
        return Err("layout must contain at least one layout item".to_owned());
    }

    if explicit_rows || force_explicit_rows {
        return Ok(Document::new(rows, true, filter_available));
    }

    let rows = if let Some(row_count) = hinted_rows {
        split_even_rows_document_items(&flat_items, row_count.max(1))
    } else if flat_items.len() > 5 {
        flat_items
            .chunks(5)
            .map(|row| Row::new(None, row.to_vec()))
            .collect()
    } else {
        vec![Row::new(None, flat_items)]
    };

    Ok(Document::new(rows, false, filter_available))
}

fn parse_document_rows(
    parser: &mut LayoutParser,
    stop_at_rparen: bool,
    inherited_label: Option<String>,
    explicit_rows: bool,
    flat: &mut Vec<MetricKind>,
    flat_items: &mut Vec<Item>,
    hinted_rows: &mut Option<usize>,
    filter_available: &mut bool,
    force_explicit_rows: &mut bool,
) -> Result<Vec<Row>, String> {
    let mut rows = Vec::new();
    let mut current = Vec::new();

    while !parser.at_end() {
        match parser.peek() {
            Some(LayoutToken::RParen) if stop_at_rparen => break,
            Some(LayoutToken::Comma) => {
                parser.next();
                if current.is_empty() {
                    return Err("rows cannot be empty".to_owned());
                }
                rows.push(Row::new(inherited_label.clone(), current));
                current = Vec::new();
            }
            _ if starts_group(parser) => {
                if !current.is_empty() {
                    return Err("groups cannot appear inline with other items".to_owned());
                }
                rows.extend(parse_document_group(
                    parser,
                    explicit_rows,
                    flat,
                    flat_items,
                    hinted_rows,
                    filter_available,
                    force_explicit_rows,
                )?);
                if matches!(parser.peek(), Some(LayoutToken::Comma)) {
                    parser.next();
                }
            }
            _ => {
                let rows_for_entry = parse_document_entry(
                    parser,
                    explicit_rows,
                    hinted_rows,
                    filter_available,
                )?;
                if rows_for_entry.len() == 1 && rows_for_entry[0].label.is_none() {
                    for item in &rows_for_entry[0].items {
                        push_unique_document_metrics(flat, &item.source);
                        flat_items.push(item.clone());
                        current.push(item.clone());
                    }
                } else {
                    *force_explicit_rows = true;
                    if !current.is_empty() {
                        return Err("grouped macros cannot appear inline with other items".to_owned());
                    }
                    for row in rows_for_entry {
                        for item in &row.items {
                            push_unique_document_metrics(flat, &item.source);
                            flat_items.push(item.clone());
                        }
                        rows.push(row);
                    }
                }
            }
        }
    }

    if !current.is_empty() {
        rows.push(Row::new(inherited_label, current));
    }

    if rows.is_empty() {
        Err("layout must contain at least one metric".to_owned())
    } else {
        if !explicit_rows && rows.len() > 1 {
            *hinted_rows = Some(rows.len());
        }
        Ok(rows)
    }
}

fn parse_document_group(
    parser: &mut LayoutParser,
    explicit_rows: bool,
    flat: &mut Vec<MetricKind>,
    flat_items: &mut Vec<Item>,
    hinted_rows: &mut Option<usize>,
    filter_available: &mut bool,
    force_explicit_rows: &mut bool,
) -> Result<Vec<Row>, String> {
    let label = if matches!(
        (parser.peek(), parser.peek_n(1), parser.peek_n(2)),
        (
            Some(LayoutToken::Word(_)),
            Some(LayoutToken::Equals),
            Some(LayoutToken::LParen)
        )
    ) {
        match parser.next() {
            Some(LayoutToken::Word(label)) => {
                expect_layout_token(parser, LayoutToken::Equals)?;
                Some(label)
            }
            _ => unreachable!(),
        }
    } else {
        None
    };

    expect_layout_token(parser, LayoutToken::LParen)?;
    let rows = parse_document_rows(
        parser,
        true,
        label,
        true,
        flat,
        flat_items,
        hinted_rows,
        filter_available,
        force_explicit_rows,
    )?;
    expect_layout_token(parser, LayoutToken::RParen)?;
    let _ = explicit_rows;
    Ok(rows)
}

fn parse_document_entry(
    parser: &mut LayoutParser,
    explicit_rows: bool,
    hinted_rows: &mut Option<usize>,
    filter_available: &mut bool,
) -> Result<Vec<Row>, String> {
    let token = match parser.next() {
        Some(LayoutToken::Word(word)) => word,
        Some(other) => return Err(format!("unexpected token in layout: {:?}", other)),
        None => return Err("unexpected end of layout".to_owned()),
    };

    if let Some(entry) = parse_all_items(&token)? {
        match entry {
            ParsedEntry::Items {
                count,
                filter_available: should_filter_available,
                items,
            } => {
                if explicit_rows {
                    return Err("all/avail cannot be mixed with explicit row separators".to_owned());
                }
                *hinted_rows = Some(count);
                *filter_available |= should_filter_available;
                return Ok(vec![Row::new(None, items)]);
            }
            ParsedEntry::Rows {
                filter_available: should_filter_available,
                rows,
            } => {
                if explicit_rows {
                    return Err("all/avail cannot be mixed with explicit row separators".to_owned());
                }
                *filter_available |= should_filter_available;
                return Ok(rows);
            }
        }
    }

    if let Some(items) = parse_group_alias_items(&token)? {
        return Ok(vec![Row::new(None, items)]);
    }

    if matches!(parser.peek(), Some(LayoutToken::Equals))
        && matches!(parser.peek_n(1), Some(LayoutToken::Word(_)))
    {
        parser.next();
        let source = match parser.next() {
            Some(LayoutToken::Word(word)) => word,
            _ => unreachable!(),
        };
        return Ok(vec![Row::new(
            None,
            vec![parse_document_source_item(Some(token), &source)?],
        )]);
    }

    Ok(vec![Row::new(
        None,
        vec![parse_document_source_item(None, &token)?],
    )])
}

fn starts_group(parser: &LayoutParser) -> bool {
    matches!(
        (parser.peek(), parser.peek_n(1), parser.peek_n(2)),
        (
            Some(LayoutToken::Word(_)),
            Some(LayoutToken::Equals),
            Some(LayoutToken::LParen)
        )
    ) || matches!(parser.peek(), Some(LayoutToken::LParen))
}

fn expect_layout_token(parser: &mut LayoutParser, expected: LayoutToken) -> Result<(), String> {
    let actual = parser.next();
    if actual.as_ref() == Some(&expected) {
        Ok(())
    } else {
        Err(format!(
            "unexpected token in layout: expected {:?}, got {:?}",
            expected, actual
        ))
    }
}

fn push_unique(metrics: &mut Vec<MetricKind>, metric: MetricKind) {
    if !metrics.contains(&metric) {
        metrics.push(metric);
    }
}

fn push_unique_document_metrics(metrics: &mut Vec<MetricKind>, source: &Source) {
    match source {
        Source::Metric(metric) => push_unique(metrics, *metric),
        Source::SplitMetric(upper, lower) => {
            push_unique(metrics, *upper);
            push_unique(metrics, *lower);
        }
        Source::StreamColumn(_) | Source::File(_) | Source::Process(_) => {}
    }
}

enum ParsedEntry {
    Items {
        count: usize,
        filter_available: bool,
        items: Vec<Item>,
    },
    Rows {
        filter_available: bool,
        rows: Vec<Row>,
    },
}

fn parse_all_items(token: &str) -> Result<Option<ParsedEntry>, String> {
    if token.starts_with("all/") || token.starts_with("all:") {
        return Err("invalid all syntax; use 'all', 'avail', or write explicit rows".to_owned());
    }
    if token.starts_with("avail/") || token.starts_with("avail:") {
        return Err("invalid avail syntax; use 'avail' or write explicit rows".to_owned());
    }

    if token == "all" {
        return Ok(Some(ParsedEntry::Items {
            count: 2,
            filter_available: false,
            items: all_metrics()
                .iter()
                .copied()
                .map(|metric| Item::metric(metric, LayoutView::Default, DisplayMode::Full, 1, None, None))
                .collect(),
        }));
    }

    if token == "avail" {
        return Ok(Some(ParsedEntry::Rows {
            filter_available: true,
            rows: default_avail_document_rows(LayoutView::Default),
        }));
    }

    let (count, filter_available, view, metrics) = if let Some(view_token) = token.strip_prefix("all.") {
        if view_token.contains(':')
            || view_token.contains('/')
            || view_token.contains('+')
            || view_token.contains('-')
        {
            return Err("all does not support size or width constraints".to_owned());
        }
        (
            2,
            false,
            LayoutView::parse(view_token)
                .ok_or_else(|| format!("unknown metric view: {view_token}"))?,
            all_metrics().to_vec(),
        )
    } else if let Some(view_token) = token.strip_prefix("avail.") {
        if view_token.contains(':')
            || view_token.contains('/')
            || view_token.contains('+')
            || view_token.contains('-')
        {
            return Err("avail does not support size or width constraints".to_owned());
        }
        return Ok(Some(ParsedEntry::Rows {
            filter_available: true,
            rows: default_avail_document_rows(
                LayoutView::parse(view_token)
                    .ok_or_else(|| format!("unknown metric view: {view_token}"))?,
            ),
        }));
    } else {
        return Ok(None);
    };

    Ok(Some(ParsedEntry::Items {
        count,
        filter_available,
        items: metrics
            .iter()
            .copied()
            .map(|metric| Item::metric(metric, view, DisplayMode::Full, 1, None, None))
            .collect(),
    }))
}

fn parse_group_alias_items(token: &str) -> Result<Option<Vec<Item>>, String> {
    let (metrics, view) = if token == "xpu" {
        (vec![MetricKind::Cpu, MetricKind::Gpu], LayoutView::Default)
    } else if token == "mem" {
        (vec![MetricKind::Memory, MetricKind::Vram], LayoutView::Default)
    } else if let Some(view_token) = token.strip_prefix("xpu.") {
        if view_token.contains(':')
            || view_token.contains('/')
            || view_token.contains('+')
            || view_token.contains('-')
        {
            return Err("xpu does not support size or width constraints".to_owned());
        }
        (
            vec![MetricKind::Cpu, MetricKind::Gpu],
            LayoutView::parse(view_token)
                .ok_or_else(|| format!("unknown metric view: {view_token}"))?,
        )
    } else if let Some(view_token) = token.strip_prefix("mem.") {
        if view_token.contains(':')
            || view_token.contains('/')
            || view_token.contains('+')
            || view_token.contains('-')
        {
            return Err("mem does not support size or width constraints".to_owned());
        }
        (
            vec![MetricKind::Memory, MetricKind::Vram],
            LayoutView::parse(view_token)
                .ok_or_else(|| format!("unknown metric view: {view_token}"))?,
        )
    } else {
        return Ok(None);
    };

    Ok(Some(
        metrics
            .into_iter()
            .map(|metric| Item::metric(metric, view, DisplayMode::Full, 1, None, None))
            .collect(),
    ))
}

fn parse_document_source_item(label: Option<String>, token: &str) -> Result<Item, String> {
    let (head, min_width) = parse_positive_suffix(token, '-', "metric min width")?;
    let (head, max_width) = parse_positive_suffix(head, '+', "metric max width")?;

    let (head, basis) = match head.rsplit_once(':') {
        Some((head, basis)) if basis.chars().all(|ch| ch.is_ascii_digit()) => {
            let parsed = basis
                .parse::<usize>()
                .map_err(|_| format!("invalid metric size: {basis}"))?;
            if parsed == 0 {
                return Err("metric sizes must be greater than zero".to_owned());
            }
            (head, Some(parsed))
        }
        Some((_, basis))
            if !head.starts_with("p:") && !head.starts_with("f:") && basis.contains('/') =>
        {
            return Err("invalid metric syntax: use ':size' only".to_owned());
        }
        _ => (head, Some(1)),
    };

    let mut metric_token = head;
    let mut view = LayoutView::Default;
    let mut display = DisplayMode::Full;
    let mut suffixes = Vec::new();
    while let Some((prefix, suffix)) = metric_token.rsplit_once('.') {
        if LayoutView::parse(suffix).is_some() || DisplayMode::parse(suffix).is_some() {
            suffixes.push(suffix);
            metric_token = prefix;
            continue;
        }
        break;
    }
    suffixes.reverse();

    for suffix in suffixes {
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

    let size = basis.unwrap_or(1);
    if let Some(column) = metric_token.strip_prefix('@') {
        let parsed_column = column
            .parse::<usize>()
            .map_err(|_| format!("invalid stream column reference: @{column}"))?;
        if parsed_column == 0 {
            return Err("stream column references are 1-based; use @1, @2, ...".to_owned());
        }
        return Ok(Item::stream_column(
            label,
            parsed_column - 1,
            display,
            size,
            max_width,
            min_width,
        ));
    }
    if let Some(path) = metric_token.strip_prefix("f:") {
        let path = path.trim();
        if path.is_empty() {
            return Err("f: source path must be non-empty".to_owned());
        }
        return Ok(Item::file(
            label,
            PathBuf::from(path),
            display,
            size,
            max_width,
            min_width,
        ));
    }
    if let Some(command) = metric_token.strip_prefix("p:") {
        let command = command.trim();
        if command.is_empty() {
            return Err("p: source command must be non-empty".to_owned());
        }
        return Ok(Item::process(
            label,
            command.to_owned(),
            display,
            size,
            max_width,
            min_width,
        ));
    }

    if let Some((upper, lower)) = metric_token.split_once('+') {
        let upper = MetricKind::parse(upper).ok_or_else(|| format!("unknown metric: {upper}"))?;
        let lower = MetricKind::parse(lower).ok_or_else(|| format!("unknown metric: {lower}"))?;
        return Ok(Item::split_metric(
            label,
            upper,
            lower,
            display,
            size,
            max_width,
            min_width,
        ));
    }

    let metric =
        MetricKind::parse(metric_token).ok_or_else(|| format!("unknown metric: {metric_token}"))?;
    Ok(Item {
        label,
        source: Source::Metric(metric),
        view,
        display,
        size,
        max_width,
        min_width,
    })
}

fn parse_positive_suffix<'a>(
    token: &'a str,
    delimiter: char,
    label: &str,
) -> Result<(&'a str, Option<usize>), String> {
    match token.rsplit_once(delimiter) {
        Some((head, value)) if value.chars().all(|ch| ch.is_ascii_digit()) => {
            let parsed = value
                .parse::<usize>()
                .map_err(|_| format!("invalid {label}: {value}"))?;
            if parsed == 0 {
                return Err(format!("{label}s must be greater than zero"));
            }
            Ok((head, Some(parsed)))
        }
        _ => Ok((token, None)),
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

fn split_even_rows_document_items(items: &[Item], rows: usize) -> Vec<Row> {
    let heights = split_even_width(items.len(), rows);
    let mut start = 0;

    heights
        .into_iter()
        .filter(|count| *count > 0)
        .map(|count| {
            let end = start + count;
            let row = Row::new(None, items[start..end].to_vec());
            start = end;
            row
        })
        .collect()
}

fn default_avail_metric_rows() -> Vec<Vec<MetricKind>> {
    vec![
        vec![
            MetricKind::Cpu,
            MetricKind::Gpu,
            MetricKind::Memory,
            MetricKind::Vram,
            MetricKind::Storage,
        ],
        vec![MetricKind::Io, MetricKind::Net],
    ]
}

fn default_avail_document_rows(view: LayoutView) -> Vec<Row> {
    default_avail_metric_rows()
        .into_iter()
        .map(|row| {
            Row::new(
                None,
                row.into_iter()
                    .map(|metric| Item::metric(metric, view, DisplayMode::Full, 1, None, None))
                    .collect(),
            )
        })
        .collect()
}

fn default_avail_layout_rows(view: LayoutView) -> Vec<Vec<LayoutItem>> {
    default_avail_metric_rows()
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|metric| LayoutItem::new(metric, view, Some(1), 1))
                .collect()
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
        assert_eq!(
            layout.rows()[1]
                .iter()
                .map(|item| item.metric())
                .collect::<Vec<_>>(),
            [MetricKind::Memory, MetricKind::Vram, MetricKind::Net]
        );
    }

    #[test]
    fn document_parser_preserves_rows_and_metric_sources() {
        let document = parse_layout_document("cpu ram, gpu").unwrap();
        assert!(document.explicit_rows());
        assert_eq!(document.rows().len(), 2);
        assert_eq!(document.rows()[0].items().len(), 2);
        assert_eq!(document.rows()[1].items().len(), 1);
        assert_eq!(
            document.rows()[0].items()[0].source(),
            &Source::Metric(MetricKind::Cpu)
        );
        assert_eq!(
            document.rows()[0].items()[1].source(),
            &Source::Metric(MetricKind::Memory)
        );
        assert_eq!(
            document.rows()[1].items()[0].source(),
            &Source::Metric(MetricKind::Gpu)
        );
    }

    #[test]
    fn document_parser_handles_stream_and_process_sources() {
        let document =
            parse_layout_document("cpu=@1 \"load avg\"=p:'cut -f 1 -d\\  /proc/loadavg'").unwrap();
        assert_eq!(document.rows().len(), 1);
        assert_eq!(document.rows()[0].items()[0].label(), Some("cpu"));
        assert_eq!(document.rows()[0].items()[0].source(), &Source::StreamColumn(0));
        assert_eq!(document.rows()[0].items()[1].label(), Some("load avg"));
        assert_eq!(
            document.rows()[0].items()[1].source(),
            &Source::Process("cut -f 1 -d\\  /proc/loadavg".to_owned())
        );
    }

    #[test]
    fn document_parser_handles_split_metric_sources() {
        let document = parse_layout_document("spcram=(spc+ram)").unwrap();
        assert_eq!(document.rows().len(), 1);
        assert_eq!(document.rows()[0].label(), Some("spcram"));
        assert_eq!(
            document.rows()[0].items()[0].source(),
            &Source::SplitMetric(MetricKind::Storage, MetricKind::Memory)
        );
    }

    #[test]
    fn document_parser_applies_group_labels_to_rows() {
        let document = parse_layout_document("\"pair\"=(a=@1, b=@2)").unwrap();
        assert!(document.explicit_rows());
        assert_eq!(document.rows().len(), 2);
        assert_eq!(document.rows()[0].label(), Some("pair"));
        assert_eq!(document.rows()[1].label(), Some("pair"));
        assert_eq!(document.rows()[0].items()[0].label(), Some("a"));
        assert_eq!(document.rows()[1].items()[0].label(), Some("b"));
    }

    #[test]
    fn document_lowering_matches_current_layout_behavior() {
        let document = parse_layout_document("cpu:3 ram.value+8-2").unwrap();
        let layout = document.lower().unwrap();
        let item0 = layout.rows()[0][0];
        let item1 = layout.rows()[0][1];

        assert_eq!(item0.metric(), MetricKind::Cpu);
        assert_eq!(item0.basis(), Some(3));
        assert_eq!(item0.grow(), 3);
        assert_eq!(item1.metric(), MetricKind::Memory);
        assert_eq!(item1.display(), DisplayMode::Value);
        assert_eq!(item1.max_width(), Some(8));
        assert_eq!(item1.min_width(), Some(2));
    }

    #[test]
    fn default_layout_matches_avail() {
        assert_eq!(Layout::default(), parse_layout_spec("avail").unwrap());
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
    fn avail_expands_to_the_full_metric_set_with_filtering_enabled() {
        let layout = parse_layout_spec("avail").unwrap();
        assert_eq!(
            layout.metrics(),
            &[
                MetricKind::Cpu,
                MetricKind::Gpu,
                MetricKind::Memory,
                MetricKind::Vram,
                MetricKind::Storage,
                MetricKind::Io,
                MetricKind::Net,
            ]
        );
        assert_eq!(layout.rows().len(), 2);
        assert_eq!(layout.rows()[0].len(), 5);
        assert_eq!(layout.rows()[1].len(), 2);
        assert!(layout.filter_available());
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
    fn xpu_expands_to_cpu_and_gpu() {
        let layout = parse_layout_spec("xpu").unwrap();
        assert_eq!(layout.metrics(), &[MetricKind::Cpu, MetricKind::Gpu]);
        assert_eq!(layout.rows().len(), 1);
        assert_eq!(
            layout.rows()[0]
                .iter()
                .map(|item| item.metric())
                .collect::<Vec<_>>(),
            [MetricKind::Cpu, MetricKind::Gpu]
        );
    }

    #[test]
    fn mem_expands_to_ram_and_vram() {
        let layout = parse_layout_spec("mem").unwrap();
        assert_eq!(layout.metrics(), &[MetricKind::Memory, MetricKind::Vram]);
        assert_eq!(layout.rows().len(), 1);
        assert_eq!(
            layout.rows()[0]
                .iter()
                .map(|item| item.metric())
                .collect::<Vec<_>>(),
            [MetricKind::Memory, MetricKind::Vram]
        );
    }

    #[test]
    fn grouped_metric_aliases_accept_views() {
        let layout = parse_layout_spec("xpu.pct mem.free").unwrap();
        assert_eq!(layout.rows().len(), 1);
        assert!(layout.rows()[0][0..2]
            .iter()
            .all(|item| item.view() == LayoutView::Pct));
        assert!(layout.rows()[0][2..4]
            .iter()
            .all(|item| item.view() == LayoutView::Free));
    }

    #[test]
    fn avail_view_expands_to_the_full_metric_set_with_that_view() {
        let layout = parse_layout_spec("avail.pct").unwrap();
        assert_eq!(
            layout.metrics(),
            &[
                MetricKind::Cpu,
                MetricKind::Gpu,
                MetricKind::Memory,
                MetricKind::Vram,
                MetricKind::Storage,
                MetricKind::Io,
                MetricKind::Net,
            ]
        );
        assert_eq!(layout.rows().len(), 2);
        assert!(layout.filter_available());
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
                    MetricKind::In,
                    MetricKind::Out,
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
                    MetricKind::In,
                    MetricKind::Out,
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
                    MetricKind::In,
                    MetricKind::Out,
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
                MetricKind::In,
                MetricKind::Out,
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
    fn avail_row_count_suffix_is_rejected() {
        let error = parse_layout_spec("avail/3").unwrap_err();
        assert!(error.contains("invalid avail syntax"));
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
