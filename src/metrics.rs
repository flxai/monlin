use std::collections::{HashMap, HashSet, VecDeque};
use std::ffi::CString;
use std::fs;
use std::io;
use std::os::raw::{c_char, c_int, c_ulong};
use std::time::{Duration, Instant};

use crate::layout::{MetricKind, Source};
use rand::rngs::SmallRng;
use rand::SeedableRng;
use rand_distr::{Beta, Distribution, Normal};

#[derive(Clone, Copy, Debug)]
struct CpuCounters {
    idle: u64,
    total: u64,
}

#[derive(Clone, Copy, Debug)]
struct DiskCounters {
    read_bytes: u64,
    write_bytes: u64,
}

#[derive(Clone, Copy, Debug)]
struct NetCounters {
    rx_bytes: u64,
    tx_bytes: u64,
}

#[derive(Clone, Copy, Debug, Default)]
struct GpuSample {
    utilization: Option<f64>,
    vram_used_bytes: Option<u64>,
    vram_total_bytes: Option<u64>,
}

#[derive(Clone, Copy, Debug)]
struct MemorySample {
    usage_ratio: f64,
    used_bytes: u64,
    available_bytes: u64,
    total_bytes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MetricValue {
    Single(f64),
    Split { upper: f64, lower: f64 },
}

#[derive(Debug)]
pub struct Sample {
    pub values: HashMap<MetricKind, MetricValue>,
    pub headlines: HashMap<MetricKind, HeadlineValue>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CanonicalValue {
    Scalar {
        normalized: f64,
        absolute: Option<f64>,
    },
    Split {
        upper_normalized: f64,
        lower_normalized: f64,
        upper_absolute: Option<f64>,
        lower_absolute: Option<f64>,
    },
    Unavailable,
}

#[derive(Debug)]
pub struct CanonicalSample {
    pub values: HashMap<Source, CanonicalValue>,
    pub headlines: HashMap<Source, HeadlineValue>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HeadlineValue {
    Scalar(f64),
    Memory {
        used_bytes: u64,
        available_bytes: u64,
        total_bytes: u64,
    },
    Storage {
        used_bytes: u64,
        total_bytes: u64,
    },
}

impl HeadlineValue {
    pub fn scalar(self) -> Option<f64> {
        match self {
            Self::Scalar(value) => Some(value),
            Self::Memory { .. } => None,
            Self::Storage { .. } => None,
        }
    }
}

impl MetricValue {
    pub fn headline_value(self) -> f64 {
        match self {
            Self::Single(value) => value,
            Self::Split { upper, lower } => upper.max(lower),
        }
    }

    pub fn upper(self) -> f64 {
        match self {
            Self::Single(value) => value,
            Self::Split { upper, .. } => upper,
        }
    }

    pub fn lower(self) -> f64 {
        match self {
            Self::Single(value) => value,
            Self::Split { lower, .. } => lower,
        }
    }
}

impl CanonicalValue {
    pub fn normalized_metric_value(self) -> Option<MetricValue> {
        match self {
            Self::Scalar { normalized, .. } => Some(MetricValue::Single(normalized)),
            Self::Split {
                upper_normalized,
                lower_normalized,
                ..
            } => Some(MetricValue::Split {
                upper: upper_normalized,
                lower: lower_normalized,
            }),
            Self::Unavailable => None,
        }
    }

