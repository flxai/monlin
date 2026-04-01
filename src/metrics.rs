use std::collections::HashMap;
use std::fs;
use std::io;
use std::time::Instant;

use crate::layout::MetricKind;

#[derive(Clone, Copy, Debug)]
struct CpuCounters {
    idle: u64,
    total: u64,
}

#[derive(Clone, Copy, Debug)]
struct DiskCounters {
    bytes: u64,
}

#[derive(Clone, Copy, Debug)]
struct NetCounters {
    rx_bytes: u64,
    tx_bytes: u64,
}

#[derive(Debug)]
pub struct Sampler {
    cpu_prev: Option<CpuCounters>,
    disk_prev: Option<(DiskCounters, Instant)>,
    net_prev: Option<(NetCounters, Instant)>,
    scale_maxima: HashMap<MetricKind, f64>,
}

impl Default for Sampler {
    fn default() -> Self {
        Self {
            cpu_prev: None,
            disk_prev: None,
            net_prev: None,
            scale_maxima: HashMap::new(),
        }
    }
}

impl Sampler {
    pub fn prime(&mut self, metrics: &[MetricKind]) -> io::Result<()> {
        if metrics.contains(&MetricKind::Cpu) {
            self.cpu_prev = Some(read_cpu_counters()?);
        }
        if metrics.contains(&MetricKind::Io) {
            self.disk_prev = Some((read_disk_counters()?, Instant::now()));
        }
        if metrics.contains(&MetricKind::Ingress) || metrics.contains(&MetricKind::Egress) {
            self.net_prev = Some((read_net_counters()?, Instant::now()));
        }
        Ok(())
    }

    pub fn sample(&mut self, metrics: &[MetricKind]) -> io::Result<HashMap<MetricKind, f64>> {
        let mut values = HashMap::new();

        for metric in metrics {
            let value = match metric {
                MetricKind::Cpu => self.sample_cpu()?,
                MetricKind::Gpu => read_gpu_usage().unwrap_or(0.0),
                MetricKind::Memory => read_memory_usage()?,
                MetricKind::Io => self.sample_io()?,
                MetricKind::Ingress => self.sample_net(true)?,
                MetricKind::Egress => self.sample_net(false)?,
            };
            values.insert(*metric, value.clamp(0.0, 1.0));
        }

        Ok(values)
    }

    fn sample_cpu(&mut self) -> io::Result<f64> {
        let current = read_cpu_counters()?;
        let usage = self
            .cpu_prev
            .map(|prev| cpu_usage(prev, current))
            .unwrap_or(0.0);
        self.cpu_prev = Some(current);
        Ok(usage)
    }

    fn sample_io(&mut self) -> io::Result<f64> {
        let current = read_disk_counters()?;
        let now = Instant::now();
        let usage = if let Some((prev, at)) = self.disk_prev {
            let dt = now.duration_since(at).as_secs_f64();
            let rate = rate_from_counters(prev.bytes, current.bytes, dt);
            self.normalize_rate(MetricKind::Io, rate)
        } else {
            0.0
        };
        self.disk_prev = Some((current, now));
        Ok(usage)
    }

    fn sample_net(&mut self, ingress: bool) -> io::Result<f64> {
        let current = read_net_counters()?;
        let now = Instant::now();
        let usage = if let Some((prev, at)) = self.net_prev {
            let dt = now.duration_since(at).as_secs_f64();
            let current_value = if ingress {
                current.rx_bytes
            } else {
                current.tx_bytes
            };
            let prev_value = if ingress { prev.rx_bytes } else { prev.tx_bytes };
            let rate = rate_from_counters(prev_value, current_value, dt);
            self.normalize_rate(
                if ingress {
                    MetricKind::Ingress
                } else {
                    MetricKind::Egress
                },
                rate,
            )
        } else {
            0.0
        };
        self.net_prev = Some((current, now));
        Ok(usage)
    }

    fn normalize_rate(&mut self, metric: MetricKind, rate: f64) -> f64 {
        let entry = self.scale_maxima.entry(metric).or_insert(rate.max(1.0));
        *entry = (entry.mul_add(0.97, 0.0)).max(rate).max(1.0);
        (rate / *entry).clamp(0.0, 1.0)
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

fn read_memory_usage() -> io::Result<f64> {
    let meminfo = fs::read_to_string("/proc/meminfo")?;
    parse_memory_usage(&meminfo)
}

fn parse_memory_usage(meminfo: &str) -> io::Result<f64> {
    let mut total: Option<u64> = None;
    let mut available: Option<u64> = None;

    for line in meminfo.lines() {
        if line.starts_with("MemTotal:") {
            total = line.split_whitespace().nth(1).and_then(|value| value.parse().ok());
        } else if line.starts_with("MemAvailable:") {
            available = line.split_whitespace().nth(1).and_then(|value| value.parse().ok());
        }
    }

    let total = total.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing MemTotal"))?;
    let available =
        available.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing MemAvailable"))?;
    if total == 0 {
        return Ok(0.0);
    }
    Ok(((total - available) as f64 / total as f64).clamp(0.0, 1.0))
}

fn read_disk_counters() -> io::Result<DiskCounters> {
    let diskstats = fs::read_to_string("/proc/diskstats")?;
    parse_disk_counters(&diskstats)
}

fn parse_disk_counters(diskstats: &str) -> io::Result<DiskCounters> {
    let mut sectors = 0_u64;

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
        sectors = sectors.saturating_add(sectors_read.saturating_add(sectors_written));
    }

    Ok(DiskCounters {
        bytes: sectors.saturating_mul(512),
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
    parse_net_counters(&netdev)
}

fn parse_net_counters(netdev: &str) -> io::Result<NetCounters> {
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

fn read_gpu_usage() -> io::Result<f64> {
    let entries = fs::read_dir("/sys/class/drm")?;
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
        let value = fs::read_to_string(path)?;
        if let Ok(percent) = value.trim().parse::<f64>() {
            return Ok((percent / 100.0).clamp(0.0, 1.0));
        }
    }

    Ok(0.0)
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
    fn memory_usage_is_parsed_from_meminfo() {
        let value = parse_memory_usage(
            "MemTotal:       1000 kB\nMemAvailable:    250 kB\n",
        )
        .unwrap();
        assert!((value - 0.75).abs() < f64::EPSILON);
    }

    #[test]
    fn disk_counters_skip_loop_devices() {
        let counters = parse_disk_counters(
            "   7       0 loop0 1 2 3 4 5 6 7 8 9 10 11\n\
               8       0 sda   1 2 3 4 5 6 7 8 9 10 11\n",
        )
        .unwrap();
        assert_eq!(counters.bytes, (3 + 7) * 512);
    }

    #[test]
    fn net_counters_skip_loopback() {
        let counters = parse_net_counters(
            "Inter-|   Receive                                                |  Transmit\n\
             face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed\n\
             lo: 10 0 0 0 0 0 0 0 20 0 0 0 0 0 0 0\n\
             eth0: 100 0 0 0 0 0 0 0 200 0 0 0 0 0 0 0\n",
        )
        .unwrap();
        assert_eq!(counters.rx_bytes, 100);
        assert_eq!(counters.tx_bytes, 200);
    }
}
