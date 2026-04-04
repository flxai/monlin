pub mod color;
pub mod config;
pub mod layout;
pub mod metrics;
pub mod render;

use std::collections::{HashMap, VecDeque};
#[cfg(unix)]
use std::ffi::c_void;
use std::fs;
use std::io::{self, BufRead, IsTerminal, Read, Seek, SeekFrom, Write};
use std::process::Command;
use std::thread;
use std::time::Duration;

use clap_complete::{generate, Generator, Shell};
use config::{ColorMode, ExternalInputSource, OutputMode, StreamGroup, StreamItem};
use layout::{Document, MetricKind, Source};
use metrics::{CanonicalSample, CanonicalValue, MetricValue};
use serde_json::json;

const HIDE_CURSOR: &str = "\x1b[?25l";
const SHOW_CURSOR: &str = "\x1b[?25h";

#[cfg(unix)]
const CURSOR_RESTORE_SIGNALS: [i32; 4] = [1, 2, 3, 15];
#[cfg(unix)]
const STDOUT_FILENO: i32 = 1;

enum ExternalPollResult {
    Frame(Vec<f64>),
    NoUpdate,
}

type CanonicalHistories = HashMap<Source, VecDeque<CanonicalValue>>;

enum ExternalPoller {
    File(FilePoller),
    Process(String),
}

struct FilePoller {
    path: std::path::PathBuf,
    offset: u64,
    pending: String,
}

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

    if config.stream_groups.is_some()
        && config.external_input.is_none()
        && !config.force_stream_input
        && io::stdin().is_terminal()
    {
        return Err("stream column references like @1 require stdin input".to_owned());
    }
    if config
        .document
        .as_ref()
        .is_some_and(|document| document.uses_stream_columns())
        && config.external_input.is_none()
        && !config.force_stream_input
        && io::stdin().is_terminal()
    {
        return Err("stream column references like @1 require stdin input".to_owned());
    }

    if let Some(result) = maybe_run_document_sources(&config)? {
        return result;
    }

    if let Some(result) = maybe_run_external_stream(&config)? {
        return result;
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
        config::CompletionShell::PowerShell => generate_to_stdout(Shell::PowerShell, &mut command),
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
    rnd
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
    'f:/path/to/file'
    'p:command'
    'label1,label2=f:/path/to/file'
    'label1,label2=p:command'
  )

  cur="${words[CURRENT]}"
  token="${cur##*,}"
  prefix="${cur%$token}"

  if [[ "$token" == *.* ]]; then
    base="${token%%.*}"
    case "$base" in
      cpu|rnd|sys|gpu|vram|gfx|memory|mem|ram|storage|disk|space|spc|io|net|ingress|in|egress|out)
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
    'rnd:Synthetic random metric' \
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
    'all:Canonical multi-row layout' \
    'f:/path/to/file:Poll a file for numeric rows' \
    'p:command:Poll a shell command for numeric rows' \
    'label1,label2=f:/path/to/file:Poll a file and label the stream columns' \
    'label1,label2=p:command:Poll a command and label the stream columns'
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
    '-e:How native layouts arrange rows'
    '--engine:How native layouts arrange rows'
    '--renderer:Graph renderer to use'
    '-c:Comma-separated visible-order colors: named palettes like default/pastel/neon, angle 20 or A20, RGB Rff8800, or packed LCh L086078020'
    '--colors:Comma-separated visible-order colors: named palettes like default/pastel/neon, angle 20 or A20, RGB Rff8800, or packed LCh L086078020'
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
      compadd stable graph segment
      return
      ;;
    -e|--engine)
      compadd auto flow flex grid pack
      return
      ;;
    -c|--colors)
      compadd default canonical rainbow warm cool pastel neon
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
        let item_hues = crate::color::metric_hues_for_visible_hue(*metric, effective_hues[index]);
        let graph =
            render::render_braille_graph(&samples, width, *metric, Some(&item_hues), color_enabled);
        println!("{label:<4} {graph}");
    }
}

fn run_native(config: &config::Config) -> Result<(), String> {
    let requested_metrics = config.layout.metrics();
    let mut sampler = metrics::Sampler::default();
    sampler.prime(requested_metrics).map_err(monlin_error)?;

    let mut histories = init_canonical_metric_histories(requested_metrics, config.history);
    match config.output_mode {
        OutputMode::Terminal => run_terminal(&config, &mut sampler, &mut histories),
        OutputMode::I3bar => run_i3bar(&config, &mut sampler, &mut histories),
    }
}

fn maybe_run_document_sources(
    config: &config::Config,
) -> Result<Option<Result<(), String>>, String> {
    let Some(document) = &config.document else {
        return Ok(None);
    };
    if !document.uses_external_sources() {
        return Ok(None);
    }
    if document.uses_stream_columns() {
        return Ok(Some(Err(
            "mixing polled sources with stdin column references is not yet supported".to_owned(),
        )));
    }
    if document
        .sources()
        .iter()
        .any(|source| matches!(source, Source::Metric(_)))
    {
        return Ok(Some(Err(
            "mixing native metrics with polled sources is not yet supported".to_owned(),
        )));
    }

    let (groups, sources) = stream_groups_from_external_document(document)?;
    let mut render_config = config.clone();
    render_config.stream_groups = Some(groups);
    render_config.external_input = None;

    let mut pollers = sources
        .iter()
        .map(ExternalPoller::from_source)
        .collect::<Result<Vec<_>, _>>()?;
    let mut current = pollers
        .iter_mut()
        .zip(sources.iter())
        .map(|(poller, source)| match poller.poll(None)? {
            ExternalPollResult::Frame(frame) => scalar_from_frame(source, frame),
            ExternalPollResult::NoUpdate => Ok(0.0),
        })
        .collect::<Result<Vec<_>, String>>()?;
    validate_stream_shape(&render_config, current.len())?;

    let result = match config.output_mode {
        OutputMode::Terminal => {
            run_document_external_terminal(&render_config, &sources, &mut pollers, &mut current)
        }
        OutputMode::I3bar => {
            run_document_external_i3bar(&render_config, &sources, &mut pollers, &mut current)
        }
    };
    Ok(Some(result))
}

