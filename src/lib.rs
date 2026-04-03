pub mod color;
pub mod config;
pub mod layout;
pub mod metrics;
pub mod render;

use std::collections::{HashMap, VecDeque};
#[cfg(unix)]
use std::ffi::c_void;
use std::io::{self, BufRead, IsTerminal, Write};
use std::thread;
use std::time::Duration;

use clap_complete::{generate, Generator, Shell};
use config::{ColorMode, OutputMode};
use layout::MetricKind;
use metrics::MetricValue;
use serde_json::json;

const HIDE_CURSOR: &str = "\x1b[?25l";
const SHOW_CURSOR: &str = "\x1b[?25h";

#[cfg(unix)]
const CURSOR_RESTORE_SIGNALS: [i32; 4] = [1, 2, 3, 15];
#[cfg(unix)]
const STDOUT_FILENO: i32 = 1;

pub fn run(args: Vec<String>) -> Result<(), String> {
    let config = config::parse_args(args)?;
    if config.show_help {
        print!("{}", config::help_text());
        return Ok(());
    }
    if let Some(shell) = config.print_completion {
        print_completion(shell);
        return Ok(());
    }
    if let Some(steps) = config.debug_colors_steps {
        print_debug_colors(
            steps,
            config.colors.as_deref(),
            colors_enabled(config.color_mode),
        );
        return Ok(());
    }

    if let Some(result) = maybe_run_stream(&config)? {
        return result;
    }

    run_native(&config)
}

fn print_completion(shell: config::CompletionShell) {
    let mut command = config::clap_command();
    match shell {
        config::CompletionShell::Bash => generate_to_stdout(Shell::Bash, &mut command),
        config::CompletionShell::Elvish => generate_to_stdout(Shell::Elvish, &mut command),
        config::CompletionShell::Fish => generate_to_stdout(Shell::Fish, &mut command),
        config::CompletionShell::PowerShell => {
            generate_to_stdout(Shell::PowerShell, &mut command)
        }
        config::CompletionShell::Zsh => print!("{}", zsh_completion_script()),
    }
}

fn generate_to_stdout<G: Generator>(generator: G, command: &mut clap::Command) {
    generate(generator, command, "monlin", &mut io::stdout());
}

fn zsh_completion_script() -> &'static str {
    r#"#compdef monlin

_monlin_layout() {
  local cur token prefix base
  local -a metrics
  metrics=(
    cpu
    sys
    gpu
    vram
    gfx
    memory
    mem
    ram
    storage
    disk
    space
    spc
    io
    net
    ingress
    in
    egress
    out
    all
  )

  cur="${words[CURRENT]}"
  token="${cur##*,}"
  prefix="${cur%$token}"

  if [[ "$token" == *.* ]]; then
    base="${token%%.*}"
    case "$base" in
      cpu|sys|gpu|vram|gfx|memory|mem|ram|storage|disk|space|spc|io|net|ingress|in|egress|out)
        compadd -Q -P "${prefix}${base}." -- pct hum free
        return
        ;;
    esac
  fi

  if [[ -n "$prefix" ]]; then
    compadd -Q -P "$prefix" -- $metrics
    return
  fi

  _describe -t metrics 'layout item' \
    'cpu:CPU usage' \
    'sys:CPU and RAM split' \
    'gpu:GPU utilization' \
    'vram:VRAM usage' \
    'gfx:GPU and VRAM split' \
    'memory:RAM usage' \
    'mem:RAM usage' \
    'ram:RAM usage' \
    'storage:Storage usage' \
    'disk:Storage usage' \
    'space:Storage usage' \
    'spc:Storage usage' \
    'io:Disk I/O split' \
    'net:Network traffic split' \
    'ingress:Network ingress' \
    'in:Network ingress' \
    'egress:Network egress' \
    'out:Network egress' \
    'all:Canonical multi-row layout'
}

