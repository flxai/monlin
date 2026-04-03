use clap::{ArgAction, CommandFactory, Parser, ValueEnum};

use crate::color::{ColorSpec, Rgb};
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum Space {
    Stable,
    Graph,
    Segment,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum CompletionShell {
    Bash,
    Elvish,
    Fish,
    PowerShell,
    Zsh,
}

#[derive(Clone, Debug, Eq, PartialEq, clap::Subcommand)]
enum DebugCommand {
    #[command(about = "Print sampled metric color ramps")]
    Colors {
        #[arg(long, default_value_t = 10, help = "Number of glyph samples to print per metric")]
        steps: usize,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, clap::Subcommand)]
enum CliCommand {
    #[command(about = "Generate shell completion scripts")]
    Completion {
        #[arg(value_enum, help = "Shell to generate completions for")]
        shell: CompletionShell,
    },
    #[command(about = "Debug rendering helpers")]
    Debug {
        #[command(subcommand)]
        command: DebugCommand,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub history: usize,
    pub interval_ms: u64,
    pub align: Align,
    pub label: Option<String>,
    pub stream_labels: Option<Vec<String>>,
    pub stream_layout: StreamLayout,
    pub space: Space,
    pub layout: Layout,
    pub renderer: Renderer,
    pub color_mode: ColorMode,
    pub output_mode: OutputMode,
    pub width: Option<usize>,
    pub once: bool,
    pub colors: Option<Vec<ColorSpec>>,
    pub force_stream_input: bool,
    pub print_completion: Option<CompletionShell>,
    pub debug_colors_steps: Option<usize>,
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
    #[command(subcommand)]
    command: Option<CliCommand>,

    #[arg(
        value_name = "LAYOUT",
        help = "Layout DSL, e.g. 'sys gfx io net' or 'cpu:12/2 ram:10 net.hum/3'"
    )]
    layout_parts: Vec<String>,

    #[arg(long, default_value_t = DEFAULT_HISTORY, help = "Number of history samples to retain")]
    history: usize,

    #[arg(
        long = "interval-ms",
        default_value_t = DEFAULT_INTERVAL_MS,
        help = "Sampling interval in milliseconds"
    )]
    interval_ms: u64,

    #[arg(long, value_enum, default_value_t = Align::Left, help = "Place the value before or after the graph")]
    align: Align,

    #[arg(long, help = "Prefix every rendered line with a label")]
    label: Option<String>,

    #[arg(
        long = "labels",
        value_delimiter = ',',
        value_parser = parse_stream_label,
        help = "Comma-separated labels for stdin stream columns"
    )]
    stream_labels: Vec<String>,

    #[arg(
        long = "stream-layout",
        value_enum,
        default_value_t = StreamLayout::Columns,
        help = "Render streamed stdin as columns on one line or as separate lines"
    )]
    stream_layout: StreamLayout,

    #[arg(
        long = "space",
        value_enum,
        default_value_t = Space::Stable,
        help = "How streamed columns allocate width: stable prefixes, compact equal graph space, or equal total segment space"
    )]
    space: Space,

    #[arg(long, value_enum, default_value_t = Renderer::Braille, help = "Graph renderer to use")]
    renderer: Renderer,

    #[arg(
        short = 'c',
        long = "colors",
        value_delimiter = ',',
        value_parser = parse_color_spec,
        help = "Comma-separated visible-order colors: angle 20 or A20, RGB Rff8800/Lff8800, or packed LCh L086078020/R086078020"
    )]
    colors: Vec<ColorSpec>,

    #[arg(long = "color", value_enum, default_value_t = ColorMode::Auto, help = "When to emit ANSI colors")]
    color_mode: ColorMode,

    #[arg(long = "output", value_enum, default_value_t = OutputMode::Terminal, help = "Output protocol to render")]
    output_mode: OutputMode,

    #[arg(long, help = "Override the render width")]
    width: Option<usize>,

    #[arg(long, action = ArgAction::SetTrue, help = "Render one frame and exit")]
    once: bool,

    #[arg(short = 'h', long = "help", action = ArgAction::SetTrue, help = "Show help text")]
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
    let colors = (!cli.colors.is_empty()).then_some(cli.colors);

    Ok(Config {
        history,
        interval_ms: cli.interval_ms,
        align: cli.align,
        label: cli.label,
        stream_labels: (!cli.stream_labels.is_empty()).then_some(cli.stream_labels),
        stream_layout: cli.stream_layout,
        space: cli.space,
        layout,
        renderer: cli.renderer,
        colors,
        color_mode: cli.color_mode,
        output_mode: cli.output_mode,
        width: cli.width,
        once: cli.once,
        force_stream_input,
        print_completion: match cli.command {
            Some(CliCommand::Completion { shell }) => Some(shell),
            _ => None,
        },
        debug_colors_steps: match cli.command {
            Some(CliCommand::Debug {
                command: DebugCommand::Colors { steps },
            }) => Some(steps.max(1)),
            Some(CliCommand::Completion { .. }) => None,
            None => None,
        },
        show_help: cli.show_help,
    })
}