    pub fn from_stream_percent(raw: f64) -> Self {
        Self::Scalar {
            normalized: (raw / 100.0).clamp(0.0, 1.0),
            absolute: Some(raw.max(0.0)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum ScaleKey {
    IoRead,
    IoWrite,
    NetIngress,
    NetEgress,
}

#[derive(Clone, Copy, Debug)]
struct RateSample {
    value: MetricValue,
    upper_raw: f64,
    lower_raw: f64,
}

#[derive(Clone, Copy, Debug)]
struct RatePoint {
    at: Instant,
    value: f64,
}

#[derive(Debug)]
pub struct Sampler {
    cpu_prev: Option<CpuCounters>,
    disk_prev: Option<(DiskCounters, Instant)>,
    net_prev: Option<(NetCounters, Instant)>,
    net_ema: Option<(f64, f64)>,
    rate_windows: HashMap<ScaleKey, VecDeque<RatePoint>>,
    rnd_rng: SmallRng,
    rnd_state: Option<f64>,
    rnd_instance_rngs: HashMap<usize, SmallRng>,
    rnd_instance_states: HashMap<usize, Option<f64>>,
}

impl Default for Sampler {
    fn default() -> Self {
        Self {
            cpu_prev: None,
            disk_prev: None,
            net_prev: None,
            net_ema: None,
            rate_windows: HashMap::new(),
            rnd_rng: SmallRng::from_entropy(),
            rnd_state: None,
            rnd_instance_rngs: HashMap::new(),
            rnd_instance_states: HashMap::new(),
        }
    }
}

impl Sampler {
    pub fn prime(&mut self, metrics: &[MetricKind]) -> io::Result<()> {
        if metrics.contains(&MetricKind::Cpu) || metrics.contains(&MetricKind::Xpu) {
            self.cpu_prev = best_effort_host_metric(read_cpu_counters)?;
        }
        if metrics.contains(&MetricKind::Io)
            || metrics.contains(&MetricKind::In)
            || metrics.contains(&MetricKind::Out)
        {
            self.disk_prev = best_effort_host_metric(read_disk_counters)?
                .map(|counters| (counters, Instant::now()));
        }
        if metrics.contains(&MetricKind::Ingress)
            || metrics.contains(&MetricKind::Egress)
            || metrics.contains(&MetricKind::Net)
        {
            self.net_prev = best_effort_host_metric(read_net_counters)?
                .map(|counters| (counters, Instant::now()));
        }
        Ok(())
    }

    pub fn sample(&mut self, metrics: &[MetricKind]) -> io::Result<Sample> {
        let mut values = HashMap::new();
        let mut headlines = HashMap::new();

        let cpu_value = if metrics.contains(&MetricKind::Cpu)
            || metrics.contains(&MetricKind::Xpu)
            || metrics.contains(&MetricKind::Sys)
        {
            self.sample_cpu()?
        } else {
            None
        };

        let rnd_value = if metrics.contains(&MetricKind::Rnd) {
            Some(self.sample_rnd())
        } else {
            None
        };

        let gpu_sample = if metrics.contains(&MetricKind::Gpu)
            || metrics.contains(&MetricKind::Vram)
            || metrics.contains(&MetricKind::Xpu)
            || metrics.contains(&MetricKind::Gfx)
            || metrics.contains(&MetricKind::Mem)
        {
            Some(read_gpu_sample().unwrap_or_default())
        } else {
            None
        };

        let gpu_value = if metrics.contains(&MetricKind::Gpu)
            || metrics.contains(&MetricKind::Xpu)
            || metrics.contains(&MetricKind::Gfx)
        {
            gpu_sample.unwrap_or_default().utilization
        } else {
            None
        };

        let vram_value = if metrics.contains(&MetricKind::Vram)
            || metrics.contains(&MetricKind::Gfx)
            || metrics.contains(&MetricKind::Mem)
        {
            gpu_sample.unwrap_or_default().vram_fraction()
        } else {
            None
        };

        let memory_value = if metrics.contains(&MetricKind::Memory)
            || metrics.contains(&MetricKind::Sys)
            || metrics.contains(&MetricKind::Mem)
        {
            best_effort_host_metric(read_memory_usage)?
        } else {
            None
        };

        let storage_value = if metrics.contains(&MetricKind::Storage) {
            best_effort_host_metric(|| read_storage_usage("/"))?
        } else {
            None
        };

        let io_value = if metrics.contains(&MetricKind::Io)
            || metrics.contains(&MetricKind::In)
            || metrics.contains(&MetricKind::Out)
        {
            self.sample_io()?
        } else {
            None
        };

        let net_value = if metrics.contains(&MetricKind::Ingress)
            || metrics.contains(&MetricKind::Egress)
            || metrics.contains(&MetricKind::Net)
        {
            self.sample_net()?
        } else {
            None
        };

        for metric in metrics {
            let value = match metric {
                MetricKind::Cpu => cpu_value.map(MetricValue::Single),
                MetricKind::Xpu => match (cpu_value, gpu_value) {
                    (Some(upper), Some(lower)) => Some(MetricValue::Split { upper, lower }),
                    (Some(upper), None) => Some(MetricValue::Split { upper, lower: 0.0 }),
                    (None, Some(lower)) => Some(MetricValue::Split { upper: 0.0, lower }),
                    (None, None) => None,
                },
                MetricKind::Rnd => rnd_value.map(|sample| MetricValue::Single(sample.normalized)),
                MetricKind::Sys => match (cpu_value, memory_value.map(|sample| sample.usage_ratio))
                {
                    (Some(upper), Some(lower)) => Some(MetricValue::Split { upper, lower }),
                    (Some(upper), None) => Some(MetricValue::Split { upper, lower: 0.0 }),
                    (None, Some(lower)) => Some(MetricValue::Split { upper: 0.0, lower }),
                    (None, None) => None,
                },
                MetricKind::Gpu => gpu_value.map(MetricValue::Single),
                MetricKind::Vram => vram_value.map(MetricValue::Single),
                MetricKind::Gfx => match (gpu_value, vram_value) {
                    (Some(upper), Some(lower)) => Some(MetricValue::Split { upper, lower }),
                    (Some(upper), None) => Some(MetricValue::Split { upper, lower: 0.0 }),
                    (None, Some(lower)) => Some(MetricValue::Split { upper: 0.0, lower }),
                    (None, None) => None,
                },
                MetricKind::Memory => {
                    memory_value.map(|sample| MetricValue::Single(sample.usage_ratio))
                }
                MetricKind::Mem => {
                    match (memory_value.map(|sample| sample.usage_ratio), vram_value) {
                        (Some(upper), Some(lower)) => Some(MetricValue::Split { upper, lower }),
                        (Some(upper), None) => Some(MetricValue::Split { upper, lower: 0.0 }),
                        (None, Some(lower)) => Some(MetricValue::Split { upper: 0.0, lower }),
                        (None, None) => None,
                    }
                }
                MetricKind::Storage => {
                    storage_value.map(|sample| MetricValue::Single(sample.usage_ratio))
                }
                MetricKind::Io => io_value.map(|sample| sample.value),
                MetricKind::In => io_value.map(|sample| MetricValue::Single(sample.value.lower())),
                MetricKind::Out => io_value.map(|sample| MetricValue::Single(sample.value.upper())),
                MetricKind::Net => net_value.map(|sample| sample.value),
                MetricKind::Ingress => {
                    net_value.map(|sample| MetricValue::Single(sample.value.upper()))
                }
                MetricKind::Egress => {
                    net_value.map(|sample| MetricValue::Single(sample.value.lower()))
                }
            };
            if let Some(value) = value {
                let value = clamp_value(value);
                let headline = match metric {
                    MetricKind::Io => io_value
                        .map(|sample| HeadlineValue::Scalar(sample.upper_raw + sample.lower_raw)),
                    MetricKind::In => {
                        io_value.map(|sample| HeadlineValue::Scalar(sample.lower_raw))
                    }
                    MetricKind::Out => {
                        io_value.map(|sample| HeadlineValue::Scalar(sample.upper_raw))
                    }
                    MetricKind::Net => net_value
                        .map(|sample| HeadlineValue::Scalar(sample.upper_raw + sample.lower_raw)),
                    MetricKind::Ingress => {
                        net_value.map(|sample| HeadlineValue::Scalar(sample.upper_raw))
                    }
                    MetricKind::Egress => {
                        net_value.map(|sample| HeadlineValue::Scalar(sample.lower_raw))
                    }
                    MetricKind::Storage => storage_value.map(|sample| HeadlineValue::Storage {
                        used_bytes: sample.used_bytes,
                        total_bytes: sample.total_bytes,
                    }),
                    MetricKind::Memory => memory_value.map(|sample| HeadlineValue::Memory {
                        used_bytes: sample.used_bytes,
                        available_bytes: sample.available_bytes,
                        total_bytes: sample.total_bytes,
                    }),
                    MetricKind::Rnd => {
                        rnd_value.map(|sample| HeadlineValue::Scalar(sample.absolute))
                    }
                    _ => Some(HeadlineValue::Scalar(value.headline_value())),
                }
                .unwrap_or_else(|| HeadlineValue::Scalar(value.headline_value()));

                values.insert(*metric, value);
                headlines.insert(*metric, headline);
            }
        }

        Ok(Sample { values, headlines })
    }

    pub fn sample_canonical(&mut self, metrics: &[MetricKind]) -> io::Result<CanonicalSample> {
        let sample = self.sample(metrics)?;
        let mut values = HashMap::with_capacity(sample.values.len());
        let mut headlines = HashMap::with_capacity(sample.headlines.len());

        for metric in metrics {
            let source = Source::Metric(*metric);
            if let Some(value) = sample.values.get(metric).copied() {
                let headline = sample
                    .headlines
                    .get(metric)
                    .copied()
                    .unwrap_or_else(|| HeadlineValue::Scalar(value.headline_value()));
                values.insert(
                    source.clone(),
                    canonicalize_metric_value(*metric, value, headline),
                );
                headlines.insert(source, headline);
            }
        }

        Ok(CanonicalSample { values, headlines })
    }

    pub(crate) fn sample_rnd_instance(&mut self, instance: usize) -> RandomSample {
        let rng = self.rnd_instance_rngs.entry(instance).or_insert_with(|| {
            SmallRng::seed_from_u64(0x6d6f_6e6c_696e_0000_u64 ^ instance as u64)
        });
        let state = self.rnd_instance_states.entry(instance).or_insert(None);
        sample_rnd_value(rng, state)
    }

    fn sample_cpu(&mut self) -> io::Result<Option<f64>> {
        let Some(current) = best_effort_host_metric(read_cpu_counters)? else {
            self.cpu_prev = None;
            return Ok(None);
        };
        let usage = self
            .cpu_prev
            .map(|prev| cpu_usage(prev, current))
            .unwrap_or(0.0);
        self.cpu_prev = Some(current);
        Ok(Some(usage))
    }

    fn sample_io(&mut self) -> io::Result<Option<RateSample>> {
        let Some(current) = best_effort_host_metric(read_disk_counters)? else {
            self.disk_prev = None;
            return Ok(None);
        };
        let now = Instant::now();
        let usage = if let Some((prev, at)) = self.disk_prev {
            let dt = now.duration_since(at).as_secs_f64();
            let read_rate = rate_from_counters(prev.read_bytes, current.read_bytes, dt);
            let write_rate = rate_from_counters(prev.write_bytes, current.write_bytes, dt);
            RateSample {
                value: MetricValue::Split {
                    upper: self.normalize_rate(ScaleKey::IoWrite, write_rate, now),
                    lower: self.normalize_rate(ScaleKey::IoRead, read_rate, now),
                },
                upper_raw: write_rate,
                lower_raw: read_rate,
            }
        } else {
            RateSample {
                value: MetricValue::Split {
                    upper: 0.0,
                    lower: 0.0,
                },
                upper_raw: 0.0,
                lower_raw: 0.0,
            }
        };
        self.disk_prev = Some((current, now));
        Ok(Some(usage))
    }

    fn sample_net(&mut self) -> io::Result<Option<RateSample>> {
        let Some(current) = best_effort_host_metric(read_net_counters)? else {
            self.net_prev = None;
            return Ok(None);
        };
        let now = Instant::now();
        let usage = if let Some((prev, at)) = self.net_prev {
            let dt = now.duration_since(at).as_secs_f64();
            let ingress_rate = rate_from_counters(prev.rx_bytes, current.rx_bytes, dt);
            let egress_rate = rate_from_counters(prev.tx_bytes, current.tx_bytes, dt);
            let (ingress_rate, egress_rate) = self.smooth_net_rates(ingress_rate, egress_rate);
            RateSample {
                value: MetricValue::Split {
                    upper: self.normalize_rate(ScaleKey::NetIngress, ingress_rate, now),
                    lower: self.normalize_rate(ScaleKey::NetEgress, egress_rate, now),
                },
                upper_raw: ingress_rate,
                lower_raw: egress_rate,
            }
        } else {
            RateSample {
                value: MetricValue::Split {
                    upper: 0.0,
                    lower: 0.0,
                },
                upper_raw: 0.0,
                lower_raw: 0.0,
            }
        };
        self.net_prev = Some((current, now));
        Ok(Some(usage))
    }

    fn sample_rnd(&mut self) -> RandomSample {
        sample_rnd_value(&mut self.rnd_rng, &mut self.rnd_state)
    }

    fn smooth_net_rates(&mut self, ingress_rate: f64, egress_rate: f64) -> (f64, f64) {
        const NET_EMA_ALPHA: f64 = 0.35;

        let current = (ingress_rate.max(0.0), egress_rate.max(0.0));
        let smoothed = if let Some((previous_ingress, previous_egress)) = self.net_ema {
            (
                (NET_EMA_ALPHA * current.0) + ((1.0 - NET_EMA_ALPHA) * previous_ingress),
                (NET_EMA_ALPHA * current.1) + ((1.0 - NET_EMA_ALPHA) * previous_egress),
            )
        } else {
            current
        };
        self.net_ema = Some(smoothed);
        smoothed
    }

    fn normalize_rate(&mut self, metric: ScaleKey, rate: f64, now: Instant) -> f64 {
        const RATE_SCALE_WINDOW: Duration = Duration::from_secs(8);

        let window = self.rate_windows.entry(metric).or_default();
        window.push_back(RatePoint {
            at: now,
            value: rate.max(0.0),
        });
        trim_rate_window(window, now, RATE_SCALE_WINDOW);

        let scale = window
            .iter()
            .map(|point| point.value)
            .fold(1.0_f64, f64::max);
        (rate / scale).clamp(0.0, 1.0)
    }
}

fn sample_rnd_value(rng: &mut SmallRng, state: &mut Option<f64>) -> RandomSample {
    // A beta-distributed target keeps samples mostly in the mid/low range,
    // while an AR step keeps the graph feeling like a plausible timeseries.
    let beta = Beta::new(2.2, 3.8).expect("valid beta parameters");
    let noise = Normal::new(0.0, 0.08).expect("valid normal parameters");

    let target = beta.sample(rng);
    let current = state.unwrap_or(target);
    let next = (0.82 * current + 0.18 * target + noise.sample(rng)).clamp(0.0, 1.0);
    *state = Some(next);

    // Map into a wide byte-like domain so rnd.abs produces compact SI values.
    let absolute = if next <= f64::EPSILON {
        0.0
    } else {
        1024_f64.powf(next * 4.0)
    };

    RandomSample {
        normalized: next,
        absolute,
    }
}

fn canonicalize_metric_value(
    metric: MetricKind,
    value: MetricValue,
    headline: HeadlineValue,
) -> CanonicalValue {
    match (metric, value) {
        (MetricKind::Cpu, MetricValue::Single(normalized))
        | (MetricKind::Rnd, MetricValue::Single(normalized))
        | (MetricKind::Gpu, MetricValue::Single(normalized))
        | (MetricKind::Vram, MetricValue::Single(normalized))
        | (MetricKind::Memory, MetricValue::Single(normalized))
        | (MetricKind::Storage, MetricValue::Single(normalized))
        | (MetricKind::In, MetricValue::Single(normalized))
        | (MetricKind::Out, MetricValue::Single(normalized))
        | (MetricKind::Ingress, MetricValue::Single(normalized))
        | (MetricKind::Egress, MetricValue::Single(normalized)) => CanonicalValue::Scalar {
            normalized,
            absolute: canonical_metric_absolute(metric, headline),
        },
        (MetricKind::Xpu, MetricValue::Split { upper, lower })
        | (MetricKind::Sys, MetricValue::Split { upper, lower })
        | (MetricKind::Gfx, MetricValue::Split { upper, lower }) => CanonicalValue::Split {
            upper_normalized: upper,
            lower_normalized: lower,
            upper_absolute: None,
            lower_absolute: None,
        },
        (MetricKind::Mem, MetricValue::Split { upper, lower }) => CanonicalValue::Split {
            upper_normalized: upper,
            lower_normalized: lower,
            upper_absolute: None,
            lower_absolute: None,
        },
        (MetricKind::Io, MetricValue::Split { upper, lower })
        | (MetricKind::Net, MetricValue::Split { upper, lower }) => {
            let total = headline.scalar().unwrap_or(0.0);
            CanonicalValue::Split {
                upper_normalized: upper,
                lower_normalized: lower,
                upper_absolute: Some(total),
                lower_absolute: Some(total),
            }
        }
        (_, MetricValue::Single(normalized)) => CanonicalValue::Scalar {
            normalized,
            absolute: canonical_metric_absolute(metric, headline),
        },
        (_, MetricValue::Split { upper, lower }) => CanonicalValue::Split {
            upper_normalized: upper,
            lower_normalized: lower,
            upper_absolute: None,
            lower_absolute: None,
        },
    }
}

fn canonical_metric_absolute(metric: MetricKind, headline: HeadlineValue) -> Option<f64> {
    match (metric, headline) {
        (_, HeadlineValue::Scalar(value)) => Some(value.max(0.0)),
        (
            MetricKind::Memory,
            HeadlineValue::Memory {
                available_bytes, ..
            },
        ) => Some(available_bytes as f64),
        (
            MetricKind::Storage,
            HeadlineValue::Storage {
                used_bytes,
                total_bytes,
            },
        ) => Some(total_bytes.saturating_sub(used_bytes) as f64),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RandomSample {
    pub(crate) normalized: f64,
    pub(crate) absolute: f64,
}

fn trim_rate_window(window: &mut VecDeque<RatePoint>, now: Instant, max_age: Duration) {
    while let Some(front) = window.front() {
        if now.duration_since(front.at) <= max_age {
            break;
        }
        window.pop_front();
    }
}

fn best_effort_host_metric<T, F>(read: F) -> io::Result<Option<T>>
where
    F: FnOnce() -> io::Result<T>,
{
    match read() {
        Ok(value) => Ok(Some(value)),
        Err(error) if host_metric_unavailable(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

fn host_metric_unavailable(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied
    )
}

#[derive(Clone, Copy, Debug)]
struct StorageSample {
    usage_ratio: f64,
    used_bytes: u64,
    total_bytes: u64,
}

impl GpuSample {
    fn vram_fraction(self) -> Option<f64> {
        let used = self.vram_used_bytes?;
        let total = self.vram_total_bytes?;
        if total == 0 {
            return Some(0.0);
        }
        Some((used as f64 / total as f64).clamp(0.0, 1.0))
    }
}

fn clamp_value(value: MetricValue) -> MetricValue {
    match value {
        MetricValue::Single(value) => MetricValue::Single(value.clamp(0.0, 1.0)),
        MetricValue::Split { upper, lower } => MetricValue::Split {
            upper: upper.clamp(0.0, 1.0),
            lower: lower.clamp(0.0, 1.0),
        },
    }
}

fn rate_from_counters(previous: u64, current: u64, seconds: f64) -> f64 {
    if seconds <= 0.0 {
        return 0.0;
    }
    current.saturating_sub(previous) as f64 / seconds
}

fn read_cpu_counters() -> io::Result<CpuCounters> {
    let stat = fs::read_to_string("/proc/stat")?;
    parse_cpu_counters(&stat)
}

fn parse_cpu_counters(stat: &str) -> io::Result<CpuCounters> {
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

    Ok(CpuCounters {
        idle: idle + iowait,
        total: user + nice + system + idle + iowait + irq + softirq + steal + guest + guest_nice,
    })
}

fn next_u64<'a, I>(fields: &mut I) -> io::Result<u64>
where
    I: Iterator<Item = &'a str>,
{
    fields
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing cpu field"))?
        .parse::<u64>()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid cpu field"))
}

fn cpu_usage(previous: CpuCounters, current: CpuCounters) -> f64 {
    let total_delta = current.total.saturating_sub(previous.total);
    if total_delta == 0 {
        return 0.0;
    }
    let idle_delta = current.idle.saturating_sub(previous.idle);
    let active_delta = total_delta.saturating_sub(idle_delta);
    (active_delta as f64 / total_delta as f64).clamp(0.0, 1.0)
}

fn read_memory_usage() -> io::Result<MemorySample> {
    let meminfo = fs::read_to_string("/proc/meminfo")?;
    parse_memory_usage(&meminfo)
}

fn parse_memory_usage(meminfo: &str) -> io::Result<MemorySample> {
    let mut total: Option<u64> = None;
    let mut available: Option<u64> = None;

    for line in meminfo.lines() {
        if line.starts_with("MemTotal:") {
            total = line
                .split_whitespace()
                .nth(1)
                .and_then(|value| value.parse().ok());
        } else if line.starts_with("MemAvailable:") {
            available = line
                .split_whitespace()
                .nth(1)
                .and_then(|value| value.parse().ok());
        }
    }

    let total =
        total.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing MemTotal"))?;
    let available = available
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing MemAvailable"))?;
    if total == 0 {
        return Ok(MemorySample {
            usage_ratio: 0.0,
            used_bytes: 0,
            available_bytes: 0,
            total_bytes: 0,
        });
    }
    let total_bytes = total.saturating_mul(1024);
    let available_bytes = available.min(total).saturating_mul(1024);
    let used_bytes = total_bytes.saturating_sub(available_bytes);
    Ok(MemorySample {
        usage_ratio: (used_bytes as f64 / total_bytes as f64).clamp(0.0, 1.0),
        used_bytes,
        available_bytes,
        total_bytes,
    })
}

fn read_storage_usage(path: &str) -> io::Result<StorageSample> {
    let path = CString::new(path)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid storage path"))?;
    let mut stat = StatVfs {
        f_bsize: 0,
        f_frsize: 0,
        f_blocks: 0,
        f_bfree: 0,
        f_bavail: 0,
        f_files: 0,
        f_ffree: 0,
        f_favail: 0,
        f_fsid: 0,
        f_unused: 0,
        f_flag: 0,
        f_namemax: 0,
        f_spare: [0; 6],
    };

    let rc = unsafe { statvfs(path.as_ptr(), &mut stat) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }

    storage_usage_from_stat(&stat).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid filesystem statistics for storage usage",
        )
    })
}

fn storage_usage_from_stat(stat: &StatVfs) -> Option<StorageSample> {
    let block_size = stat.f_frsize.max(1);
    let total_blocks = stat.f_blocks;
    let available_blocks = stat.f_bavail.min(total_blocks);
    if total_blocks == 0 {
        return None;
    }

    let total_bytes = total_blocks.saturating_mul(block_size);
    let available_bytes = available_blocks.saturating_mul(block_size);
    let used_bytes = total_bytes.saturating_sub(available_bytes);
    let usage_ratio = (used_bytes as f64 / total_bytes as f64).clamp(0.0, 1.0);

    Some(StorageSample {
        usage_ratio,
        used_bytes,
        total_bytes,
    })
}

fn read_disk_counters() -> io::Result<DiskCounters> {
    let diskstats = fs::read_to_string("/proc/diskstats")?;
    parse_disk_counters(&diskstats)
}

fn parse_disk_counters(diskstats: &str) -> io::Result<DiskCounters> {
    let mut read_sectors = 0_u64;
    let mut write_sectors = 0_u64;

    for line in diskstats.lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() < 10 {
            continue;
        }
        let name = fields[2];
        if !is_primary_disk(name) {
            continue;
        }
        let sectors_read = fields[5].parse::<u64>().unwrap_or(0);
        let sectors_written = fields[9].parse::<u64>().unwrap_or(0);
        read_sectors = read_sectors.saturating_add(sectors_read);
        write_sectors = write_sectors.saturating_add(sectors_written);
    }

    Ok(DiskCounters {
        read_bytes: read_sectors.saturating_mul(512),
        write_bytes: write_sectors.saturating_mul(512),
    })
}

fn is_primary_disk(name: &str) -> bool {
    name.starts_with("sd")
        || name.starts_with("vd")
        || name.starts_with("xvd")
        || name.starts_with("nvme")
        || name.starts_with("mmcblk")
        || name.starts_with("md")
}

fn read_net_counters() -> io::Result<NetCounters> {
    let netdev = fs::read_to_string("/proc/net/dev")?;
    let routed_ifaces = read_default_route_ifaces();
    let routed_ifaces = (!routed_ifaces.is_empty()).then_some(&routed_ifaces);
    parse_net_counters(&netdev, routed_ifaces)
}

fn parse_net_counters(
    netdev: &str,
    routed_ifaces: Option<&HashSet<String>>,
) -> io::Result<NetCounters> {
    let mut rx = 0_u64;
    let mut tx = 0_u64;

    for line in netdev.lines().skip(2) {
        let Some((name, data)) = line.split_once(':') else {
            continue;
        };
        let iface = name.trim();
        if iface == "lo" {
            continue;
        }
        if let Some(routed_ifaces) = routed_ifaces {
            if !routed_ifaces.contains(iface) {
                continue;
            }
        }
        let fields = data.split_whitespace().collect::<Vec<_>>();
        if fields.len() < 16 {
            continue;
        }
        rx = rx.saturating_add(fields[0].parse::<u64>().unwrap_or(0));
        tx = tx.saturating_add(fields[8].parse::<u64>().unwrap_or(0));
    }

    Ok(NetCounters {
        rx_bytes: rx,
        tx_bytes: tx,
    })
}

fn read_default_route_ifaces() -> HashSet<String> {
    let mut ifaces = HashSet::new();

    if let Ok(route) = fs::read_to_string("/proc/net/route") {
        parse_default_route_ifaces_v4(&route, &mut ifaces);
    }
    if let Ok(route) = fs::read_to_string("/proc/net/ipv6_route") {
        parse_default_route_ifaces_v6(&route, &mut ifaces);
    }

    ifaces
}

fn parse_default_route_ifaces_v4(route: &str, ifaces: &mut HashSet<String>) {
    for line in route.lines().skip(1) {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() < 8 {
            continue;
        }
        if fields[1] == "00000000" && fields[7] == "00000000" {
            ifaces.insert(fields[0].to_owned());
        }
    }
}

fn parse_default_route_ifaces_v6(route: &str, ifaces: &mut HashSet<String>) {
    for line in route.lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() < 10 {
            continue;
        }
        if fields[0] == "00000000000000000000000000000000" && fields[1] == "00000000" {
            ifaces.insert(fields[9].to_owned());
        }
    }
}

fn read_gpu_sample() -> io::Result<GpuSample> {
    if let Ok(sample) = read_nvidia_gpu_sample() {
        return Ok(sample);
    }
    read_generic_gpu_sample()
}

fn read_generic_gpu_sample() -> io::Result<GpuSample> {
    let entries = match fs::read_dir("/sys/class/drm") {
        Ok(entries) => entries,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied
            ) =>
        {
            return Ok(GpuSample::default());
        }
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("card") || name.contains('-') {
            continue;
        }
        let path = entry.path().join("device/gpu_busy_percent");
        if !path.exists() {
            continue;
        }
        let value = match fs::read_to_string(&path) {
            Ok(value) => value,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied
                ) =>
            {
                continue;
            }
            Err(error) => return Err(error),
        };
        if let Ok(percent) = value.trim().parse::<f64>() {
            return Ok(GpuSample {
                utilization: Some((percent / 100.0).clamp(0.0, 1.0)),
                ..GpuSample::default()
            });
        }
    }

    Ok(GpuSample::default())
}

fn read_nvidia_gpu_sample() -> io::Result<GpuSample> {
    let output = std::process::Command::new("nvidia-smi")
        .args([
            "--query-gpu=utilization.gpu,memory.used,memory.total",
            "--format=csv,noheader,nounits",
        ])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other("nvidia-smi failed"));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_nvidia_smi_csv(&stdout)
}

fn parse_nvidia_smi_csv(stdout: &str) -> io::Result<GpuSample> {
    let line = stdout
        .lines()
        .find(|line| !line.trim().is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing nvidia-smi output"))?;
    let fields = line
        .split(',')
        .map(|field| field.trim())
        .collect::<Vec<_>>();
    if fields.len() < 3 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid nvidia-smi output",
        ));
    }

