use clap::{ArgAction, CommandFactory, Parser, ValueEnum};
use serde::Deserialize;
use std::env;
use std::fs;
use std::path::PathBuf;

use crate::color::{named_colormap, named_palette, ColorSpec, Rgb};
use crate::layout::{
    parse_layout_document, parse_layout_spec, DisplayMode, Document, Item, Layout, Row,
};
use crate::render::Renderer;

const DEFAULT_INTERVAL_MS: u64 = 1000;
const DEFAULT_HISTORY: usize = 512;

const AFTER_HELP: &str = "\
Metrics:
  cpu xpu rnd sys gpu vram gfx memory mem spc io net in out rx tx
  all
  avail

Notes:
  Layout is the canonical interface.
  Without a layout, monlin defaults to avail.
  Defaults can be read from $XDG_CONFIG_HOME/monlin/config.toml or ~/.config/monlin/config.toml.
  Legacy shell-word configs in .../config are still supported.
  Item syntax is metric[.view][:size][+max][-min], e.g. xpu io net.hum:12+20-8.
  Rows can be separated with ',' or a literal newline.
  Flat layouts auto-wrap after 5 metrics per row.
  If stdin provides whitespace-separated numeric rows, monlin switches to stream mode automatically.
  Use f:/path/to/file to poll a file for numeric rows, or p:command to poll a shell command.
  Prefix external sources with label1,label2= to label their stream columns, e.g. cpu,ram=p:echo 10 20.
  Stream layouts can reference stdin columns explicitly with @1, @2, ... and groups, e.g. graph=(cpu=@1 ram=@2).
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
pub enum LayoutEngine {
    Auto,
    Flow,
    Flex,
    Grid,
    Pack,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum CompletionShell {
    Bash,
    Colors,
    Elvish,
    Fish,
    PowerShell,
    Zsh,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExternalInputSource {
    File(PathBuf),
    Process(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamItem {
    pub label: Option<String>,
    pub column_index: usize,
    pub display: DisplayMode,
    pub basis: usize,
    pub max_width: Option<usize>,
    pub min_width: Option<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamGroup {
    pub label: Option<String>,
    pub rows: Vec<Vec<StreamItem>>,
}

#[derive(Clone, Debug, Eq, PartialEq, clap::Subcommand)]
enum DebugCommand {
    #[command(about = "Print sampled metric color ramps")]
    Colors {
        #[arg(
            long,
            default_value_t = 10,
            help = "Number of glyph samples to print per metric"
        )]
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
    pub document: Option<Document>,
    pub history: usize,
    pub interval_ms: u64,
    pub align: Align,
    pub packed: bool,
    pub label: Option<String>,
    pub stream_labels: Option<Vec<String>>,
    pub stream_groups: Option<Vec<StreamGroup>>,
    pub stream_layout: StreamLayout,
    pub space: Space,
    pub layout_engine: LayoutEngine,
    pub layout: Layout,
    pub renderer: Renderer,
    pub color_mode: ColorMode,
    pub output_mode: OutputMode,
    pub width: Option<usize>,
    pub once: bool,
    pub colors: Option<Vec<ColorSpec>>,
    pub force_stream_input: bool,
    pub external_input: Option<ExternalInputSource>,
    pub print_completion: Option<CompletionShell>,
    pub debug_colors_steps: Option<usize>,
    pub show_help: bool,
}

#[derive(Debug, Parser)]
#[command(
    name = "monlin",
    disable_help_flag = true,
    disable_version_flag = true,
    args_override_self = true,
    about = "Compact terminal monitor for narrow panes, bars, and shell-driven status views.",
    after_help = AFTER_HELP
)]
struct Cli {
    #[command(subcommand)]
    command: Option<CliCommand>,

    #[arg(
        value_name = "LAYOUT",
        help = "Layout DSL, e.g. 'sys gfx io net' or 'cpu:12 ram:10 net.hum'"
    )]
    layout_parts: Vec<String>,

    #[arg(long, default_value_t = DEFAULT_HISTORY, help = "Number of history samples to retain")]
    history: usize,