fn maybe_run_external_stream(
    config: &config::Config,
) -> Result<Option<Result<(), String>>, String> {
    let Some(source) = &config.external_input else {
        return Ok(None);
    };

    let (mut poller, first_frame) = ExternalPoller::new(source, None)?;
    let first_frame = match first_frame {
        Some(frame) => frame,
        None => return Ok(Some(Ok(()))),
    };
    validate_stream_shape(config, first_frame.len())?;

    let result = match config.output_mode {
        OutputMode::Terminal => run_external_stream_terminal(config, &mut poller, first_frame),
        OutputMode::I3bar => run_external_stream_i3bar(config, &mut poller, first_frame),
    };
    Ok(Some(result))
}

fn stream_groups_from_external_document(
    document: &Document,
) -> Result<(Vec<StreamGroup>, Vec<Source>), String> {
    let mut ordered_sources = Vec::new();
    let mut source_indices = HashMap::new();
    let mut groups: Vec<StreamGroup> = Vec::new();

    for row in document.rows() {
        let mut rendered_row = Vec::with_capacity(row.items().len());
        for item in row.items() {
            let source = item.source().clone();
            if !matches!(source, Source::File(_) | Source::Process(_)) {
                return Err(
                    "document stream rendering currently supports only f:/ and p: items"
                        .to_owned(),
                );
            }
            let index = if let Some(index) = source_indices.get(&source).copied() {
                index
            } else {
                let index = ordered_sources.len();
                source_indices.insert(source.clone(), index);
                ordered_sources.push(source);
                index
            };
            rendered_row.push(StreamItem {
                label: item.label().map(str::to_owned),
                column_index: index,
                display: item.display(),
                basis: item.size(),
                max_width: item.max_width(),
                min_width: item.min_width(),
            });
        }

        let row_label = row.label().map(str::to_owned);
        match groups.last_mut() {
            Some(group) if group.label == row_label => group.rows.push(rendered_row),
            _ => groups.push(StreamGroup {
                label: row_label,
                rows: vec![rendered_row],
            }),
        }
    }

    Ok((groups, ordered_sources))
}

fn stream_groups_from_stream_document(document: &Document) -> Result<Vec<StreamGroup>, String> {
    let mut groups: Vec<StreamGroup> = Vec::new();

    for row in document.rows() {
        let mut rendered_row = Vec::with_capacity(row.items().len());
        for item in row.items() {
            let column_index = match item.source() {
                Source::StreamColumn(index) => *index,
                Source::Metric(_) | Source::File(_) | Source::Process(_) => {
                    return Err(
                        "document stream rendering currently supports only @N items".to_owned(),
                    )
                }
            };
            rendered_row.push(StreamItem {
                label: item.label().map(str::to_owned),
                column_index,
                display: item.display(),
                basis: item.size(),
                max_width: item.max_width(),
                min_width: item.min_width(),
            });
        }

        let row_label = row.label().map(str::to_owned);
        match groups.last_mut() {
            Some(group) if group.label == row_label => group.rows.push(rendered_row),
            _ => groups.push(StreamGroup {
                label: row_label,
                rows: vec![rendered_row],
            }),
        }
    }

    Ok(groups)
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
    let groups = config
        .document
        .as_ref()
        .filter(|document| document.uses_stream_columns())
        .map(stream_groups_from_stream_document)
        .transpose()?;
    let mut render_config = config.clone();
    if let Some(groups) = groups {
        render_config.stream_groups = Some(groups);
    }
    validate_stream_shape(&render_config, first_frame.len())?;

    let result = match render_config.output_mode {
        OutputMode::Terminal => run_stream_terminal(&render_config, &mut reader, first_frame),
        OutputMode::I3bar => run_stream_i3bar(&render_config, &mut reader, first_frame),
    };
    Ok(Some(result))
}

fn validate_stream_shape(config: &config::Config, series_count: usize) -> Result<(), String> {
    if let Some(labels) = &config.stream_labels {
        if labels.len() != series_count {
            return Err(format!(
                "--labels expected {series_count} entries for the input stream, got {}",
                labels.len()
            ));
        }
    }

    if let Some(groups) = &config.stream_groups {
        let max_ref = groups
            .iter()
            .flat_map(|group| group.rows.iter())
            .flat_map(|row| row.iter())
            .map(|item| item.column_index)
            .max();
        if let Some(max_ref) = max_ref {
            if max_ref >= series_count {
                return Err(format!(
                    "stream layout references @{}, but the input stream has only {series_count} columns",
                    max_ref + 1
                ));
            }
        }
    }

    Ok(())
}