    let utilization = fields[0]
        .parse::<f64>()
        .map(|value| (value / 100.0).clamp(0.0, 1.0))
        .ok();
    let memory_used_mib = fields[1].parse::<u64>().ok();
    let memory_total_mib = fields[2].parse::<u64>().ok();

    Ok(GpuSample {
        utilization,
        vram_used_bytes: memory_used_mib.map(|mib| mib.saturating_mul(1024 * 1024)),
        vram_total_bytes: memory_total_mib.map(|mib| mib.saturating_mul(1024 * 1024)),
    })
}

#[repr(C)]
struct StatVfs {
    f_bsize: c_ulong,
    f_frsize: c_ulong,
    f_blocks: u64,
    f_bfree: u64,
    f_bavail: u64,
    f_files: u64,
    f_ffree: u64,
    f_favail: u64,
    f_fsid: c_ulong,
    f_unused: c_ulong,
    f_flag: c_ulong,
    f_namemax: c_ulong,
    f_spare: [c_int; 6],
}

unsafe extern "C" {
    fn statvfs(path: *const c_char, buf: *mut StatVfs) -> c_int;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_delta_is_computed() {
        let prev = CpuCounters {
            idle: 100,
            total: 200,
        };
        let current = CpuCounters {
            idle: 130,
            total: 260,
        };
        assert!((cpu_usage(prev, current) - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn cpu_counters_are_parsed_from_aggregate_line() {
        let counters = parse_cpu_counters("cpu  10 20 30 40 50 60 70 80 90 100\n").unwrap();
        assert_eq!(counters.idle, 90);
        assert_eq!(counters.total, 550);
    }

    #[test]
    fn cpu_counters_require_an_aggregate_line() {
        let error = parse_cpu_counters("cpu0  10 20 30 40 50 60 70\n").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn cpu_counters_reject_invalid_fields() {
        let error = parse_cpu_counters("cpu  10 xx 30 40 50 60 70\n").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn memory_usage_is_parsed_from_meminfo() {
        let value =
            parse_memory_usage("MemTotal:       1000 kB\nMemAvailable:    250 kB\n").unwrap();
        assert!((value.usage_ratio - 0.75).abs() < f64::EPSILON);
        assert_eq!(value.total_bytes, 1000 * 1024);
        assert_eq!(value.available_bytes, 250 * 1024);
        assert_eq!(value.used_bytes, 750 * 1024);
    }

    #[test]
    fn memory_usage_requires_memavailable() {
        let error = parse_memory_usage("MemTotal:       1000 kB\n").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn memory_usage_handles_zero_total() {
        let value = parse_memory_usage("MemTotal:       0 kB\nMemAvailable:    0 kB\n").unwrap();
        assert_eq!(value.usage_ratio, 0.0);
        assert_eq!(value.total_bytes, 0);
        assert_eq!(value.available_bytes, 0);
        assert_eq!(value.used_bytes, 0);
    }

    #[test]
    fn storage_usage_is_computed_from_statvfs_values() {
        let stat = StatVfs {
            f_bsize: 4096,
            f_frsize: 4096,
            f_blocks: 100,
            f_bfree: 0,
            f_bavail: 25,
            f_files: 0,
            f_ffree: 0,
            f_favail: 0,
            f_fsid: 0,
            f_unused: 0,
            f_flag: 0,
            f_namemax: 0,
            f_spare: [0; 6],
        };

        let sample = storage_usage_from_stat(&stat).unwrap();
        assert!((sample.usage_ratio - 0.75).abs() < f64::EPSILON);
        assert_eq!(sample.used_bytes, 75 * 4096);
        assert_eq!(sample.total_bytes, 100 * 4096);
    }

    #[test]
    fn storage_usage_rejects_paths_with_nul_bytes() {
        let error = read_storage_usage("/tmp/\0bad").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn disk_counters_skip_loop_devices() {
        let counters = parse_disk_counters(
            "   7       0 loop0 1 2 3 4 5 6 7 8 9 10 11\n\
               8       0 sda   1 2 3 4 5 6 7 8 9 10 11\n",
        )
        .unwrap();
        assert_eq!(counters.read_bytes, 3 * 512);
        assert_eq!(counters.write_bytes, 7 * 512);
    }

    #[test]
    fn net_counters_skip_loopback() {
        let counters = parse_net_counters(
            "Inter-|   Receive                                                |  Transmit\n\
             face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed\n\
             lo: 10 0 0 0 0 0 0 0 20 0 0 0 0 0 0 0\n\
             eth0: 100 0 0 0 0 0 0 0 200 0 0 0 0 0 0 0\n",
            None,
        )
        .unwrap();
        assert_eq!(counters.rx_bytes, 100);
        assert_eq!(counters.tx_bytes, 200);
    }

    #[test]
    fn net_counters_prefer_routed_interfaces_when_available() {
        let counters = parse_net_counters(
            "Inter-|   Receive                                                |  Transmit\n\
             face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed\n\
             wlp0s20f3: 100 0 0 0 0 0 0 0 200 0 0 0 0 0 0 0\n\
             tailscale0: 300 0 0 0 0 0 0 0 400 0 0 0 0 0 0 0\n",
            Some(&HashSet::from([String::from("wlp0s20f3")])),
        )
        .unwrap();
        assert_eq!(counters.rx_bytes, 100);
        assert_eq!(counters.tx_bytes, 200);
    }

    #[test]
    fn parses_default_route_interfaces_from_procfs_views() {
        let mut ifaces = HashSet::new();

        parse_default_route_ifaces_v4(
            "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\n\
             wlp0s20f3\t00000000\t01020304\t0003\t0\t0\t0\t00000000\n\
             tailscale0\t0008FE64\t00000000\t0001\t0\t0\t0\t00FFFFFF\n",
            &mut ifaces,
        );
        parse_default_route_ifaces_v6(
            "00000000000000000000000000000000 00000000 00000000000000000000000000000000 00000000 00000000000000000000000000000000 00000000 00000000 00000000 00000000 tailscale0\n",
            &mut ifaces,
        );

        assert!(ifaces.contains("wlp0s20f3"));
        assert!(ifaces.contains("tailscale0"));
    }

    #[test]
    fn split_values_headline_on_the_stronger_side() {
        let value = MetricValue::Split {
            upper: 0.2,
            lower: 0.8,
        };
        assert!((value.headline_value() - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn parses_nvidia_gpu_and_vram_separately() {
        let sample = parse_nvidia_smi_csv("35, 1024, 4096\n").unwrap();
        assert_eq!(sample.utilization, Some(0.35));
        assert_eq!(sample.vram_fraction(), Some(0.25));
    }

    #[test]
    fn nvidia_parser_rejects_missing_output() {
        let error = parse_nvidia_smi_csv("\n\n").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn nvidia_parser_rejects_truncated_rows() {
        let error = parse_nvidia_smi_csv("35, 1024\n").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn gfx_can_degrade_to_gpu_only() {
        let value = match (Some(0.35), None) {
            (Some(upper), Some(lower)) => Some(MetricValue::Split { upper, lower }),
            (Some(upper), None) => Some(MetricValue::Split { upper, lower: 0.0 }),
            (None, Some(lower)) => Some(MetricValue::Split { upper: 0.0, lower }),
            (None, None) => None,
        };

        assert_eq!(
            value,
            Some(MetricValue::Split {
                upper: 0.35,
                lower: 0.0,
            })
        );
    }

    #[test]
    fn rate_scaling_keeps_recent_spikes_in_the_window() {
        let mut sampler = Sampler::default();
        let start = Instant::now();

        assert!(
            (sampler.normalize_rate(ScaleKey::NetIngress, 1000.0, start) - 1.0).abs()
                < f64::EPSILON
        );

        let normalized =
            sampler.normalize_rate(ScaleKey::NetIngress, 100.0, start + Duration::from_secs(4));

        assert!((normalized - 0.1).abs() < 1e-9);
    }

    #[test]
    fn rate_scaling_recovers_after_old_spikes_age_out() {
        let mut sampler = Sampler::default();
        let start = Instant::now();

        sampler.normalize_rate(ScaleKey::IoWrite, 1000.0, start);
        let normalized =
            sampler.normalize_rate(ScaleKey::IoWrite, 100.0, start + Duration::from_secs(9));

        assert!((normalized - 1.0).abs() < 1e-9);
    }

    #[test]
    fn net_rate_smoothing_damps_spikes() {
        let mut sampler = Sampler::default();

        let first = sampler.smooth_net_rates(1000.0, 500.0);
        assert_eq!(first, (1000.0, 500.0));

        let second = sampler.smooth_net_rates(0.0, 0.0);
        assert!((second.0 - 650.0).abs() < 1e-9);
        assert!((second.1 - 325.0).abs() < 1e-9);
    }

    #[test]
    fn rate_from_counters_handles_zero_and_positive_intervals() {
        assert_eq!(rate_from_counters(100, 200, 0.0), 0.0);
        assert_eq!(rate_from_counters(100, 300, 2.0), 100.0);
    }

    #[test]
    fn host_metric_readers_are_best_effort_successes() {
        let mut sampler = Sampler::default();
        let metrics = [
            MetricKind::Memory,
            MetricKind::Storage,
            MetricKind::Gpu,
            MetricKind::Io,
            MetricKind::Net,
        ];

        sampler.prime(&metrics).unwrap();
        std::thread::sleep(Duration::from_millis(5));

        let sample = sampler.sample(&metrics).unwrap();

        for metric in [
            MetricKind::Memory,
            MetricKind::Storage,
            MetricKind::Io,
            MetricKind::Net,
        ] {
            if let Some(value) = sample.values.get(&metric) {
                assert!(sample.headlines.contains_key(&metric));
                match value {
                    MetricValue::Single(value) => {
                        assert!((0.0..=1.0).contains(value));
                    }
                    MetricValue::Split { upper, lower } => {
                        assert!((0.0..=1.0).contains(upper));
                        assert!((0.0..=1.0).contains(lower));
                    }
                }
            }
        }
    }

    #[test]
    fn rnd_metric_produces_bounded_values_and_scalar_headlines() {
        let mut sampler = Sampler::default();
        let sample = sampler.sample(&[MetricKind::Rnd]).unwrap();

        let value = sample.values.get(&MetricKind::Rnd).copied().unwrap();
        let headline = sample.headlines.get(&MetricKind::Rnd).copied().unwrap();

        match value {
            MetricValue::Single(value) => assert!((0.0..=1.0).contains(&value)),
            MetricValue::Split { .. } => panic!("rnd should be a single-value metric"),
        }

        match headline {
            HeadlineValue::Scalar(value) => assert!(value >= 0.0),
            HeadlineValue::Memory { .. } | HeadlineValue::Storage { .. } => {
                panic!("rnd should expose a scalar headline")
            }
        }
    }

    #[test]
    fn direct_host_readers_execute_successfully() {
        let memory = best_effort_host_metric(read_memory_usage).unwrap();
        let storage = best_effort_host_metric(|| read_storage_usage("/")).unwrap();
        let disk = best_effort_host_metric(read_disk_counters).unwrap();
        let net = best_effort_host_metric(read_net_counters).unwrap();
        let gpu = read_generic_gpu_sample().unwrap();

        if let Some(memory) = memory {
            assert!((0.0..=1.0).contains(&memory.usage_ratio));
            assert!(memory.total_bytes >= memory.used_bytes);
            assert!(memory.total_bytes >= memory.available_bytes);
        }
        if let Some(storage) = storage {
            assert!((0.0..=1.0).contains(&storage.usage_ratio));
            assert!(storage.total_bytes >= storage.used_bytes);
        }
        let _ = disk;
        let _ = net;
        assert!(gpu
            .utilization
            .is_none_or(|value| (0.0..=1.0).contains(&value)));
    }

    #[test]
    fn nvidia_probe_executes_even_when_unavailable() {
        match read_nvidia_gpu_sample() {
            Ok(sample) => {
                assert!(sample
                    .utilization
                    .is_none_or(|value| (0.0..=1.0).contains(&value)));
            }
            Err(error) => {
                assert!(matches!(
                    error.kind(),
                    io::ErrorKind::NotFound
                        | io::ErrorKind::Other
                        | io::ErrorKind::PermissionDenied
                        | io::ErrorKind::InvalidData
                ));
            }
        }
    }
}