_monlin() {
  local cur prev
  cur="${words[CURRENT]}"
  prev="${words[CURRENT-1]}"

  local -a opts commands debug_commands shells
  opts=(
    '--history:Number of history samples to retain'
    '--interval-ms:Sampling interval in milliseconds'
    '--align:Place the value before or after the graph'
    '--label:Prefix every rendered line with a label'
    '--labels:Comma-separated labels for stdin stream columns'
    '--stream-layout:Render streamed stdin as columns or lines'
    '--space:How streamed columns allocate width'
    '--renderer:Graph renderer to use'
    '-c:Comma-separated visible-order colors: angle 20 or A20, RGB Rff8800/Lff8800, or packed LCh L086078020/R086078020'
    '--colors:Comma-separated visible-order colors: angle 20 or A20, RGB Rff8800/Lff8800, or packed LCh L086078020/R086078020'
    '--color:When to emit ANSI colors'
    '--output:Output protocol to render'
    '--width:Override the render width'
    '--once:Render one frame and exit'
    '-h:Show help text'
    '--help:Show help text'
    '--steps:Number of glyph samples to print per metric'
  )
  commands=(completion debug)
  debug_commands=(colors)
  shells=(bash elvish fish powershell zsh)

  case "$prev" in
    --align)
      compadd left right
      return
      ;;
    --stream-layout)
      compadd columns lines
      return
      ;;
    --space)
      compadd graph segment
      return
      ;;
    --renderer)
      compadd braille block
      return
      ;;
    --color)
      compadd auto always never
      return
      ;;
    --output)
      compadd terminal i3bar
      return
      ;;
    completion)
      compadd $shells
      return
      ;;
    debug)
      compadd $debug_commands
      return
      ;;
  esac

  if [[ "${words[2]}" == "completion" ]]; then
    if (( CURRENT == 3 )); then
      compadd $shells
    fi
    return
  fi

  if [[ "${words[2]}" == "debug" ]]; then
    if (( CURRENT == 3 )); then
      compadd $debug_commands
      return
    fi
    if [[ "$cur" == -* ]]; then
      _describe -t options 'option' $opts
    fi
    return
  fi

  if [[ "$cur" == -* ]]; then
    _describe -t options 'option' $opts
    return
  fi

  if (( CURRENT == 2 )); then
    _describe -t commands 'command' \
      'completion:Generate shell completion scripts' \
      'debug:Debug rendering helpers'
  fi
  _monlin_layout
}

_monlin "$@"
"#
}

fn print_debug_colors(
    steps: usize,
    colors: Option<&[crate::color::ColorSpec]>,
    color_enabled: bool,
) {
    const METRICS: &[(MetricKind, &str)] = &[
        (MetricKind::Cpu, "cpu"),
        (MetricKind::Sys, "sys"),
        (MetricKind::Memory, "ram"),
        (MetricKind::Gpu, "gpu"),
        (MetricKind::Gfx, "gfx"),
        (MetricKind::Storage, "spc"),
        (MetricKind::Io, "io"),
        (MetricKind::Ingress, "in"),
        (MetricKind::Egress, "out"),
        (MetricKind::Net, "net"),
    ];

    let width = steps.max(1);
    let effective_hues = crate::color::visible_hues(METRICS.len(), colors);
    for (index, (metric, label)) in METRICS.iter().enumerate() {
        let samples = (0..width)
            .map(|index| {
                if width == 1 {
                    1.0
                } else {
                    index as f64 / (width - 1) as f64
                }
            })
            .map(MetricValue::Single)
            .collect::<Vec<_>>();
        let item_hues =
            crate::color::metric_hues_for_visible_hue(*metric, effective_hues[index]);
        let graph =
            render::render_braille_graph(&samples, width, *metric, Some(&item_hues), color_enabled);
        println!("{label:<4} {graph}");
    }
}

fn run_native(config: &config::Config) -> Result<(), String> {
    let requested_metrics = config.layout.metrics();
    let mut sampler = metrics::Sampler::default();
    sampler.prime(requested_metrics).map_err(monlin_error)?;

    let mut histories = init_histories(requested_metrics, config.history);
    match config.output_mode {
        OutputMode::Terminal => run_terminal(&config, &mut sampler, &mut histories),
        OutputMode::I3bar => run_i3bar(&config, &mut sampler, &mut histories),
    }
}

fn maybe_run_stream(config: &config::Config) -> Result<Option<Result<(), String>>, String> {
    let should_probe = config.force_stream_input || !io::stdin().is_terminal();
    if !should_probe {
        return Ok(None);
    }

    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let first_frame = match read_next_stream_frame(&mut reader, None)? {
        Some(frame) => frame,
        None => {
            if config.force_stream_input {
                return Ok(Some(Ok(())));
            }
            return Ok(None);
        }
    };
    validate_stream_labels(config, first_frame.len())?;

    let result = match config.output_mode {
        OutputMode::Terminal => run_stream_terminal(config, &mut reader, first_frame),
        OutputMode::I3bar => run_stream_i3bar(config, &mut reader, first_frame),
    };
    Ok(Some(result))
}

