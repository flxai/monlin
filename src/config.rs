use crate::layout::{parse_layout_spec, Layout};
use crate::render::Renderer;

const DEFAULT_INTERVAL_MS: u64 = 1000;
const DEFAULT_HISTORY: usize = 512;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Align {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ColorMode {
    Auto,
    Always,
    Never,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputMode {
    Terminal,
    I3bar,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub history: usize,
    pub interval_ms: u64,
    pub align: Align,
    pub label: Option<String>,
    pub layout: Layout,
    pub renderer: Renderer,
    pub color_mode: ColorMode,
    pub output_mode: OutputMode,
    pub width: Option<usize>,
    pub once: bool,
    pub show_help: bool,
}

pub fn parse_args<I>(args: I) -> Result<Config, String>
where
    I: IntoIterator<Item = String>,
{
    let args = args.into_iter().collect::<Vec<_>>();
    let mut history = DEFAULT_HISTORY;
    let mut interval_ms = DEFAULT_INTERVAL_MS;
    let mut align = Align::Left;
    let mut label = None;
    let mut layout_spec = None;
    let mut renderer = Renderer::Braille;
    let mut color_mode = ColorMode::Auto;
    let mut output_mode = OutputMode::Terminal;
    let mut width = None;
    let mut once = false;
    let mut show_help = false;
    let mut positionals = Vec::new();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                show_help = true;
                i += 1;
            }
            "--history" => {
                history = parse_value(&args, &mut i, "--history")?;
            }
            "--interval-ms" => {
                interval_ms = parse_value(&args, &mut i, "--interval-ms")?;
            }
            "--align" => {
                align = match parse_string(&args, &mut i, "--align")?.as_str() {
                    "left" => Align::Left,
                    "right" => Align::Right,
                    other => return Err(format!("invalid --align value: {other}")),
                };
            }
            "--label" => {
                label = Some(parse_string(&args, &mut i, "--label")?);
            }
            "-l" | "--layout" => {
                layout_spec = Some(parse_string(&args, &mut i, "--layout")?);
            }
            "--renderer" => {
                renderer = match parse_string(&args, &mut i, "--renderer")?.as_str() {
                    "braille" => Renderer::Braille,
                    "block" => Renderer::Block,
                    other => return Err(format!("invalid --renderer value: {other}")),
                };
            }
            "--color" => {
                color_mode = match parse_string(&args, &mut i, "--color")?.as_str() {
                    "auto" => ColorMode::Auto,
                    "always" => ColorMode::Always,
                    "never" => ColorMode::Never,
                    other => return Err(format!("invalid --color value: {other}")),
                };
            }
            "--output" => {
                output_mode = match parse_string(&args, &mut i, "--output")?.as_str() {
                    "terminal" => OutputMode::Terminal,
                    "i3bar" => OutputMode::I3bar,
                    other => return Err(format!("invalid --output value: {other}")),
                };
            }
            "--width" => {
                width = Some(parse_value(&args, &mut i, "--width")?);
            }
            "--once" => {
                once = true;
                i += 1;
            }
            value if value.starts_with('-') => {
                return Err(format!("unknown flag: {value}"));
            }
            value => {
                positionals.push(value.to_owned());
                i += 1;
            }
        }
    }

    if history == 0 {
        history = 1;
    }
    if let Some(value) = width {
        if value == 0 {
            return Err("--width must be greater than zero".to_owned());
        }
    }
    if layout_spec.is_some() && !positionals.is_empty() {
        return Err("use either positional metrics or --layout, not both".to_owned());
    }

    let layout = if let Some(spec) = layout_spec {
        parse_layout_spec(&spec)?
    } else if positionals.is_empty() {
        Layout::default()
    } else {
        parse_layout_spec(&positionals.join(" "))?
    };

    Ok(Config {
        history,
        interval_ms,
        align,
        label,
        layout,
        renderer,
        color_mode,
        output_mode,
        width,
        once,
        show_help,
    })
}

fn parse_value<T: std::str::FromStr>(
    args: &[String],
    i: &mut usize,
    flag: &str,
) -> Result<T, String> {
    let value = parse_string(args, i, flag)?;
    value
        .parse::<T>()
        .map_err(|_| format!("invalid value for {flag}: {value}"))
}

fn parse_string(args: &[String], i: &mut usize, flag: &str) -> Result<String, String> {
    let next = *i + 1;
    let value = args
        .get(next)
        .cloned()
        .ok_or_else(|| format!("missing value for {flag}"))?;
    *i += 2;
    Ok(value)
}

pub fn help_text() -> &'static str {
    "\
Usage: monlin --layout SPEC [OPTIONS]
       monlin [LEGACY_METRIC...]

Metrics:
  cpu sys gpu vram gfx memory spc io net ingress egress
  all[/N]

Options:
  -l, --layout SPEC     Primary layout specification, e.g. \"sys gfx io net\" or \"all/2\"
  --history N           Number of history samples to retain
  --interval-ms N       Sampling interval in milliseconds
  --align left|right    Place the percentage before or after the graph
  --label TEXT          Prefix the entire rendered line
  --renderer braille|block
  --color auto|always|never
  --output terminal|i3bar
  --width N             Override terminal width
  --once                Render once and exit
  -h, --help            Show this help text

Notes:
  Layout is the canonical interface.
  Item syntax is metric[.view][:basis][+grow], e.g. net.hum:12+2.
  Rows can be separated with ';' or a literal newline.
  Flat layouts auto-wrap after 5 metrics per row.
  Positional metrics are kept only as a compatibility path for older invocations.
"
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
    fn parses_positional_metrics() {
        let config = parse(&["monlin", "cpu", "net"]);
        assert_eq!(config.layout.metrics(), &[MetricKind::Cpu, MetricKind::Net]);
    }

    #[test]
    fn rejects_layout_and_positionals_together() {
        let error = parse_args(
            ["monlin", "--layout", "cpu gpu", "memory"]
                .into_iter()
                .map(|item| item.to_string()),
        )
        .unwrap_err();
        assert!(error.contains("either positional metrics or --layout"));
    }

    #[test]
    fn accepts_short_layout_flag() {
        let config = parse(&["monlin", "-l", "all"]);
        assert_eq!(config.layout.metrics(), crate::layout::all_metrics());
        assert_eq!(config.layout.rows().len(), 2);
    }

    #[test]
    fn all_two_rows_is_accepted() {
        let config = parse(&["monlin", "--layout", "all/2"]);
        assert_eq!(config.layout.rows().len(), 2);
    }

    #[test]
    fn parses_i3bar_output_mode() {
        let config = parse(&["monlin", "--output", "i3bar"]);
        assert_eq!(config.output_mode, OutputMode::I3bar);
    }
}
