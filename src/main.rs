use std::collections::VecDeque;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::process;
use std::thread;
use std::time::Duration;

const DEFAULT_INTERVAL_MS: u64 = 1000;
const DEFAULT_HISTORY: usize = 512;
const BLOCKS: &[char] = &[' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

#[derive(Clone, Copy, Debug)]
struct CpuSample {
    idle: u64,
    total: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Align {
    Left,
    Right,
}

fn parse_arg<T: std::str::FromStr>(args: &[String], flag: &str) -> Option<T> {
    args.windows(2)
        .find(|window| window[0] == flag)
        .and_then(|window| window[1].parse::<T>().ok())
}

fn read_cpu_sample() -> io::Result<CpuSample> {
    let stat = fs::read_to_string("/proc/stat")?;
    let line = stat
        .lines()
        .find(|line| line.starts_with("cpu "))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing aggregate cpu line"))?;
    let mut fields = line.split_whitespace().skip(1);

    let user = next_u64(&mut fields)?;
    let nice = next_u64(&mut fields)?;
    let system = next_u64(&mut fields)?;
    let idle = next_u64(&mut fields)?;
    let iowait = next_u64(&mut fields)?;
    let irq = next_u64(&mut fields)?;
    let softirq = next_u64(&mut fields)?;
    let steal = next_u64(&mut fields).unwrap_or(0);
    let guest = next_u64(&mut fields).unwrap_or(0);
    let guest_nice = next_u64(&mut fields).unwrap_or(0);

    let idle_all = idle + iowait;
    let total = user + nice + system + idle + iowait + irq + softirq + steal + guest + guest_nice;

    Ok(CpuSample {
        idle: idle_all,
        total,
    })
}

fn next_u64<'a, I>(fields: &mut I) -> io::Result<u64>
where
    I: Iterator<Item = &'a str>,
{
    let value = fields
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing cpu field"))?;
    value
        .parse::<u64>()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid cpu field"))
}

fn cpu_usage(prev: CpuSample, current: CpuSample) -> f64 {
    let total_delta = current.total.saturating_sub(prev.total);
    if total_delta == 0 {
        return 0.0;
    }
    let idle_delta = current.idle.saturating_sub(prev.idle);
    let active_delta = total_delta.saturating_sub(idle_delta);
    (active_delta as f64 / total_delta as f64).clamp(0.0, 1.0)
}

fn spark_char(usage: f64) -> char {
    let idx = (usage * (BLOCKS.len() - 1) as f64).round() as usize;
    BLOCKS[idx.min(BLOCKS.len() - 1)]
}

fn render_line(history: &VecDeque<f64>, usage: f64, label: &str, align: Align) -> String {
    let label_prefix = if label.is_empty() {
        String::new()
    } else {
        format!("{label} ")
    };
    let usage_text = format!("{:>3.0}%", usage * 100.0);
    let term_width = terminal_width().unwrap_or(80).max(16);
    let fixed_width = match align {
        Align::Right => label_prefix.chars().count() + 1 + usage_text.chars().count(),
        Align::Left => label_prefix.chars().count() + usage_text.chars().count() + 1,
    };
    let graph_width = term_width.saturating_sub(fixed_width);

    let graph_tail: String = history
        .iter()
        .rev()
        .take(graph_width)
        .copied()
        .collect::<Vec<f64>>()
        .into_iter()
        .rev()
        .map(spark_char)
        .collect();
    let padding = " ".repeat(graph_width.saturating_sub(graph_tail.chars().count()));

    match align {
        Align::Right => format!("{label_prefix}{padding}{graph_tail} {usage_text}"),
        Align::Left => format!("{label_prefix}{usage_text} {graph_tail}{padding}"),
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let interval_ms = parse_arg::<u64>(&args, "--interval-ms").unwrap_or(DEFAULT_INTERVAL_MS);
    let history_len = parse_arg::<usize>(&args, "--history").unwrap_or(DEFAULT_HISTORY);
    let label = parse_label(&args).unwrap_or_default();
    let align = parse_align(&args).unwrap_or(Align::Left);

    let mut prev = match read_cpu_sample() {
        Ok(sample) => sample,
        Err(error) => {
            eprintln!("nxu-cpu: {error}");
            process::exit(1);
        }
    };

    let mut history = VecDeque::with_capacity(history_len.max(1));
    let sleep_for = Duration::from_millis(interval_ms.max(100));
    let mut stdout = io::stdout().lock();

    loop {
        thread::sleep(sleep_for);

        let current = match read_cpu_sample() {
            Ok(sample) => sample,
            Err(error) => {
                eprintln!("\rnxu-cpu: {error}");
                process::exit(1);
            }
        };

        let usage = cpu_usage(prev, current);
        prev = current;

        if history.len() == history_len.max(1) {
            history.pop_front();
        }
        history.push_back(usage);

        let line = render_line(&history, usage, &label, align);
        if write!(stdout, "\r{line}\x1b[K").is_err() {
            process::exit(0);
        }
        if stdout.flush().is_err() {
            process::exit(0);
        }
    }
}

fn parse_label(args: &[String]) -> Option<String> {
    args.windows(2)
        .find(|window| window[0] == "--label")
        .map(|window| window[1].clone())
}

fn parse_align(args: &[String]) -> Option<Align> {
    match args
        .windows(2)
        .find(|window| window[0] == "--align")
        .map(|window| window[1].as_str())
    {
        Some("left") => Some(Align::Left),
        Some("right") => Some(Align::Right),
        _ => None,
    }
}

fn terminal_width() -> Option<usize> {
    let from_env = env::var("COLUMNS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0);
    if from_env.is_some() {
        return from_env;
    }

    unsafe {
        let mut winsize = WinSize {
            ws_row: 0,
            ws_col: 0,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        if ioctl(1, TIOCGWINSZ, &mut winsize) == 0 && winsize.ws_col > 0 {
            return Some(winsize.ws_col as usize);
        }
    }

    None
}

const TIOCGWINSZ: u64 = 0x5413;

#[repr(C)]
struct WinSize {
    ws_row: u16,
    ws_col: u16,
    ws_xpixel: u16,
    ws_ypixel: u16,
}

unsafe extern "C" {
    fn ioctl(fd: i32, request: u64, ...) -> i32;
}