fn validate_stream_labels(config: &config::Config, series_count: usize) -> Result<(), String> {
    if let Some(labels) = &config.stream_labels {
        if labels.len() != series_count {
            return Err(format!(
                "--labels expected {series_count} entries for the input stream, got {}",
                labels.len()
            ));
        }
    }

    Ok(())
}

fn run_terminal(
    config: &config::Config,
    sampler: &mut metrics::Sampler,
    histories: &mut HashMap<MetricKind, VecDeque<MetricValue>>,
) -> Result<(), String> {
    let hide_cursor = io::stdout().is_terminal();
    let mut stdout = io::stdout().lock();
    let _cursor_guard =
        CursorVisibilityGuard::new(&mut stdout, hide_cursor).map_err(io_to_string)?;
    let mut rendered_rows = 0;

    loop {
        if config.interval_ms > 0 {
            thread::sleep(Duration::from_millis(config.interval_ms));
        }

        let lines = sample_lines(config, sampler, histories)?;

        rendered_rows =
            write_terminal_frame(&mut stdout, &lines, rendered_rows).map_err(io_to_string)?;
        stdout.flush().map_err(io_to_string)?;

        if config.once {
            writeln!(stdout).map_err(io_to_string)?;
            break;
        }
    }

    Ok(())
}

fn run_stream_terminal<R: BufRead>(
    config: &config::Config,
    reader: &mut R,
    first_frame: Vec<f64>,
) -> Result<(), String> {
    let hide_cursor = io::stdout().is_terminal();
    let mut stdout = io::stdout().lock();
    let _cursor_guard =
        CursorVisibilityGuard::new(&mut stdout, hide_cursor).map_err(io_to_string)?;
    let mut rendered_rows = 0;
    let mut histories = init_stream_histories(first_frame.len(), config.history);
    let width = config
        .width
        .or_else(render::terminal_width)
        .unwrap_or(80)
        .max(16);
    let color_enabled = colors_enabled(config.color_mode);

    let mut current = first_frame;
    loop {
        update_stream_histories(&mut histories, &current, config.history);
        let lines = render::render_stream_lines(config, width, color_enabled, &histories, &current);
        rendered_rows =
            write_terminal_frame(&mut stdout, &lines, rendered_rows).map_err(io_to_string)?;
        stdout.flush().map_err(io_to_string)?;

        if config.once {
            writeln!(stdout).map_err(io_to_string)?;
            break;
        }

        match read_next_stream_frame(reader, Some(current.len()))? {
            Some(frame) => current = frame,
            None => break,
        }
    }

    Ok(())
}

struct CursorVisibilityGuard {
    restore: bool,
    #[cfg(unix)]
    previous_handlers: Option<[SignalHandler; CURSOR_RESTORE_SIGNALS.len()]>,
}

impl CursorVisibilityGuard {
    fn new<W: Write>(stdout: &mut W, hide_cursor: bool) -> io::Result<Self> {
        #[cfg(unix)]
        let previous_handlers = hide_cursor.then(install_cursor_restore_signal_handlers);

        if hide_cursor {
            write_cursor_visibility(stdout, false)?;
            stdout.flush()?;
        }

        Ok(Self {
            restore: hide_cursor,
            #[cfg(unix)]
            previous_handlers,
        })
    }
}

impl Drop for CursorVisibilityGuard {
    fn drop(&mut self) {
        if !self.restore {
            return;
        }

        #[cfg(unix)]
        if let Some(previous_handlers) = self.previous_handlers {
            restore_signal_handlers(previous_handlers);
        }

        let mut stdout = io::stdout().lock();
        let _ = write_cursor_visibility(&mut stdout, true);
        let _ = stdout.flush();
    }
}

#[cfg(unix)]
type SignalHandler = usize;

#[cfg(unix)]
const SIG_DFL: SignalHandler = 0;

