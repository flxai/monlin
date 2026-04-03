use clap::{ArgAction, CommandFactory, Parser, ValueEnum};

use crate::layout::{parse_layout_spec, Layout};
use crate::render::Renderer;

const DEFAULT_INTERVAL_MS: u64 = 1000;
const DEFAULT_HISTORY: usize = 512;

const AFTER_HELP: &str = "\
Metrics:
  cpu sys gpu vram gfx memory spc io net ingress egress
  all

Notes:
  Layout is the canonical interface.
  Item syntax is metric[.view][:basis][/grow][+max][-min], e.g. net.hum:12/2+20-8.
  Rows can be separated with ',' or a literal newline.
  Flat layouts auto-wrap after 5 metrics per row.
  If stdin provides whitespace-separated numeric rows, monlin switches to stream mode automatically.
";

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum Align {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum ColorMode {
    Auto,
    Always,
    Never,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum OutputMode {
    Terminal,
    I3bar,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum StreamLayout {
    Columns,
    Lines,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub history: usize,
    pub interval_ms: u64,
    pub align: Align,
    pub label: Option<String>,
    pub stream_labels: Option<Vec<String>>,
    pub stream_layout: StreamLayout,
    pub layout: Layout,
    pub renderer: Renderer,
    pub color_mode: ColorMode,
    pub output_mode: OutputMode,
    pub width: Option<usize>,
    pub once: bool,
    pub force_stream_input: bool,
    pub show_help: bool,
}

#[derive(Debug, Parser)]
#[command(
    name = "monlin",
    disable_help_flag = true,
    disable_version_flag = true,
    about = "Compact terminal monitor for narrow panes, bars, and shell-driven status views.",
    after_help = AFTER_HELP
)]
struct Cli {
    #[arg(value_name = "LAYOUT")]
    layout_parts: Vec<String>,

    #[arg(long, default_value_t = DEFAULT_HISTORY)]
    history: usize,

    #[arg(long = "interval-ms", default_value_t = DEFAULT_INTERVAL_MS)]
    interval_ms: u64,

    #[arg(long, value_enum, default_value_t = Align::Left)]
    align: Align,

    #[arg(long)]
    label: Option<String>,

    #[arg(
        long = "labels",
        value_delimiter = ',',
        value_parser = parse_stream_label
    )]
    stream_labels: Vec<String>,

    #[arg(long = "stream-layout", value_enum, default_value_t = StreamLayout::Columns)]
    stream_layout: StreamLayout,

    #[arg(long, value_enum, default_value_t = Renderer::Braille)]
    renderer: Renderer,

    #[arg(long = "color", value_enum, default_value_t = ColorMode::Auto)]
    color_mode: ColorMode,

    #[arg(long = "output", value_enum, default_value_t = OutputMode::Terminal)]
    output_mode: OutputMode,

    #[arg(long)]
    width: Option<usize>,

    #[arg(long, action = ArgAction::SetTrue)]
    once: bool,

    #[arg(short = 'h', long = "help", action = ArgAction::SetTrue)]
    show_help: bool,
}

pub fn parse_args<I>(args: I) -> Result<Config, String>
where
    I: IntoIterator<Item = String>,
{
    let args = args.into_iter().collect::<Vec<_>>();
    let mut filtered_args = Vec::with_capacity(args.len());
    let mut force_stream_input = false;

    for arg in args {
        if arg == "-" {
            force_stream_input = true;
        } else {
            filtered_args.push(arg);
        }
    }

    let cli = Cli::try_parse_from(filtered_args).map_err(|error| error.to_string())?;

    let history = cli.history.max(1);
    if let Some(value) = cli.width {
        if value == 0 {
            return Err("--width must be greater than zero".to_owned());
        }
    }

    let layout = if cli.layout_parts.is_empty() {
        Layout::default()
    } else {
        parse_layout_spec(&cli.layout_parts.join(" "))?
    };

    Ok(Config {
        history,
        interval_ms: cli.interval_ms,
        align: cli.align,
        label: cli.label,
        stream_labels: (!cli.stream_labels.is_empty()).then_some(cli.stream_labels),
        stream_layout: cli.stream_layout,
        layout,
        renderer: cli.renderer,
        color_mode: cli.color_mode,
        output_mode: cli.output_mode,
        width: cli.width,
        once: cli.once,
        force_stream_input,
        show_help: cli.show_help,
    })
}

fn parse_stream_label(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        Err("labels must be non-empty".to_owned())
    } else {
        Ok(trimmed.to_owned())
    }
}

