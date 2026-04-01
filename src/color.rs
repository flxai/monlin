use crate::layout::MetricKind;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Gradient {
    pub low: Rgb,
    pub high: Rgb,
}

pub fn gradient_for(metric: MetricKind) -> Gradient {
    match metric {
        MetricKind::Cpu => Gradient {
            low: Rgb {
                r: 184,
                g: 220,
                b: 255,
            },
            high: Rgb {
                r: 20,
                g: 92,
                b: 214,
            },
        },
        MetricKind::Gpu => Gradient {
            low: Rgb {
                r: 191,
                g: 247,
                b: 200,
            },
            high: Rgb {
                r: 24,
                g: 138,
                b: 61,
            },
        },
        MetricKind::Memory => Gradient {
            low: Rgb {
                r: 255,
                g: 202,
                b: 202,
            },
            high: Rgb {
                r: 194,
                g: 28,
                b: 28,
            },
        },
        MetricKind::Io => Gradient {
            low: Rgb {
                r: 255,
                g: 240,
                b: 179,
            },
            high: Rgb {
                r: 201,
                g: 146,
                b: 15,
            },
        },
        MetricKind::Ingress => Gradient {
            low: Rgb {
                r: 190,
                g: 244,
                b: 255,
            },
            high: Rgb {
                r: 11,
                g: 134,
                b: 168,
            },
        },
        MetricKind::Egress => Gradient {
            low: Rgb {
                r: 243,
                g: 204,
                b: 255,
            },
            high: Rgb {
                r: 166,
                g: 37,
                b: 191,
            },
        },
    }
}

pub fn interpolate(gradient: Gradient, t: f64) -> Rgb {
    let clamped = t.clamp(0.0, 1.0);
    let lerp = |low: u8, high: u8| -> u8 {
        let low = low as f64;
        let high = high as f64;
        (low + (high - low) * clamped).round() as u8
    };

    Rgb {
        r: lerp(gradient.low.r, gradient.high.r),
        g: lerp(gradient.low.g, gradient.high.g),
        b: lerp(gradient.low.b, gradient.high.b),
    }
}

pub fn paint(text: &str, color: Rgb, enabled: bool) -> String {
    if !enabled {
        return text.to_owned();
    }

    format!(
        "\x1b[38;2;{};{};{}m{}\x1b[0m",
        color.r, color.g, color.b, text
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolation_hits_endpoints() {
        let gradient = gradient_for(MetricKind::Cpu);
        assert_eq!(interpolate(gradient, 0.0), gradient.low);
        assert_eq!(interpolate(gradient, 1.0), gradient.high);
    }

    #[test]
    fn paint_omits_escape_codes_when_disabled() {
        assert_eq!(paint("x", Rgb { r: 1, g: 2, b: 3 }, false), "x");
    }
}