#[cfg(unix)]
unsafe extern "C" {
    fn signal(signum: i32, handler: SignalHandler) -> SignalHandler;
    fn write(fd: i32, buf: *const c_void, count: usize) -> isize;
    fn _exit(status: i32) -> !;
}

#[cfg(unix)]
unsafe extern "C" fn restore_cursor_and_exit(signum: i32) {
    let bytes = SHOW_CURSOR.as_bytes();
    let _ = unsafe { write(STDOUT_FILENO, bytes.as_ptr().cast(), bytes.len()) };
    unsafe { _exit(128 + signum) }
}

#[cfg(unix)]
fn install_cursor_restore_signal_handlers() -> [SignalHandler; CURSOR_RESTORE_SIGNALS.len()] {
    let mut previous_handlers = [SIG_DFL; CURSOR_RESTORE_SIGNALS.len()];

    for (index, signal_number) in CURSOR_RESTORE_SIGNALS.iter().copied().enumerate() {
        previous_handlers[index] = unsafe {
            signal(
                signal_number,
                restore_cursor_and_exit as *const () as SignalHandler,
            )
        };
    }

    previous_handlers
}

#[cfg(unix)]
fn restore_signal_handlers(previous_handlers: [SignalHandler; CURSOR_RESTORE_SIGNALS.len()]) {
    for (signal_number, previous_handler) in CURSOR_RESTORE_SIGNALS
        .iter()
        .copied()
        .zip(previous_handlers)
    {
        unsafe {
            signal(signal_number, previous_handler);
        }
    }
}

fn write_terminal_frame<W: Write>(
    stdout: &mut W,
    lines: &[String],
    previous_rows: usize,
) -> io::Result<usize> {
    if previous_rows > 0 {
        write!(stdout, "\r")?;
        if previous_rows > 1 {
            write!(stdout, "\x1b[{}A", previous_rows - 1)?;
        }
    }

    for (index, line) in lines.iter().enumerate() {
        write!(stdout, "{line}\x1b[K")?;
        if index + 1 < lines.len() {
            writeln!(stdout)?;
        }
    }

    let stale_rows = previous_rows.saturating_sub(lines.len());
    if stale_rows > 0 {
        for _ in 0..stale_rows {
            writeln!(stdout)?;
            write!(stdout, "\x1b[K")?;
        }
        write!(stdout, "\r\x1b[{}A", stale_rows)?;
    }

    Ok(lines.len())
}

fn write_cursor_visibility<W: Write>(stdout: &mut W, visible: bool) -> io::Result<()> {
    let sequence = if visible { SHOW_CURSOR } else { HIDE_CURSOR };
    write!(stdout, "{sequence}")
}

fn run_i3bar(
    config: &config::Config,
    sampler: &mut metrics::Sampler,
    histories: &mut HashMap<MetricKind, VecDeque<MetricValue>>,
) -> Result<(), String> {
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{}", json!({"version": 1, "click_events": false})).map_err(io_to_string)?;
    writeln!(stdout, "[").map_err(io_to_string)?;

    let mut first = true;
    loop {
        if config.interval_ms > 0 {
            thread::sleep(Duration::from_millis(config.interval_ms));
        }

        let lines = sample_lines(config, sampler, histories)?;
        let frame = i3bar_frame_json(&lines).to_string();

        if !first {
            writeln!(stdout, ",{frame}").map_err(io_to_string)?;
        } else {
            writeln!(stdout, "{frame}").map_err(io_to_string)?;
            first = false;
        }
        stdout.flush().map_err(io_to_string)?;

        if config.once {
            writeln!(stdout, "]").map_err(io_to_string)?;
            break;
        }
    }

    Ok(())
}

fn run_stream_i3bar<R: BufRead>(
    config: &config::Config,
    reader: &mut R,
    first_frame: Vec<f64>,
) -> Result<(), String> {
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{}", json!({"version": 1, "click_events": false})).map_err(io_to_string)?;
    writeln!(stdout, "[").map_err(io_to_string)?;

    let mut first = true;
    let mut histories = init_stream_histories(first_frame.len(), config.history);
    let width = config
        .width
        .or_else(render::terminal_width)
        .unwrap_or(80)
        .max(16);

    let mut current = first_frame;
    loop {
        update_stream_histories(&mut histories, &current, config.history);
        let lines = render::render_stream_lines(config, width, false, &histories, &current);
        let frame = i3bar_frame_json(&lines).to_string();

        if !first {
            writeln!(stdout, ",{frame}").map_err(io_to_string)?;
        } else {
            writeln!(stdout, "{frame}").map_err(io_to_string)?;
            first = false;
        }
        stdout.flush().map_err(io_to_string)?;

        if config.once {
            writeln!(stdout, "]").map_err(io_to_string)?;
            break;
        }

        match read_next_stream_frame(reader, Some(current.len()))? {
            Some(frame) => current = frame,
            None => {
                writeln!(stdout, "]").map_err(io_to_string)?;
                break;
            }
        }
    }

    Ok(())
}