pub fn help_text() -> String {
    let mut command = Cli::command();
    let mut help = Vec::new();
    command
        .write_long_help(&mut help)
        .expect("writing clap help should succeed");
    String::from_utf8(help).expect("clap help is valid utf-8")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::MetricKind;

    fn parse(items: &[&str]) -> Config {
        parse_args(items.iter().map(|item| item.to_string())).unwrap()
    }

    #[test]
    fn defaults_to_all_layout() {
        let config = parse(&["monlin"]);
        assert_eq!(config.layout.metrics(), crate::layout::all_metrics());
        assert_eq!(config.layout.rows().len(), 2);
        assert_eq!(config.output_mode, OutputMode::Terminal);
    }

    #[test]
    fn parses_layout_from_positionals() {
        let config = parse(&["monlin", "cpu", "net"]);
        assert_eq!(config.layout.metrics(), &[MetricKind::Cpu, MetricKind::Net]);
    }

    #[test]
    fn rejects_removed_layout_flag() {
        let error = parse_args(
            ["monlin", "--layout", "cpu gpu"]
                .into_iter()
                .map(|item| item.to_string()),
        )
        .unwrap_err();
        assert!(error.contains("--layout"));
    }

    #[test]
    fn bare_all_is_accepted() {
        let config = parse(&["monlin", "all"]);
        assert_eq!(config.layout.rows().len(), 2);
    }

    #[test]
    fn parses_i3bar_output_mode() {
        let config = parse(&["monlin", "--output", "i3bar"]);
        assert_eq!(config.output_mode, OutputMode::I3bar);
    }

    #[test]
    fn parses_stream_labels() {
        let config = parse(&["monlin", "--labels", "wifi,eth,vpn"]);
        assert_eq!(
            config.stream_labels,
            Some(vec![
                "wifi".to_owned(),
                "eth".to_owned(),
                "vpn".to_owned()
            ])
        );
    }

    #[test]
    fn parses_stream_layout() {
        let config = parse(&["monlin", "--stream-layout", "lines"]);
        assert_eq!(config.stream_layout, StreamLayout::Lines);
    }

    #[test]
    fn rejects_empty_stream_labels() {
        let error = parse_args(
            ["monlin", "--labels", "wifi,,vpn"]
                .into_iter()
                .map(|item| item.to_string()),
        )
        .unwrap_err();
        assert!(error.contains("labels"));
    }

    #[test]
    fn zero_history_is_clamped_to_one() {
        let config = parse(&["monlin", "--history", "0"]);
        assert_eq!(config.history, 1);
    }

    #[test]
    fn rejects_zero_width() {
        let error = parse_args(
            ["monlin", "--width", "0"]
                .into_iter()
                .map(|item| item.to_string()),
        )
        .unwrap_err();
        assert!(error.contains("--width must be greater than zero"));
    }

    #[test]
    fn rejects_invalid_align_value() {
        let error = parse_args(
            ["monlin", "--align", "center"]
                .into_iter()
                .map(|item| item.to_string()),
        )
        .unwrap_err();
        assert!(error.contains("--align"));
    }

    #[test]
    fn rejects_invalid_renderer_value() {
        let error = parse_args(
            ["monlin", "--renderer", "spark"]
                .into_iter()
                .map(|item| item.to_string()),
        )
        .unwrap_err();
        assert!(error.contains("--renderer"));
    }

    #[test]
    fn rejects_invalid_color_value() {
        let error = parse_args(
            ["monlin", "--color", "sometimes"]
                .into_iter()
                .map(|item| item.to_string()),
        )
        .unwrap_err();
        assert!(error.contains("--color"));
    }

    #[test]
    fn rejects_invalid_output_value() {
        let error = parse_args(
            ["monlin", "--output", "json"]
                .into_iter()
                .map(|item| item.to_string()),
        )
        .unwrap_err();
        assert!(error.contains("--output"));
    }

    #[test]
    fn rejects_unknown_flag() {
        let error =
            parse_args(["monlin", "--wat"].into_iter().map(|item| item.to_string())).unwrap_err();
        assert!(error.contains("--wat"));
    }

    #[test]
    fn rejects_missing_flag_value() {
        let error = parse_args(
            ["monlin", "--label"]
                .into_iter()
                .map(|item| item.to_string()),
        )
        .unwrap_err();
        assert!(error.contains("--label"));
    }

    #[test]
    fn rejects_invalid_numeric_flag_value() {
        let error = parse_args(
            ["monlin", "--interval-ms", "fast"]
                .into_iter()
                .map(|item| item.to_string()),
        )
        .unwrap_err();
        assert!(error.contains("--interval-ms"));
    }

    #[test]
    fn help_text_documents_current_layout_syntax() {
        let help = help_text();
        assert!(help.contains("metric[.view][:basis][/grow][+max][-min]"));
        assert!(help.contains("Rows can be separated with ',' or a literal newline."));
    }
}
