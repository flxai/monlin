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
                r: 40,
                g: 78,
                b: 150,
            },
            high: Rgb {
                r: 169,
                g: 219,
                b: 255,
            },
        },
        MetricKind::Sys => blend_gradients(gradient_for(MetricKind::Cpu), gradient_for(MetricKind::Memory)),
        MetricKind::Gpu => Gradient {
            low: Rgb {
                r: 17,
                g: 74,
                b: 33,
            },
            high: Rgb {
                r: 173,
                g: 255,
                b: 191,
            },
        },
        MetricKind::Vram => Gradient {
            low: Rgb {
                r: 26,
                g: 92,
                b: 61,
            },
            high: Rgb {
                r: 181,
                g: 255,
                b: 214,
            },
        },
        MetricKind::Gfx => blend_gradients(gradient_for(MetricKind::Gpu), gradient_for(MetricKind::Vram)),
        MetricKind::Memory => Gradient {
            low: Rgb {
                r: 112,
                g: 28,
                b: 28,
            },
            high: Rgb {
                r: 255,
                g: 176,
                b: 176,
            },
        },
        MetricKind::Storage => Gradient {
            low: Rgb {
                r: 96,
                g: 69,
                b: 22,
            },
            high: Rgb {
                r: 255,
                g: 208,
                b: 140,
            },
        },
        MetricKind::Io => Gradient {
            low: Rgb {
                r: 136,
                g: 104,
                b: 24,
            },
            high: Rgb {
                r: 255,
                g: 236,
                b: 153,
            },
        },
        MetricKind::Net => blend_gradients(
            gradient_for(MetricKind::Ingress),
            gradient_for(MetricKind::Egress),
        ),
        MetricKind::Ingress => Gradient {
            low: Rgb {
                r: 8,
                g: 68,
                b: 84,
            },
            high: Rgb {
                r: 156,
                g: 244,
                b: 255,
            },
        },
        MetricKind::Egress => Gradient {
            low: Rgb {
                r: 105,
                g: 44,
                b: 130,
            },
            high: Rgb {
                r: 239,
                g: 173,
                b: 255,
            },
        },
    }
}

pub fn split_gradients_for(metric: MetricKind) -> Option<(Gradient, Gradient)> {
    match metric {
        MetricKind::Sys => Some((gradient_for(MetricKind::Cpu), gradient_for(MetricKind::Memory))),
        MetricKind::Gfx => Some((gradient_for(MetricKind::Gpu), gradient_for(MetricKind::Vram))),
        MetricKind::Net => Some((gradient_for(MetricKind::Ingress), gradient_for(MetricKind::Egress))),
        MetricKind::Io => Some((
            Gradient {
                low: Rgb {
                    r: 156,
                    g: 116,
                    b: 24,
                },
                high: Rgb {
                    r: 255,
                    g: 224,
                    b: 120,
                },
            },
            Gradient {
                low: Rgb {
                    r: 124,
                    g: 142,
                    b: 40,
                },
                high: Rgb {
                    r: 235,
                    g: 255,
                    b: 143,
                },
            },
        )),
        _ => None,
    }
}

fn blend_gradients(left: Gradient, right: Gradient) -> Gradient {
    Gradient {
        low: blend_rgb(left.low, right.low),
        high: blend_rgb(left.high, right.high),
    }
}

fn blend_rgb(left: Rgb, right: Rgb) -> Rgb {
    let mean = |a: u8, b: u8| -> u8 { ((u16::from(a) + u16::from(b)) / 2) as u8 };
    Rgb {
        r: mean(left.r, right.r),
        g: mean(left.g, right.g),
        b: mean(left.b, right.b),
    }
}

pub fn interpolate(gradient: Gradient, t: f64) -> Rgb {
    let clamped = emphasize(t.clamp(0.0, 1.0));
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

fn emphasize(t: f64) -> f64 {
    t.powf(1.8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brightness(rgb: Rgb) -> u16 {
        u16::from(rgb.r) + u16::from(rgb.g) + u16::from(rgb.b)
    }

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

    #[test]
    fn colors_get_brighter_towards_saturation() {
        let gradient = gradient_for(MetricKind::Memory);
        assert!(brightness(interpolate(gradient, 0.0)) < brightness(interpolate(gradient, 1.0)));
    }

    #[test]
    fn emphasis_favors_high_usage_ranges() {
        let gradient = gradient_for(MetricKind::Cpu);
        let low_gain =
            i32::from(brightness(interpolate(gradient, 0.50))) - i32::from(brightness(interpolate(gradient, 0.25)));
        let high_gain =
            i32::from(brightness(interpolate(gradient, 1.00))) - i32::from(brightness(interpolate(gradient, 0.75)));
        assert!(high_gain > low_gain);
    }

    #[test]
    fn gpu_and_vram_have_different_gradients() {
        assert_ne!(gradient_for(MetricKind::Gpu), gradient_for(MetricKind::Vram));
    }

    #[test]
    fn combined_metric_gradients_are_midpoints() {
        assert_ne!(gradient_for(MetricKind::Sys), gradient_for(MetricKind::Cpu));
        assert_ne!(gradient_for(MetricKind::Gfx), gradient_for(MetricKind::Gpu));
        assert_ne!(gradient_for(MetricKind::Net), gradient_for(MetricKind::Ingress));
    }
}