    #[arg(
        short = 'i',
        long = "interval-ms",
        default_value_t = DEFAULT_INTERVAL_MS,
        help = "Sampling interval in milliseconds"
    )]
    interval_ms: u64,

    #[arg(long, value_enum, default_value_t = Align::Left, help = "Place the value before or after the graph")]
    align: Align,

    #[arg(
        short = 'p',
        long = "packed",
        action = ArgAction::SetTrue,
        help = "Render packed graph-only output without labels, values, or inter-item spacing"
    )]
    packed: bool,

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

    #[arg(
        short = 'e',
        long = "engine",
        value_enum,
        default_value_t = LayoutEngine::Auto,
        help = "How native layouts arrange rows: automatic (grid for multiline, flow for single-line), row-local flow, proportional flex spanning, or multiline grid alignment"
    )]
    layout_engine: LayoutEngine,

    #[arg(long, value_enum, default_value_t = Renderer::Braille, help = "Graph renderer to use")]
    renderer: Renderer,

    #[arg(
        short = 'c',
        long = "colors",
        help = "Colors: a named palette like gruvbox or solarized, a named colormap like turbo or viridis, angle 20 or A20, RGB Rff8800, or packed LCh L086078020"
    )]
    colors: Option<String>,

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConfigFormat {
    LegacyArgs,
    Toml,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ConfigSource {
    path: PathBuf,
    format: ConfigFormat,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct TomlConfig {
    layout: Option<String>,
    history: Option<usize>,
    #[serde(alias = "interval-ms")]
    interval_ms: Option<u64>,
    align: Option<String>,
    packed: Option<bool>,
    label: Option<String>,
    #[serde(alias = "labels")]
    stream_labels: Option<StringList>,
    #[serde(alias = "stream-layout")]
    stream_layout: Option<String>,
    space: Option<String>,
    #[serde(alias = "engine")]
    layout_engine: Option<String>,
    renderer: Option<String>,
    colors: Option<StringList>,
    #[serde(alias = "color")]
    color_mode: Option<String>,
    #[serde(alias = "output")]
    output_mode: Option<String>,
    width: Option<usize>,
    once: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum StringList {
    Single(String),
    Many(Vec<String>),
}

impl StringList {
    fn into_csv(self) -> Option<String> {
        match self {
            Self::Single(value) => {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_owned())
                }
            }
            Self::Many(values) => {
                let values = values
                    .into_iter()
                    .map(|value| value.trim().to_owned())
                    .filter(|value| !value.is_empty())
                    .collect::<Vec<_>>();
                if values.is_empty() {
                    None
                } else {
                    Some(values.join(","))
                }
            }
        }
    }
}

pub fn parse_args<I>(args: I) -> Result<Config, String>
where
    I: IntoIterator<Item = String>,
{
    let args = args.into_iter().collect::<Vec<_>>();
    let args = resolve_args_with_config(args)?;
    parse_args_from_vec(args)
}

fn parse_args_from_vec(args: Vec<String>) -> Result<Config, String> {
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

    let joined_layout = cli.layout_parts.join(" ");
    let document = if joined_layout.trim().is_empty() {
        None
    } else {
        parse_layout_document(&joined_layout).ok()
    };
    let (external_input, inline_stream_labels) = if cli.layout_parts.is_empty() {
        (None, None)
    } else {
        parse_external_input(&joined_layout)?
    };
    if external_input.is_some() && force_stream_input {
        return Err("'-' cannot be combined with f:... or p:... input sources".to_owned());
    }
    let stream_groups = None;
    if inline_stream_labels.is_some() && !cli.stream_labels.is_empty() {
        return Err("inline stream labels cannot be combined with --labels".to_owned());
    }
    if document
        .as_ref()
        .is_some_and(|document| document.uses_stream_columns())
        && !cli.stream_labels.is_empty()
    {
        return Err("stream column layouts cannot be combined with --labels".to_owned());
    }

    let layout = if external_input.is_some()
        || stream_groups.is_some()
        || cli.layout_parts.is_empty()
        || document.as_ref().is_some_and(|document| {
            !document.is_native_only() || document.uses_split_metrics() || document.has_row_labels()
        }) {
        Layout::default()
    } else {
        parse_layout_spec(&joined_layout)?
    };
    let document = if external_input.is_some() {
        inline_stream_labels
            .as_ref()
            .map(|labels| document_from_stream_labels(labels))
            .or(document)
    } else {
        document
    };
    let colors = cli
        .colors
        .as_deref()
        .map(split_color_tokens)
        .transpose()?
        .map(|tokens| expand_color_specs(&tokens))
        .transpose()?;

    Ok(Config {
        document,
        history,
        interval_ms: cli.interval_ms,
        align: cli.align,
        packed: cli.packed,
        label: cli.label,
        stream_labels: if !cli.stream_labels.is_empty() {
            Some(cli.stream_labels)
        } else {
            inline_stream_labels
        },
        stream_groups,
        stream_layout: cli.stream_layout,
        space: cli.space,
        layout_engine: cli.layout_engine,
        layout,
        renderer: cli.renderer,
        colors,
        color_mode: cli.color_mode,
        output_mode: cli.output_mode,
        width: cli.width,
        once: cli.once,
        force_stream_input,
        external_input,
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

fn resolve_args_with_config(args: Vec<String>) -> Result<Vec<String>, String> {
    if args.len() <= 1 || cli_requests_subcommand(&args) {
        return Ok(args);
    }

    let config_args = load_default_config_args()?;
    if config_args.is_empty() {
        return Ok(args);
    }

    let mut merged = Vec::with_capacity(args.len() + config_args.len());
    merged.push(args[0].clone());
    merged.extend(config_args);
    merged.extend(args.into_iter().skip(1));
    Ok(merged)
}

fn cli_requests_subcommand(args: &[String]) -> bool {
    args.get(1)
        .is_some_and(|arg| matches!(arg.as_str(), "completion" | "debug"))
}

fn load_default_config_args() -> Result<Vec<String>, String> {
    let Some(source) = default_config_source() else {
        return Ok(Vec::new());
    };
    load_config_args_from_path(&source.path, source.format)
}

fn load_config_args_from_path(
    path: &std::path::Path,
    format: ConfigFormat,
) -> Result<Vec<String>, String> {
    if !path.is_file() {
        return Ok(Vec::new());
    }

    let raw = fs::read_to_string(path)
        .map_err(|error| format!("failed to read config file {}: {error}", path.display()))?;
    match format {
        ConfigFormat::LegacyArgs => parse_config_contents(&raw),
        ConfigFormat::Toml => parse_toml_config_contents(&raw),
    }
    .map_err(|error| format!("invalid config file {}: {error}", path.display()))
}

fn default_config_source() -> Option<ConfigSource> {
    if let Ok(xdg_config_home) = env::var("XDG_CONFIG_HOME") {
        if !xdg_config_home.trim().is_empty() {
            let root = PathBuf::from(xdg_config_home).join("monlin");
            let toml_path = root.join("config.toml");
            if toml_path.is_file() {
                return Some(ConfigSource {
                    path: toml_path,
                    format: ConfigFormat::Toml,
                });
            }
            let legacy_path = root.join("config");
            if legacy_path.is_file() {
                return Some(ConfigSource {
                    path: legacy_path,
                    format: ConfigFormat::LegacyArgs,
                });
            }
        }
    }

    let home = env::var("HOME")
        .ok()
        .filter(|home| !home.trim().is_empty())?;
    let root = PathBuf::from(home).join(".config").join("monlin");
    let toml_path = root.join("config.toml");
    if toml_path.is_file() {
        return Some(ConfigSource {
            path: toml_path,
            format: ConfigFormat::Toml,
        });
    }
    let legacy_path = root.join("config");
    if legacy_path.is_file() {
        return Some(ConfigSource {
            path: legacy_path,
            format: ConfigFormat::LegacyArgs,
        });
    }
    None
}

fn parse_config_contents(raw: &str) -> Result<Vec<String>, String> {
    let mut args = Vec::new();

    for (index, line) in raw.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let parsed =
            shell_words::split(trimmed).map_err(|error| format!("line {}: {error}", index + 1))?;
        args.extend(parsed);
    }

    Ok(args)
}

fn parse_toml_config_contents(raw: &str) -> Result<Vec<String>, String> {
    let config = toml::from_str::<TomlConfig>(raw).map_err(|error| error.to_string())?;
    Ok(toml_config_to_args(config))
}

fn toml_config_to_args(config: TomlConfig) -> Vec<String> {
    let mut args = Vec::new();

    if let Some(layout) = non_empty_string(config.layout) {
        args.push(layout);
    }
    if let Some(history) = config.history {
        args.push("--history".to_owned());
        args.push(history.to_string());
    }
    if let Some(interval_ms) = config.interval_ms {
        args.push("--interval-ms".to_owned());
        args.push(interval_ms.to_string());
    }
    push_flag_value(&mut args, "--align", config.align);
    if config.packed.unwrap_or(false) {
        args.push("--packed".to_owned());
    }
    push_flag_value(&mut args, "--label", config.label);
    if let Some(labels) = config.stream_labels.and_then(StringList::into_csv) {
        args.push("--labels".to_owned());
        args.push(labels);
    }
    push_flag_value(&mut args, "--stream-layout", config.stream_layout);
    push_flag_value(&mut args, "--space", config.space);
    push_flag_value(&mut args, "--engine", config.layout_engine);
    push_flag_value(&mut args, "--renderer", config.renderer);
    if let Some(colors) = config.colors.and_then(StringList::into_csv) {
        args.push("--colors".to_owned());
        args.push(colors);
    }
    push_flag_value(&mut args, "--color", config.color_mode);
    push_flag_value(&mut args, "--output", config.output_mode);
    if let Some(width) = config.width {
        args.push("--width".to_owned());
        args.push(width.to_string());
    }
    if config.once.unwrap_or(false) {
        args.push("--once".to_owned());
    }

    args
}

fn push_flag_value(args: &mut Vec<String>, flag: &str, value: Option<String>) {
    if let Some(value) = non_empty_string(value) {
        args.push(flag.to_owned());
        args.push(value);
    }
}

fn non_empty_string(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_owned())
        }
    })
}