fn parse_color_spec(raw: &str) -> Result<ColorSpec, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("color token must be non-empty".to_owned());
    }

    if let Some(body) = trimmed.strip_prefix(['A', 'a']) {
        return parse_angle(body);
    }

    if let Some(body) = trimmed.strip_prefix(['L', 'l', 'R', 'r']) {
        return parse_prefixed_color(body, trimmed);
    }

    parse_angle(trimmed)
}

fn parse_prefixed_color(body: &str, raw: &str) -> Result<ColorSpec, String> {
    if is_hex_rgb(body) {
        return parse_hex_rgb(body).map(ColorSpec::Rgb);
    }
    if is_packed_lch(body) {
        return parse_packed_lch(body);
    }
    Err(format!(
        "invalid color token: {raw} (expected 6 hex RGB digits or 9 packed LCh digits)"
    ))
}

fn parse_angle(raw: &str) -> Result<ColorSpec, String> {
    let value = raw
        .trim()
        .parse::<f32>()
        .map_err(|_| format!("invalid angle: {raw}"))?;
    if !(0.0..=360.0).contains(&value) {
        return Err(format!("angle out of range 0..360: {value}"));
    }
    Ok(ColorSpec::Angle(value))
}

fn is_hex_rgb(raw: &str) -> bool {
    raw.len() == 6 && raw.chars().all(|ch| ch.is_ascii_hexdigit())
}

fn is_packed_lch(raw: &str) -> bool {
    raw.len() == 9 && raw.chars().all(|ch| ch.is_ascii_digit())
}

fn parse_hex_rgb(raw: &str) -> Result<Rgb, String> {
    let parse_channel = |range: std::ops::Range<usize>| {
        u8::from_str_radix(&raw[range], 16).map_err(|_| format!("invalid rgb hex: {raw}"))
    };
    Ok(Rgb {
        r: parse_channel(0..2)?,
        g: parse_channel(2..4)?,
        b: parse_channel(4..6)?,
    })
}

fn parse_packed_lch(raw: &str) -> Result<ColorSpec, String> {
    let lightness = raw[0..3]
        .parse::<f32>()
        .map_err(|_| format!("invalid LCh lightness: {raw}"))?;
    let chroma = raw[3..6]
        .parse::<f32>()
        .map_err(|_| format!("invalid LCh chroma: {raw}"))?;
    let hue = raw[6..9]
        .parse::<f32>()
        .map_err(|_| format!("invalid LCh hue: {raw}"))?;
    if !(0.0..=100.0).contains(&lightness) {
        return Err(format!("LCh lightness out of range 0..100: {lightness}"));
    }
    if !(0.0..=360.0).contains(&hue) {
        return Err(format!("LCh hue out of range 0..360: {hue}"));
    }
    Ok(ColorSpec::Lch {
        lightness,
        chroma,
        hue,
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

pub fn clap_command() -> clap::Command {
    Cli::command()
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
    fn parses_completion_shell() {
        let config = parse(&["monlin", "completion", "zsh"]);
        assert_eq!(config.print_completion, Some(CompletionShell::Zsh));
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
    fn parses_space_mode() {
        let config = parse(&["monlin", "--space", "segment"]);
        assert_eq!(config.space, Space::Segment);
    }

    #[test]
    fn defaults_to_stable_stream_spacing() {
        let config = parse(&["monlin"]);
        assert_eq!(config.space, Space::Stable);
    }

    #[test]
    fn parses_bare_angle_colors() {
        let config = parse(&["monlin", "--colors", "20,80"]);
        assert_eq!(
            config.colors,
            Some(vec![ColorSpec::Angle(20.0), ColorSpec::Angle(80.0)])
        );
    }

    #[test]
    fn parses_prefixed_angle_colors() {
        let config = parse(&["monlin", "--colors", "A20,a80"]);
        assert_eq!(
            config.colors,
            Some(vec![ColorSpec::Angle(20.0), ColorSpec::Angle(80.0)])
        );
    }

    #[test]
    fn parses_rgb_hex_under_l_and_r_prefixes() {
        let config = parse(&["monlin", "--colors", "Lff8800,R00ffaa"]);
        assert_eq!(
            config.colors,
            Some(vec![
                ColorSpec::Rgb(Rgb {
                    r: 0xff,
                    g: 0x88,
                    b: 0x00
                }),
                ColorSpec::Rgb(Rgb {
                    r: 0x00,
                    g: 0xff,
                    b: 0xaa
                }),
            ])
        );
    }

    #[test]
    fn parses_packed_lch_under_l_and_r_prefixes() {
        let config = parse(&["monlin", "--colors", "L086078020,R075060200"]);
        assert_eq!(
            config.colors,
            Some(vec![
                ColorSpec::Lch {
                    lightness: 86.0,
                    chroma: 78.0,
                    hue: 20.0,
                },
                ColorSpec::Lch {
                    lightness: 75.0,
                    chroma: 60.0,
                    hue: 200.0,
                },
            ])
        );
    }

    #[test]
    fn rejects_invalid_prefixed_color_payload() {
        let error = parse_args(
            ["monlin", "--colors", "Lwat"]
                .into_iter()
                .map(|item| item.to_string()),
        )
        .unwrap_err();
        assert!(error.contains("invalid color token"));
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