fn describe_source(source: &Source) -> String {
    match source {
        Source::Metric(metric) => metric.short_label().to_owned(),
        Source::StreamColumn(index) => format!("@{}", index + 1),
        Source::File(path) => format!("f:{}", path.display()),
        Source::Process(command) => format!("p:{command}"),
    }
}

fn scalar_from_frame(source: &Source, frame: Vec<f64>) -> Result<f64, String> {
    if frame.len() != 1 {
        return Err(format!(
            "source {} must produce exactly one numeric value per sample",
            describe_source(source)
        ));
    }
    Ok(frame[0])
}

fn run_terminal(
    config: &config::Config,
    sampler: &mut metrics::Sampler,
    histories: &mut CanonicalHistories,
) -> Result<(), String> {
    let hide_cursor = io::stdout().is_terminal();
    let mut stdout = io::stdout().lock();
    let _cursor_guard =
        CursorVisibilityGuard::new(&mut stdout, hide_cursor).map_err(io_to_string)?;
    let mut frame_state = TerminalFrameState::default();

    loop {
        if config.interval_ms > 0 {
            thread::sleep(Duration::from_millis(config.interval_ms));
        }

        let lines = sample_lines(config, sampler, histories)?;

        let width = config
            .width
            .or_else(render::terminal_width)
            .unwrap_or(80)
            .max(16);
        write_terminal_frame(&mut stdout, &lines, &mut frame_state, width).map_err(io_to_string)?;
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
    let mut frame_state = TerminalFrameState::default();
    let mut histories = init_canonical_stream_histories(first_frame.len(), config.history);
    let color_enabled = colors_enabled(config.color_mode);

    let mut current = first_frame;
    loop {
        update_canonical_stream_histories(&mut histories, &current, config.history);
        let width = config
            .width
            .or_else(render::terminal_width)
            .unwrap_or(80)
            .max(16);
        let projected_histories = project_stream_histories(&histories, current.len());
        let projected_values = project_stream_values(&current);
        let lines = render::render_stream_lines(
            config,
            width,
            color_enabled,
            &projected_histories,
            &projected_values,
        );
        write_terminal_frame(&mut stdout, &lines, &mut frame_state, width).map_err(io_to_string)?;
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

fn run_external_stream_terminal(
    config: &config::Config,
    poller: &mut ExternalPoller,
    first_frame: Vec<f64>,
) -> Result<(), String> {
    let hide_cursor = io::stdout().is_terminal();
    let mut stdout = io::stdout().lock();
    let _cursor_guard =
        CursorVisibilityGuard::new(&mut stdout, hide_cursor).map_err(io_to_string)?;
    let mut frame_state = TerminalFrameState::default();
    let mut histories = init_canonical_stream_histories(first_frame.len(), config.history);
    let color_enabled = colors_enabled(config.color_mode);

    let mut current = first_frame;
    loop {
        update_canonical_stream_histories(&mut histories, &current, config.history);
        let width = config
            .width
            .or_else(render::terminal_width)
            .unwrap_or(80)
            .max(16);
        let projected_histories = project_stream_histories(&histories, current.len());
        let projected_values = project_stream_values(&current);
        let lines = render::render_stream_lines(
            config,
            width,
            color_enabled,
            &projected_histories,
            &projected_values,
        );
        write_terminal_frame(&mut stdout, &lines, &mut frame_state, width).map_err(io_to_string)?;
        stdout.flush().map_err(io_to_string)?;

        if config.once {
            writeln!(stdout).map_err(io_to_string)?;
            break;
        }

        if config.interval_ms > 0 {
            thread::sleep(Duration::from_millis(config.interval_ms));
        }

        match poller.poll(Some(current.len()))? {
            ExternalPollResult::Frame(frame) => current = frame,
            ExternalPollResult::NoUpdate => {}
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

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct TerminalFrameState {
    reserved_rows: usize,
    previous_lines: Vec<String>,
}

fn write_terminal_frame<W: Write>(
    stdout: &mut W,
    lines: &[String],
    frame_state: &mut TerminalFrameState,
    width: usize,
) -> io::Result<()> {
    let current_rows = physical_frame_rows(lines, width).max(1);
    let previous_rows = physical_frame_rows(&frame_state.previous_lines, width);

    if frame_state.reserved_rows == 0 {
        frame_state.reserved_rows = current_rows;
        write_frame_lines(stdout, lines)?;
        frame_state.previous_lines = lines.to_vec();
        return Ok(());
    }

    if current_rows > frame_state.reserved_rows {
        for _ in 0..(current_rows - frame_state.reserved_rows) {
            writeln!(stdout)?;
        }
        frame_state.reserved_rows = current_rows;
    }

    move_to_previous_frame_top(stdout, previous_rows)?;
    clear_reserved_region(stdout, frame_state.reserved_rows)?;
    move_to_region_top(stdout, frame_state.reserved_rows)?;
    write_frame_lines(stdout, lines)?;
    frame_state.previous_lines = lines.to_vec();

    Ok(())
}

fn write_frame_lines<W: Write>(stdout: &mut W, lines: &[String]) -> io::Result<()> {
    for (index, line) in lines.iter().enumerate() {
        write!(stdout, "{line}\x1b[K")?;
        if index + 1 < lines.len() {
            writeln!(stdout)?;
        }
    }

    Ok(())
}

fn move_to_region_top<W: Write>(stdout: &mut W, reserved_rows: usize) -> io::Result<()> {
    write!(stdout, "\r")?;
    if reserved_rows > 1 {
        write!(stdout, "\x1b[{}A", reserved_rows - 1)?;
    }
    Ok(())
}

fn move_to_previous_frame_top<W: Write>(stdout: &mut W, previous_rows: usize) -> io::Result<()> {
    write!(stdout, "\r")?;
    if previous_rows > 1 {
        write!(stdout, "\x1b[{}A", previous_rows - 1)?;
    }
    Ok(())
}

fn clear_reserved_region<W: Write>(stdout: &mut W, reserved_rows: usize) -> io::Result<()> {
    for row in 0..reserved_rows {
        write!(stdout, "\x1b[2K")?;
        if row + 1 < reserved_rows {
            write!(stdout, "\x1b[1B\r")?;
        }
    }
    Ok(())
}

fn physical_frame_rows(lines: &[String], width: usize) -> usize {
    let width = width.max(1);
    lines
        .iter()
        .map(|line| {
            let visible = render::visible_width(line);
            visible.max(1).div_ceil(width)
        })
        .sum()
}

fn write_cursor_visibility<W: Write>(stdout: &mut W, visible: bool) -> io::Result<()> {
    let sequence = if visible { SHOW_CURSOR } else { HIDE_CURSOR };
    write!(stdout, "{sequence}")
}

fn run_i3bar(
    config: &config::Config,
    sampler: &mut metrics::Sampler,
    histories: &mut CanonicalHistories,
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
    let mut histories = init_canonical_stream_histories(first_frame.len(), config.history);
    let width = config
        .width
        .or_else(render::terminal_width)
        .unwrap_or(80)
        .max(16);

    let mut current = first_frame;
    loop {
        update_canonical_stream_histories(&mut histories, &current, config.history);
        let projected_histories = project_stream_histories(&histories, current.len());
        let projected_values = project_stream_values(&current);
        let lines =
            render::render_stream_lines(config, width, false, &projected_histories, &projected_values);
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

fn run_external_stream_i3bar(
    config: &config::Config,
    poller: &mut ExternalPoller,
    first_frame: Vec<f64>,
) -> Result<(), String> {
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{}", json!({"version": 1, "click_events": false})).map_err(io_to_string)?;
    writeln!(stdout, "[").map_err(io_to_string)?;

    let mut first = true;
    let mut histories = init_canonical_stream_histories(first_frame.len(), config.history);
    let width = config
        .width
        .or_else(render::terminal_width)
        .unwrap_or(80)
        .max(16);

    let mut current = first_frame;
    loop {
        update_canonical_stream_histories(&mut histories, &current, config.history);
        let projected_histories = project_stream_histories(&histories, current.len());
        let projected_values = project_stream_values(&current);
        let lines =
            render::render_stream_lines(config, width, false, &projected_histories, &projected_values);
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

        if config.interval_ms > 0 {
            thread::sleep(Duration::from_millis(config.interval_ms));
        }

        match poller.poll(Some(current.len()))? {
            ExternalPollResult::Frame(frame) => current = frame,
            ExternalPollResult::NoUpdate => {}
        }
    }

    Ok(())
}

fn run_document_external_terminal(
    config: &config::Config,
    sources: &[Source],
    pollers: &mut [ExternalPoller],
    current: &mut [f64],
) -> Result<(), String> {
    let hide_cursor = io::stdout().is_terminal();
    let mut stdout = io::stdout().lock();
    let _cursor_guard =
        CursorVisibilityGuard::new(&mut stdout, hide_cursor).map_err(io_to_string)?;
    let mut frame_state = TerminalFrameState::default();
    let mut histories = init_canonical_stream_histories(current.len(), config.history);
    let color_enabled = colors_enabled(config.color_mode);

    loop {
        update_canonical_stream_histories(&mut histories, current, config.history);
        let width = config
            .width
            .or_else(render::terminal_width)
            .unwrap_or(80)
            .max(16);
        let projected_histories = project_stream_histories(&histories, current.len());
        let projected_values = project_stream_values(current);
        let lines = render::render_stream_lines(
            config,
            width,
            color_enabled,
            &projected_histories,
            &projected_values,
        );
        write_terminal_frame(&mut stdout, &lines, &mut frame_state, width).map_err(io_to_string)?;
        stdout.flush().map_err(io_to_string)?;

        if config.once {
            writeln!(stdout).map_err(io_to_string)?;
            break;
        }

        if config.interval_ms > 0 {
            thread::sleep(Duration::from_millis(config.interval_ms));
        }

        for ((poller, source), value) in pollers.iter_mut().zip(sources.iter()).zip(current.iter_mut()) {
            if let ExternalPollResult::Frame(frame) = poller.poll(None)? {
                *value = scalar_from_frame(source, frame)?;
            }
        }
    }

    Ok(())
}

fn run_document_external_i3bar(
    config: &config::Config,
    sources: &[Source],
    pollers: &mut [ExternalPoller],
    current: &mut [f64],
) -> Result<(), String> {
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{}", json!({"version": 1, "click_events": false})).map_err(io_to_string)?;
    writeln!(stdout, "[").map_err(io_to_string)?;

    let mut first = true;
    let mut histories = init_canonical_stream_histories(current.len(), config.history);

    loop {
        update_canonical_stream_histories(&mut histories, current, config.history);
        let width = config
            .width
            .or_else(render::terminal_width)
            .unwrap_or(80)
            .max(16);
        let projected_histories = project_stream_histories(&histories, current.len());
        let projected_values = project_stream_values(current);
        let lines =
            render::render_stream_lines(config, width, false, &projected_histories, &projected_values);
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

        if config.interval_ms > 0 {
            thread::sleep(Duration::from_millis(config.interval_ms));
        }

        for ((poller, source), value) in pollers.iter_mut().zip(sources.iter()).zip(current.iter_mut()) {
            if let ExternalPollResult::Frame(frame) = poller.poll(None)? {
                *value = scalar_from_frame(source, frame)?;
            }
        }
    }

    Ok(())
}

fn sample_lines(
    config: &config::Config,
    sampler: &mut metrics::Sampler,
    histories: &mut CanonicalHistories,
) -> Result<Vec<String>, String> {
    let requested_metrics = config.layout.metrics();
    let sample = sampler.sample_canonical(requested_metrics).map_err(monlin_error)?;
    update_canonical_metric_histories(histories, &sample, requested_metrics, config.history);
    let values = project_metric_values(&sample);
    let headlines = project_metric_headlines(&sample);
    let projected_histories = project_metric_histories(histories, requested_metrics);

    let width = config
        .width
        .or_else(render::terminal_width)
        .unwrap_or(80)
        .max(16);
    let color_enabled = match config.output_mode {
        OutputMode::I3bar => false,
        OutputMode::Terminal => colors_enabled(config.color_mode),
    };
    let mut lines = render::render_lines_with_headlines(
        config,
        width,
        color_enabled,
        &projected_histories,
        &config.layout,
        &values,
        &headlines,
    );
    if lines.is_empty() {
        lines.push(String::new());
    }

    Ok(lines)
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
        return parse_stream_frame(trimmed, expected_columns).map(Some);
    }
}

fn parse_stream_frame(line: &str, expected_columns: Option<usize>) -> Result<Vec<f64>, String> {
    let values = line
        .split_whitespace()
        .map(|token| {
            token
                .parse::<f64>()
                .map_err(|_| format!("invalid stream value: {token}"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    if values.is_empty() {
        return Err("stream frame must contain at least one numeric value".to_owned());
    }

    if let Some(expected) = expected_columns {
        if values.len() != expected {
            return Err(format!(
                "stream column count changed: expected {expected}, got {}",
                values.len()
            ));
        }
    }

    Ok(values)
}

impl ExternalPoller {
    fn from_source(source: &Source) -> Result<Self, String> {
        match source {
            Source::File(path) => Ok(Self::File(FilePoller {
                path: path.clone(),
                offset: 0,
                pending: String::new(),
            })),
            Source::Process(command) => Ok(Self::Process(command.clone())),
            Source::Metric(_) | Source::StreamColumn(_) => Err(format!(
                "unsupported polled source: {}",
                describe_source(source)
            )),
        }
    }

    fn new(
        source: &ExternalInputSource,
        expected_columns: Option<usize>,
    ) -> Result<(Self, Option<Vec<f64>>), String> {
        match source {
            ExternalInputSource::File(path) => {
                let mut poller = FilePoller {
                    path: path.clone(),
                    offset: 0,
                    pending: String::new(),
                };
                let first = poller.poll(expected_columns, true)?;
                Ok((Self::File(poller), first))
            }
            ExternalInputSource::Process(command) => {
                let poller = Self::Process(command.clone());
                let first = match poll_latest_output_line(
                    &run_external_process(command)?,
                    expected_columns,
                )? {
                    Some(frame) => Some(frame),
                    None => None,
                };
                Ok((poller, first))
            }
        }
    }

    fn poll(&mut self, expected_columns: Option<usize>) -> Result<ExternalPollResult, String> {
        match self {
            Self::File(poller) => Ok(match poller.poll(expected_columns, false)? {
                Some(frame) => ExternalPollResult::Frame(frame),
                None => ExternalPollResult::NoUpdate,
            }),
            Self::Process(command) => Ok(
                match poll_latest_output_line(&run_external_process(command)?, expected_columns)? {
                    Some(frame) => ExternalPollResult::Frame(frame),
                    None => ExternalPollResult::NoUpdate,
                },
            ),
        }
    }
}

impl FilePoller {
    fn poll(
        &mut self,
        expected_columns: Option<usize>,
        allow_pending_at_eof: bool,
    ) -> Result<Option<Vec<f64>>, String> {
        let metadata = fs::metadata(&self.path)
            .map_err(|error| format!("monlin: {}: {error}", self.path.display()))?;
        if metadata.len() < self.offset {
            self.offset = 0;
            self.pending.clear();
        }

        let mut file = fs::File::open(&self.path)
            .map_err(|error| format!("monlin: {}: {error}", self.path.display()))?;
        file.seek(SeekFrom::Start(self.offset))
            .map_err(|error| format!("monlin: {}: {error}", self.path.display()))?;

        let mut chunk = String::new();
        file.read_to_string(&mut chunk)
            .map_err(|error| format!("monlin: {}: {error}", self.path.display()))?;
        self.offset = file
            .stream_position()
            .map_err(|error| format!("monlin: {}: {error}", self.path.display()))?;

        if chunk.is_empty() {
            if allow_pending_at_eof && !self.pending.trim().is_empty() {
                return parse_stream_frame(self.pending.trim(), expected_columns).map(Some);
            }
            return Ok(None);
        }

        let mut text = std::mem::take(&mut self.pending);
        text.push_str(&chunk);
        let trailing_newline = text.ends_with('\n');
        let mut latest = None;
        let mut parts = text.split('\n').collect::<Vec<_>>();
        if !trailing_newline {
            self.pending = parts.pop().unwrap_or_default().to_owned();
        }

        for line in parts {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            latest = Some(parse_stream_frame(trimmed, expected_columns)?);
        }

        if trailing_newline {
            self.pending.clear();
        } else if allow_pending_at_eof && latest.is_none() && !self.pending.trim().is_empty() {
            latest = Some(parse_stream_frame(self.pending.trim(), expected_columns)?);
        }

        Ok(latest)
    }
}

fn poll_latest_output_line(
    text: &str,
    expected_columns: Option<usize>,
) -> Result<Option<Vec<f64>>, String> {
    for line in text.lines().rev() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        return parse_stream_frame(trimmed, expected_columns).map(Some);
    }

    Ok(None)
}

fn run_external_process(command: &str) -> Result<String, String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned());
    let output = Command::new(&shell)
        .arg("-lc")
        .arg(command)
        .output()
        .map_err(|error| format!("monlin: failed to run process source `{command}`: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        if stderr.is_empty() {
            return Err(format!(
                "monlin: process source `{command}` exited with {}",
                output.status
            ));
        }
        return Err(format!(
            "monlin: process source `{command}` failed: {stderr}"
        ));
    }

    String::from_utf8(output.stdout).map_err(|error| {
        format!("monlin: process source `{command}` produced invalid UTF-8: {error}")
    })
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

fn init_canonical_metric_histories(
    metrics: &[MetricKind],
    history_len: usize,
) -> CanonicalHistories {
    let mut histories = HashMap::with_capacity(metrics.len());
    for metric in metrics {
        histories.insert(
            Source::Metric(*metric),
            VecDeque::with_capacity(history_len.max(1)),
        );
    }
    histories
}

fn init_canonical_stream_histories(series_count: usize, history_len: usize) -> CanonicalHistories {
    let mut histories = HashMap::with_capacity(series_count);
    for index in 0..series_count {
        histories.insert(
            Source::StreamColumn(index),
            VecDeque::with_capacity(history_len.max(1)),
        );
    }
    histories
}

fn update_canonical_metric_histories(
    histories: &mut CanonicalHistories,
    sample: &CanonicalSample,
    metrics: &[MetricKind],
    history_len: usize,
) {
    let limit = history_len.max(1);
    for metric in metrics {
        let source = Source::Metric(*metric);
        if let Some(history) = histories.get_mut(&source) {
            if let Some(value) = sample.values.get(&source).copied() {
                if history.len() == limit {
                    history.pop_front();
                }
                history.push_back(value);
            } else {
                history.clear();
            }
        }
    }
}

fn update_canonical_stream_histories(
    histories: &mut CanonicalHistories,
    frame: &[f64],
    history_len: usize,
) {
    let limit = history_len.max(1);
    for (index, raw) in frame.iter().copied().enumerate() {
        let source = Source::StreamColumn(index);
        if let Some(history) = histories.get_mut(&source) {
            if history.len() == limit {
                history.pop_front();
            }
            history.push_back(CanonicalValue::from_stream_percent(raw));
        }
    }
}

fn project_metric_histories(
    histories: &CanonicalHistories,
    metrics: &[MetricKind],
) -> HashMap<MetricKind, VecDeque<MetricValue>> {
    metrics
        .iter()
        .map(|metric| {
            let history = histories
                .get(&Source::Metric(*metric))
                .map(|samples| {
                    samples
                        .iter()
                        .copied()
                        .filter_map(CanonicalValue::normalized_metric_value)
                        .collect::<VecDeque<_>>()
                })
                .unwrap_or_default();
            (*metric, history)
        })
        .collect()
}

fn project_metric_values(sample: &CanonicalSample) -> HashMap<MetricKind, MetricValue> {
    sample
        .values
        .iter()
        .filter_map(|(source, value)| match source {
            Source::Metric(metric) => value.normalized_metric_value().map(|value| (*metric, value)),
            Source::StreamColumn(_) | Source::File(_) | Source::Process(_) => None,
        })
        .collect()
}

fn project_metric_headlines(sample: &CanonicalSample) -> HashMap<MetricKind, metrics::HeadlineValue> {
    sample
        .headlines
        .iter()
        .filter_map(|(source, headline)| match source {
            Source::Metric(metric) => Some((*metric, *headline)),
            Source::StreamColumn(_) | Source::File(_) | Source::Process(_) => None,
        })
        .collect()
}

fn project_stream_histories(histories: &CanonicalHistories, series_count: usize) -> Vec<VecDeque<f64>> {
    (0..series_count)
        .map(|index| {
            histories
                .get(&Source::StreamColumn(index))
                .map(|samples| {
                    samples
                        .iter()
                        .filter_map(|sample| match sample {
                            CanonicalValue::Scalar { normalized, .. } => Some(*normalized),
                            CanonicalValue::Split { .. } | CanonicalValue::Unavailable => None,
                        })
                        .collect::<VecDeque<_>>()
                })
                .unwrap_or_default()
        })
        .collect()
}

fn project_stream_values(frame: &[f64]) -> Vec<f64> {
    frame.iter()
        .copied()
        .map(|raw| match CanonicalValue::from_stream_percent(raw) {
            CanonicalValue::Scalar { normalized, .. } => normalized,
            CanonicalValue::Split { .. } | CanonicalValue::Unavailable => 0.0,
        })
        .collect()
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
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn i3bar_frame_uses_one_block_per_line() {
        let frame = i3bar_frame_json(&["sys  40% ....".to_owned(), "net 1.5M ....".to_owned()]);
        let blocks = frame.as_array().unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0]["name"], "monlin-0");
        assert_eq!(blocks[0]["full_text"], "sys  40% ....");
        assert_eq!(blocks[1]["name"], "monlin-1");
        assert_eq!(blocks[1]["full_text"], "net 1.5M ....");
    }

    #[test]
    fn terminal_repaint_moves_back_to_the_top_of_a_multiline_frame() {
        let mut output = Vec::new();
        let mut frame_state = TerminalFrameState::default();

        write_terminal_frame(
            &mut output,
            &["sys  40% ....".to_owned(), "net 1.5M ....".to_owned()],
            &mut frame_state,
            80,
        )
        .unwrap();
        write_terminal_frame(
            &mut output,
            &["cpu  12% ....".to_owned(), "ram  63% ....".to_owned()],
            &mut frame_state,
            80,
        )
        .unwrap();

        assert_eq!(frame_state.reserved_rows, 2);
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "sys  40% ....\x1b[K\nnet 1.5M ....\x1b[K\r\x1b[1A\x1b[2K\x1b[1B\r\x1b[2K\r\x1b[1Acpu  12% ....\x1b[K\nram  63% ....\x1b[K"
        );
    }

    #[test]
    fn terminal_repaint_clears_stale_rows_when_a_frame_shrinks() {
        let mut output = Vec::new();
        let mut frame_state = TerminalFrameState::default();

        write_terminal_frame(
            &mut output,
            &[
                "sys  40% ....".to_owned(),
                "gfx  22% ....".to_owned(),
                "net 1.5M ....".to_owned(),
            ],
            &mut frame_state,
            80,
        )
        .unwrap();
        write_terminal_frame(
            &mut output,
            &["cpu  12% ....".to_owned()],
            &mut frame_state,
            80,
        )
        .unwrap();

        assert_eq!(frame_state.reserved_rows, 3);
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "sys  40% ....\x1b[K\ngfx  22% ....\x1b[K\nnet 1.5M ....\x1b[K\r\x1b[2A\x1b[2K\x1b[1B\r\x1b[2K\x1b[1B\r\x1b[2K\r\x1b[2Acpu  12% ....\x1b[K"
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
    fn terminal_repaint_tracks_wrapped_physical_rows() {
        let mut output = Vec::new();
        let mut frame_state = TerminalFrameState::default();

        write_terminal_frame(
            &mut output,
            &["1234567890ABCDE".to_owned()],
            &mut frame_state,
            10,
        )
        .unwrap();
        assert_eq!(frame_state.reserved_rows, 2);

        write_terminal_frame(&mut output, &["cpu".to_owned()], &mut frame_state, 10).unwrap();
        assert_eq!(frame_state.reserved_rows, 2);
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "1234567890ABCDE\x1b[K\r\x1b[1A\x1b[2K\x1b[1B\r\x1b[2K\r\x1b[1Acpu\x1b[K"
        );
    }

    #[test]
    fn init_histories_keeps_requested_metrics_and_minimum_capacity() {
        let histories = init_canonical_metric_histories(&[MetricKind::Cpu, MetricKind::Net], 0);

        assert_eq!(histories.len(), 2);
        assert!(histories.contains_key(&Source::Metric(MetricKind::Cpu)));
        assert!(histories.contains_key(&Source::Metric(MetricKind::Net)));
        assert_eq!(histories[&Source::Metric(MetricKind::Cpu)].capacity(), 1);
        assert_eq!(histories[&Source::Metric(MetricKind::Net)].capacity(), 1);
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
    fn validate_stream_shape_accepts_absent_labels_and_rejects_mismatches() {
        let config = config::parse_args(["monlin"].into_iter().map(str::to_owned)).unwrap();
        assert!(validate_stream_shape(&config, 2).is_ok());

        let labeled =
            config::parse_args(["monlin", "--labels", "a,b"].into_iter().map(str::to_owned))
                .unwrap();
        let error = validate_stream_shape(&labeled, 3).unwrap_err();
        assert!(error.contains("--labels expected 3 entries"));
    }

    #[test]
    fn validate_stream_shape_rejects_missing_referenced_columns() {
        let config = config::parse_args(
            ["monlin", "cpu=@1", "ram=@3"]
                .into_iter()
                .map(str::to_owned),
        )
        .unwrap();
        let error = validate_stream_shape(&config, 2).unwrap_err();
        assert!(error.contains("@3"));
    }

    #[test]
    fn update_stream_histories_clamps_and_truncates() {
        let mut histories = init_canonical_stream_histories(2, 2);
        update_canonical_stream_histories(&mut histories, &[120.0, -50.0], 2);
        update_canonical_stream_histories(&mut histories, &[40.0, 60.0], 2);
        update_canonical_stream_histories(&mut histories, &[80.0, 20.0], 2);

        assert_eq!(
            project_stream_histories(&histories, 2)[0]
                .iter()
                .copied()
                .collect::<Vec<_>>(),
            vec![0.4, 0.8]
        );
        assert_eq!(
            project_stream_histories(&histories, 2)[1]
                .iter()
                .copied()
                .collect::<Vec<_>>(),
            vec![0.6, 0.2]
        );
    }

    #[test]
    fn read_next_stream_frame_skips_blank_lines_and_parses_percentages() {
        let mut reader = io::Cursor::new(b"\n\n10 20 30\n".as_slice());
        let frame = read_next_stream_frame(&mut reader, None).unwrap().unwrap();
        assert_eq!(frame, vec![10.0, 20.0, 30.0]);
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
    fn parse_stream_frame_parses_percentages() {
        let frame = parse_stream_frame("10 20 30", None).unwrap();
        assert_eq!(frame, vec![10.0, 20.0, 30.0]);
    }

    #[test]
    fn file_poller_uses_latest_non_empty_file_line() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("monlin-external-{unique}.txt"));
        fs::write(&path, "10 20\n\n30 40\n").unwrap();

        let mut poller = FilePoller {
            path: path.clone(),
            offset: 0,
            pending: String::new(),
        };
        let frame = poller.poll(None, true).unwrap().unwrap();
        assert_eq!(frame, vec![30.0, 40.0]);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn file_poller_only_emits_new_appended_lines() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("monlin-external-append-{unique}.txt"));
        fs::write(&path, "10 20\n").unwrap();

        let mut poller = FilePoller {
            path: path.clone(),
            offset: 0,
            pending: String::new(),
        };
        assert_eq!(poller.poll(None, true).unwrap(), Some(vec![10.0, 20.0]));
        assert_eq!(poller.poll(None, false).unwrap(), None);

        fs::write(&path, "10 20\n30 40\n").unwrap();
        assert_eq!(poller.poll(None, false).unwrap(), Some(vec![30.0, 40.0]));
        assert_eq!(poller.poll(None, false).unwrap(), None);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn external_poller_runs_process_sources() {
        let (_, frame) = ExternalPoller::new(
            &ExternalInputSource::Process("printf '10 20\\n30 40\\n'".to_owned()),
            None,
        )
        .unwrap();
        let frame = frame.unwrap();
        assert_eq!(frame, vec![30.0, 40.0]);
    }

    #[test]
    fn document_external_sources_lower_to_stream_groups() {
        let document =
            layout::parse_layout_document("\"pair\"=(one=p:'printf 10', two=f:/tmp/example)")
                .unwrap();
        let (groups, sources) = stream_groups_from_external_document(&document).unwrap();

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].label.as_deref(), Some("pair"));
        assert_eq!(groups[0].rows.len(), 2);
        assert_eq!(groups[0].rows[0][0].label.as_deref(), Some("one"));
        assert_eq!(groups[0].rows[1][0].label.as_deref(), Some("two"));
        assert_eq!(sources.len(), 2);
        assert!(matches!(sources[0], Source::Process(_)));
        assert!(matches!(sources[1], Source::File(_)));
    }

    #[test]
    fn mixed_native_and_polled_document_sources_are_rejected() {
        let mut config =
            config::parse_args(["monlin"].into_iter().map(str::to_owned)).unwrap();
        config.document = Some(layout::parse_layout_document("cpu load=p:'printf 10'").unwrap());
        let result = maybe_run_document_sources(&config).unwrap().unwrap();
        assert!(result
            .unwrap_err()
            .contains("mixing native metrics with polled sources"));
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
        let mut histories = init_canonical_metric_histories(metrics, config.history);

        let lines = sample_lines(&config, &mut sampler, &mut histories).unwrap();
        let history = histories.get(&Source::Metric(MetricKind::Cpu)).unwrap();

        assert_eq!(lines.len(), 1);
        assert!(!lines[0].is_empty());
        assert_eq!(history.len(), 1);

        let lines = sample_lines(&config, &mut sampler, &mut histories).unwrap();
        let history = histories.get(&Source::Metric(MetricKind::Cpu)).unwrap();

        assert_eq!(lines.len(), 1);
        assert_eq!(history.len(), 1);
    }

    #[test]
    fn run_terminal_once_executes_successfully() {
        let config = config::parse_args(
            [
                "monlin", "cpu", "--width", "16", "--color", "never", "--once",
            ]
            .into_iter()
            .map(|item| item.to_string()),
        )
        .unwrap();
        let metrics = config.layout.metrics();
        let mut sampler = metrics::Sampler::default();
        sampler.prime(metrics).unwrap();
        let mut histories = init_canonical_metric_histories(metrics, config.history);

        assert!(run_terminal(&config, &mut sampler, &mut histories).is_ok());
    }

    #[test]
    fn run_i3bar_once_executes_successfully() {
        let config = config::parse_args(
            [
                "monlin", "cpu", "--width", "16", "--color", "always", "--output", "i3bar",
                "--once",
            ]
            .into_iter()
            .map(|item| item.to_string()),
        )
        .unwrap();
        let metrics = config.layout.metrics();
        let mut sampler = metrics::Sampler::default();
        sampler.prime(metrics).unwrap();
        let mut histories = init_canonical_metric_histories(metrics, config.history);

        assert!(run_i3bar(&config, &mut sampler, &mut histories).is_ok());
    }
}