fn parse_external_input(
    raw: &str,
) -> Result<(Option<ExternalInputSource>, Option<Vec<String>>), String> {
    if let Some(source) = parse_external_input_source(raw)? {
        return Ok((Some(source), None));
    }

    let Some((label_part, source_part)) = raw.split_once('=') else {
        return Ok((None, None));
    };

    let Some(source) = parse_external_input_source(source_part)? else {
        return Ok((None, None));
    };

    let labels = label_part
        .split(',')
        .map(parse_stream_label)
        .collect::<Result<Vec<_>, _>>()?;
    if labels.is_empty() {
        return Err("external input labels must be non-empty".to_owned());
    }

    Ok((Some(source), Some(labels)))
}

fn parse_external_input_source(raw: &str) -> Result<Option<ExternalInputSource>, String> {
    if let Some(path) = raw.strip_prefix("f:") {
        let path = path.trim();
        if path.is_empty() {
            return Err("f: source path must be non-empty".to_owned());
        }
        return Ok(Some(ExternalInputSource::File(PathBuf::from(path))));
    }

    if let Some(command) = raw.strip_prefix("p:") {
        let command = command.trim();
        if command.is_empty() {
            return Err("p: source command must be non-empty".to_owned());
        }
        return Ok(Some(ExternalInputSource::Process(command.to_owned())));
    }

    Ok(None)
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

fn expand_color_specs(tokens: &[String]) -> Result<Vec<ColorSpec>, String> {
    let mut specs = Vec::new();

    for token in tokens {
        if let Some(map) = named_colormap(token.trim()) {
            if tokens.len() != 1 {
                return Err("named colormaps cannot be mixed with other color specs".to_owned());
            }
            specs.push(ColorSpec::Map(map));
        } else if let Some(palette) = named_palette(token.trim()) {
            specs.extend(palette);
        } else {
            specs.push(parse_color_spec(token)?);
        }
    }

    Ok(specs)
}

fn split_color_tokens(raw: &str) -> Result<Vec<String>, String> {
    let tokens = raw
        .split(',')
        .map(|token| token.trim())
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();

    if tokens.is_empty() {
        return Err("at least one color token is required".to_owned());
    }

    Ok(tokens)
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

fn document_from_stream_labels(labels: &[String]) -> Document {
    Document::new(
        vec![Row::new(
            None,
            labels
                .iter()
                .enumerate()
                .map(|(index, label)| {
                    Item::stream_column(
                        Some(label.clone()),
                        index,
                        DisplayMode::Full,
                        1,
                        None,
                        None,
                    )
                })
                .collect(),
        )],
        false,
        false,
    )
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
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn parse(items: &[&str]) -> Config {
        parse_args_from_vec(items.iter().map(|item| item.to_string()).collect()).unwrap()
    }

    fn unique_temp_dir(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("monlin-{name}-{nonce}"))
    }

    #[test]
    fn defaults_to_avail_layout() {
        let config = parse(&["monlin"]);
        assert_eq!(
            config.layout.metrics(),
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
        assert_eq!(config.layout.rows().len(), 1);
        assert!(config.layout.filter_available());
        assert_eq!(config.output_mode, OutputMode::Terminal);
    }

    #[test]
    fn parses_layout_from_positionals() {
        let config = parse(&["monlin", "cpu", "net"]);
        assert_eq!(config.layout.metrics(), &[MetricKind::Cpu, MetricKind::Net]);
    }

    #[test]
    fn parses_shell_split_config_contents() {
        let args = parse_config_contents(
            r#"
            # comment
            --align right
            --label "my host"
            -p
            "#,
        )
        .unwrap();
        assert_eq!(args, vec!["--align", "right", "--label", "my host", "-p"]);
    }

    #[test]
    fn parses_toml_config_contents() {
        let args = parse_toml_config_contents(
            r#"
            align = "right"
            label = "my host"
            packed = true
            colors = ["gruvbox", "320"]
            renderer = "block"
            "#,
        )
        .unwrap();
        assert_eq!(
            args,
            vec![
                "--align",
                "right",
                "--packed",
                "--label",
                "my host",
                "--renderer",
                "block",
                "--colors",
                "gruvbox,320",
            ]
        );
    }

    #[test]
    fn loads_default_args_from_xdg_config_file() {
        let root = unique_temp_dir("xdg");
        let config_dir = root.join("monlin");
        let path = config_dir.join("config");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(&path, "--align right\n-p\n").unwrap();

        let args = load_config_args_from_path(&path, ConfigFormat::LegacyArgs).unwrap();
        fs::remove_dir_all(&root).ok();

        assert_eq!(args, vec!["--align", "right", "-p"]);
    }

    #[test]
    fn loads_toml_args_from_config_file() {
        let root = unique_temp_dir("toml");
        let config_dir = root.join("monlin");
        let path = config_dir.join("config.toml");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(
            &path,
            r#"
            colors = "gruvbox"
            renderer = "block"
            "#,
        )
        .unwrap();

        let args = load_config_args_from_path(&path, ConfigFormat::Toml).unwrap();
        fs::remove_dir_all(&root).ok();

        assert_eq!(args, vec!["--renderer", "block", "--colors", "gruvbox"]);
    }

    #[test]
    fn command_line_flags_override_config_file_defaults() {
        let mut args = vec!["monlin".to_owned()];
        args.extend(parse_config_contents("--align right\n").unwrap());
        args.extend(vec!["--align".to_owned(), "left".to_owned()]);
        let config = parse_args_from_vec(args).unwrap();
        assert_eq!(config.align, Align::Left);
    }

    #[test]
    fn command_line_colors_override_config_file_colors() {
        let mut args = vec!["monlin".to_owned()];
        args.extend(parse_config_contents("--colors rainbow\n").unwrap());
        args.extend(vec!["--colors".to_owned(), "turbo".to_owned()]);
        let config = parse_args_from_vec(args).unwrap();
        assert_eq!(
            config.colors,
            Some(vec![ColorSpec::Map(crate::color::ColorMapKind::Turbo)])
        );
    }

    #[test]
    fn command_line_flags_override_toml_config_file_defaults() {
        let mut args = vec!["monlin".to_owned()];
        args.extend(
            parse_toml_config_contents(
                r#"
                align = "right"
                renderer = "block"
                "#,
            )
            .unwrap(),
        );
        args.extend(vec![
            "--align".to_owned(),
            "left".to_owned(),
            "--renderer".to_owned(),
            "braille".to_owned(),
        ]);
        let config = parse_args_from_vec(args).unwrap();
        assert_eq!(config.align, Align::Left);
        assert_eq!(config.renderer, Renderer::Braille);
    }

    #[test]
    fn skips_config_file_for_subcommands() {
        let args = resolve_args_with_config(vec![
            "monlin".to_owned(),
            "completion".to_owned(),
            "zsh".to_owned(),
        ])
        .unwrap();
        assert_eq!(args, vec!["monlin", "completion", "zsh"]);
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
    fn parses_short_interval_flag() {
        let config = parse(&["monlin", "-i", "250"]);
        assert_eq!(config.interval_ms, 250);
    }

    #[test]
    fn parses_completion_shell() {
        let config = parse(&["monlin", "completion", "zsh"]);
        assert_eq!(config.print_completion, Some(CompletionShell::Zsh));
    }

    #[test]
    fn parses_color_completion_target() {
        let config = parse(&["monlin", "completion", "colors"]);
        assert_eq!(config.print_completion, Some(CompletionShell::Colors));
    }

    #[test]
    fn parses_stream_labels() {
        let config = parse(&["monlin", "--labels", "wifi,eth,vpn"]);
        assert_eq!(
            config.stream_labels,
            Some(vec!["wifi".to_owned(), "eth".to_owned(), "vpn".to_owned()])
        );
    }

    #[test]
    fn parses_stream_layout() {
        let config = parse(&["monlin", "--stream-layout", "lines"]);
        assert_eq!(config.stream_layout, StreamLayout::Lines);
    }

    #[test]
    fn parses_packed_flag() {
        let config = parse(&["monlin", "-p"]);
        assert!(config.packed);
    }

    #[test]
    fn parses_space_mode() {
        let config = parse(&["monlin", "--space", "segment"]);
        assert_eq!(config.space, Space::Segment);
    }

    #[test]
    fn parses_layout_engine() {
        let config = parse(&["monlin", "--engine", "grid"]);
        assert_eq!(config.layout_engine, LayoutEngine::Grid);
    }

    #[test]
    fn parses_flex_layout_engine() {
        let config = parse(&["monlin", "--engine", "flex"]);
        assert_eq!(config.layout_engine, LayoutEngine::Flex);
    }

    #[test]
    fn parses_pack_layout_engine() {
        let config = parse(&["monlin", "--engine", "pack"]);
        assert_eq!(config.layout_engine, LayoutEngine::Pack);
    }

    #[test]
    fn defaults_to_auto_layout_engine() {
        let config = parse(&["monlin"]);
        assert_eq!(config.layout_engine, LayoutEngine::Auto);
    }

    #[test]
    fn parses_file_input_source() {
        let config = parse(&["monlin", "f:/tmp/data.log"]);
        assert_eq!(
            config.external_input,
            Some(ExternalInputSource::File(PathBuf::from("/tmp/data.log")))
        );
    }

    #[test]
    fn parses_process_input_source() {
        let config = parse(&["monlin", "p:printf '10 20\\n'"]);
        assert_eq!(
            config.external_input,
            Some(ExternalInputSource::Process("printf '10 20\\n'".to_owned()))
        );
    }

    #[test]
    fn parses_labeled_file_input_source() {
        let config = parse(&["monlin", "cpu,ram=f:/tmp/data.log"]);
        assert_eq!(
            config.external_input,
            Some(ExternalInputSource::File(PathBuf::from("/tmp/data.log")))
        );
        assert_eq!(
            config.stream_labels,
            Some(vec!["cpu".to_owned(), "ram".to_owned()])
        );
    }

    #[test]
    fn parses_labeled_process_input_source() {
        let config = parse(&["monlin", "cpu=p:printf '10\\n'"]);
        assert_eq!(
            config.external_input,
            Some(ExternalInputSource::Process("printf '10\\n'".to_owned()))
        );
        assert_eq!(config.stream_labels, Some(vec!["cpu".to_owned()]));
    }

    #[test]
    fn rejects_combining_inline_external_labels_with_flag_labels() {
        let error = parse_args(
            ["monlin", "--labels", "a,b", "cpu,ram=p:printf '10 20\\n'"]
                .into_iter()
                .map(str::to_owned),
        )
        .unwrap_err();
        assert!(error.contains("cannot be combined with --labels"));
    }

    #[test]
    fn parses_stream_column_layout_items() {
        let config = parse(&["monlin", "cpu=@1", "ram=@2"]);
        let document = config.document.as_ref().unwrap();
        assert_eq!(config.stream_groups, None);
        assert_eq!(document.rows().len(), 1);
        assert_eq!(document.rows()[0].items()[0].label(), Some("cpu"));
        assert_eq!(
            document.rows()[0].items()[0].source(),
            &crate::layout::Source::StreamColumn(0)
        );
    }

    #[test]
    fn parses_labeled_stream_groups() {
        let config = parse(&["monlin", "\"two randoms\"=(\"1st program\"=@1, 2nd=@2)"]);
        let document = config.document.as_ref().unwrap();
        assert_eq!(config.stream_groups, None);
        assert_eq!(document.rows().len(), 2);
        assert_eq!(document.rows()[0].label(), Some("two randoms"));
        assert_eq!(document.rows()[1].label(), Some("two randoms"));
    }

    #[test]
    fn parses_native_layout_document() {
        let config = parse(&["monlin", "cpu", "ram.free"]);
        let document = config.document.as_ref().unwrap();
        assert_eq!(document.rows().len(), 1);
        assert!(document.is_native_only());
        assert_eq!(
            document.rows()[0].items()[0].source(),
            &crate::layout::Source::Metric(MetricKind::Cpu)
        );
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
    fn parses_named_palette_colors() {
        let config = parse(&["monlin", "--colors", "default"]);
        assert_eq!(
            config.colors,
            Some(vec![
                ColorSpec::Angle(20.0),
                ColorSpec::Angle(65.0),
                ColorSpec::Angle(110.0),
                ColorSpec::Angle(155.0),
                ColorSpec::Angle(200.0),
                ColorSpec::Angle(245.0),
                ColorSpec::Angle(290.0),
                ColorSpec::Angle(335.0),
            ])
        );
    }

    #[test]
    fn parses_named_palette_mixed_with_explicit_colors() {
        let config = parse(&["monlin", "--colors", "warm,320"]);
        assert_eq!(config.colors.as_ref().unwrap().len(), 7);
        assert_eq!(config.colors.as_ref().unwrap()[6], ColorSpec::Angle(320.0));
    }

    #[test]
    fn split_color_tokens_rejects_empty_input() {
        let error = split_color_tokens(" , ").unwrap_err();
        assert!(error.contains("at least one color token"));
    }

    #[test]
    fn parses_named_colormap_colors() {
        let config = parse(&["monlin", "--colors", "turbo"]);
        assert_eq!(
            config.colors,
            Some(vec![ColorSpec::Map(crate::color::ColorMapKind::Turbo)])
        );
    }

    #[test]
    fn rejects_mixing_named_colormap_with_other_specs() {
        let error = parse_args(
            ["monlin", "--colors", "turbo,20"]
                .into_iter()
                .map(|item| item.to_string()),
        )
        .unwrap_err();
        assert!(error.contains("named colormaps cannot be mixed"));
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
        assert!(help.contains("metric[.view][:size][+max][-min]"));
        assert!(help.contains("Rows can be separated with ',' or a literal newline."));
        assert!(help.contains("named palette like gruvbox or solarized"));
    }

    #[test]
    fn palette_names_are_known_to_config_layer() {
        assert!(crate::color::palette_names().contains(&"default"));
        assert!(crate::color::palette_names().contains(&"rainbow"));
        assert!(crate::color::palette_names().contains(&"gruvbox"));
        assert!(crate::color::palette_names().contains(&"solarized"));
        assert!(crate::color::colormap_names().contains(&"turbo"));
    }
}