fn sample_lines(
    config: &config::Config,
    sampler: &mut metrics::Sampler,
    histories: &mut HashMap<MetricKind, VecDeque<MetricValue>>,
) -> Result<Vec<String>, String> {
    let requested_metrics = config.layout.metrics();
    let sample = sampler.sample(requested_metrics).map_err(monlin_error)?;
    let values = &sample.values;
    let headlines = &sample.headlines;

    for metric in requested_metrics {
        if let Some(history) = histories.get_mut(metric) {
            if let Some(value) = values.get(metric).copied() {
                if history.len() == config.history.max(1) {
                    history.pop_front();
                }
                history.push_back(value);
            } else {
                history.clear();
            }
        }
    }

    let width = config
        .width
        .or_else(render::terminal_width)
        .unwrap_or(80)
        .max(16);
    let color_enabled = match config.output_mode {
        OutputMode::I3bar => false,
        OutputMode::Terminal => colors_enabled(config.color_mode),
    };
    let active_layout = config
        .layout
        .retain_available(|metric| values.contains_key(&metric));
    let mut lines = render::render_lines_with_headlines(
        config,
        width,
        color_enabled,
        histories,
        &active_layout,
        values,
        headlines,
    );
    if lines.is_empty() {
        lines.push(String::new());
    }

    Ok(lines)
}

fn init_stream_histories(series_count: usize, history_len: usize) -> Vec<VecDeque<f64>> {
    (0..series_count)
        .map(|_| VecDeque::with_capacity(history_len.max(1)))
        .collect()
}

fn update_stream_histories(histories: &mut [VecDeque<f64>], frame: &[f64], history_len: usize) {
    let limit = history_len.max(1);
    for (history, value) in histories.iter_mut().zip(frame.iter().copied()) {
        if history.len() == limit {
            history.pop_front();
        }
        history.push_back(value.clamp(0.0, 1.0));
    }
}

fn read_next_stream_frame<R: BufRead>(
    reader: &mut R,
    expected_columns: Option<usize>,
) -> Result<Option<Vec<f64>>, String> {
    let mut line = String::new();

    loop {
        line.clear();
        let read = reader.read_line(&mut line).map_err(io_to_string)?;
        if read == 0 {
            return Ok(None);
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let values = trimmed
            .split_whitespace()
            .map(|token| {
                token
                    .parse::<f64>()
                    .map(|value| (value / 100.0).clamp(0.0, 1.0))
                    .map_err(|_| format!("invalid stream value: {token}"))
            })
            .collect::<Result<Vec<_>, _>>()?;

        if values.is_empty() {
            continue;
        }

        if let Some(expected) = expected_columns {
            if values.len() != expected {
                return Err(format!(
                    "stream column count changed: expected {expected}, got {}",
                    values.len()
                ));
            }
        }

        return Ok(Some(values));
    }
}

fn i3bar_frame_json(lines: &[String]) -> serde_json::Value {
    let mut blocks = Vec::with_capacity(lines.len());
    for (index, line) in lines.iter().enumerate() {
        blocks.push(json!({
            "name": format!("monlin-{index}"),
            "full_text": line,
            "separator": false,
            "separator_block_width": 0,
        }));
    }
    serde_json::Value::Array(blocks)
}

fn init_histories(
    metrics: &[MetricKind],
    history_len: usize,
) -> HashMap<MetricKind, VecDeque<MetricValue>> {
    let mut histories = HashMap::with_capacity(metrics.len());
    for metric in metrics {
        histories.insert(*metric, VecDeque::with_capacity(history_len.max(1)));
    }
    histories
}

fn colors_enabled(mode: ColorMode) -> bool {
    match mode {
        ColorMode::Always => true,
        ColorMode::Never => false,
        ColorMode::Auto => io::stdout().is_terminal(),
    }
}

fn monlin_error(error: io::Error) -> String {
    format!("monlin: {error}")
}

fn io_to_string(error: io::Error) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::MetricKind;

    #[test]
    fn i3bar_frame_uses_one_block_per_line() {
        let frame = i3bar_frame_json(&["sys  40% ....".to_owned(), "net 1.5M/s ....".to_owned()]);
        let blocks = frame.as_array().unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0]["name"], "monlin-0");
        assert_eq!(blocks[0]["full_text"], "sys  40% ....");
        assert_eq!(blocks[1]["name"], "monlin-1");
        assert_eq!(blocks[1]["full_text"], "net 1.5M/s ....");
    }

    #[test]
    fn terminal_repaint_moves_back_to_the_top_of_a_multiline_frame() {
        let mut output = Vec::new();

        let rendered_rows = write_terminal_frame(
            &mut output,
            &["sys  40% ....".to_owned(), "net 1.5M/s ....".to_owned()],
            0,
        )
        .unwrap();
        let rendered_rows = write_terminal_frame(
            &mut output,
            &["cpu  12% ....".to_owned(), "ram  63% ....".to_owned()],
            rendered_rows,
        )
        .unwrap();

        assert_eq!(rendered_rows, 2);
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "sys  40% ....\x1b[K\nnet 1.5M/s ....\x1b[K\r\x1b[1Acpu  12% ....\x1b[K\nram  63% ....\x1b[K"
        );
    }

    #[test]
    fn terminal_repaint_clears_stale_rows_when_a_frame_shrinks() {
        let mut output = Vec::new();

        let rendered_rows = write_terminal_frame(
            &mut output,
            &[
                "sys  40% ....".to_owned(),
                "gfx  22% ....".to_owned(),
                "net 1.5M/s ....".to_owned(),
            ],
            0,
        )
        .unwrap();
        let rendered_rows =
            write_terminal_frame(&mut output, &["cpu  12% ....".to_owned()], rendered_rows)
                .unwrap();

        assert_eq!(rendered_rows, 1);
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "sys  40% ....\x1b[K\ngfx  22% ....\x1b[K\nnet 1.5M/s ....\x1b[K\r\x1b[2Acpu  12% ....\x1b[K\n\x1b[K\n\x1b[K\r\x1b[2A"
        );
    }

    #[test]
    fn write_cursor_visibility_emits_expected_escape_sequences() {
        let mut hidden = Vec::new();
        write_cursor_visibility(&mut hidden, false).unwrap();
        assert_eq!(String::from_utf8(hidden).unwrap(), HIDE_CURSOR);

        let mut shown = Vec::new();
        write_cursor_visibility(&mut shown, true).unwrap();
        assert_eq!(String::from_utf8(shown).unwrap(), SHOW_CURSOR);
    }

    #[test]
    fn init_histories_keeps_requested_metrics_and_minimum_capacity() {
        let histories = init_histories(&[MetricKind::Cpu, MetricKind::Net], 0);

        assert_eq!(histories.len(), 2);
        assert!(histories.contains_key(&MetricKind::Cpu));
        assert!(histories.contains_key(&MetricKind::Net));
        assert_eq!(histories[&MetricKind::Cpu].capacity(), 1);
        assert_eq!(histories[&MetricKind::Net].capacity(), 1);
    }

    #[test]
    fn colors_enabled_respects_explicit_modes_and_auto() {
        assert!(colors_enabled(ColorMode::Always));
        assert!(!colors_enabled(ColorMode::Never));
        assert_eq!(colors_enabled(ColorMode::Auto), io::stdout().is_terminal());
    }

    #[test]
    fn zsh_completion_script_mentions_layout_views_and_space_option() {
        let script = zsh_completion_script();
        assert!(script.contains("pct hum free"));
        assert!(script.contains("--space:How streamed columns allocate width"));
    }

    #[test]
    fn validate_stream_labels_accepts_absent_labels_and_rejects_mismatches() {
        let config = config::parse_args(["monlin"].into_iter().map(str::to_owned)).unwrap();
        assert!(validate_stream_labels(&config, 2).is_ok());

        let labeled = config::parse_args(
            ["monlin", "--labels", "a,b"]
                .into_iter()
                .map(str::to_owned),
        )
        .unwrap();
        let error = validate_stream_labels(&labeled, 3).unwrap_err();
        assert!(error.contains("--labels expected 3 entries"));
    }

    #[test]
    fn update_stream_histories_clamps_and_truncates() {
        let mut histories = init_stream_histories(2, 2);
        update_stream_histories(&mut histories, &[1.2, -0.5], 2);
        update_stream_histories(&mut histories, &[0.4, 0.6], 2);
        update_stream_histories(&mut histories, &[0.8, 0.2], 2);

        assert_eq!(histories[0].iter().copied().collect::<Vec<_>>(), vec![0.4, 0.8]);
        assert_eq!(histories[1].iter().copied().collect::<Vec<_>>(), vec![0.6, 0.2]);
    }

    #[test]
    fn read_next_stream_frame_skips_blank_lines_and_parses_percentages() {
        let mut reader = io::Cursor::new(b"\n\n10 20 30\n".as_slice());
        let frame = read_next_stream_frame(&mut reader, None).unwrap().unwrap();
        assert_eq!(frame, vec![0.1, 0.2, 0.3]);
    }

    #[test]
    fn read_next_stream_frame_rejects_invalid_values_and_column_changes() {
        let mut invalid = io::Cursor::new(b"10 nope\n".as_slice());
        let error = read_next_stream_frame(&mut invalid, None).unwrap_err();
        assert!(error.contains("invalid stream value"));

        let mut mismatch = io::Cursor::new(b"10 20 30\n".as_slice());
        let error = read_next_stream_frame(&mut mismatch, Some(2)).unwrap_err();
        assert!(error.contains("stream column count changed"));
    }

    #[test]
    fn io_error_strings_use_expected_formatting() {
        let error = io::Error::other("boom");
        assert_eq!(io_to_string(io::Error::other("boom")), "boom");
        assert_eq!(monlin_error(error), "monlin: boom");
    }

    #[test]
    fn run_help_returns_without_error() {
        assert!(run(vec!["monlin".to_owned(), "--help".to_owned()]).is_ok());
    }

    #[test]
    fn sample_lines_updates_history_and_keeps_width_floor() {
        let config = config::parse_args(
            [
                "monlin",
                "cpu",
                "--history",
                "1",
                "--width",
                "8",
                "--color",
                "never",
                "--once",
            ]
            .into_iter()
            .map(|item| item.to_string()),
        )
        .unwrap();
        let metrics = config.layout.metrics();
        let mut sampler = metrics::Sampler::default();
        sampler.prime(metrics).unwrap();
        let mut histories = init_histories(metrics, config.history);

        let lines = sample_lines(&config, &mut sampler, &mut histories).unwrap();
        let history = histories.get(&MetricKind::Cpu).unwrap();

        assert_eq!(lines.len(), 1);
        assert!(!lines[0].is_empty());
        assert_eq!(history.len(), 1);

        let lines = sample_lines(&config, &mut sampler, &mut histories).unwrap();
        let history = histories.get(&MetricKind::Cpu).unwrap();

        assert_eq!(lines.len(), 1);
        assert_eq!(history.len(), 1);
    }

    #[test]
    fn run_terminal_once_executes_successfully() {
        let config = config::parse_args(
            ["monlin", "cpu", "--width", "16", "--color", "never", "--once"]
            .into_iter()
            .map(|item| item.to_string()),
        )
        .unwrap();
        let metrics = config.layout.metrics();
        let mut sampler = metrics::Sampler::default();
        sampler.prime(metrics).unwrap();
        let mut histories = init_histories(metrics, config.history);

        assert!(run_terminal(&config, &mut sampler, &mut histories).is_ok());
    }

    #[test]
    fn run_i3bar_once_executes_successfully() {
        let config = config::parse_args(
            [
                "monlin",
                "cpu",
                "--width",
                "16",
                "--color",
                "always",
                "--output",
                "i3bar",
                "--once",
            ]
            .into_iter()
            .map(|item| item.to_string()),
        )
        .unwrap();
        let metrics = config.layout.metrics();
        let mut sampler = metrics::Sampler::default();
        sampler.prime(metrics).unwrap();
        let mut histories = init_histories(metrics, config.history);

        assert!(run_i3bar(&config, &mut sampler, &mut histories).is_ok());
    }
}
