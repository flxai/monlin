use crate::layout::MetricKind;
use chromata::{popular, Theme};
use palette::{Clamp, FromColor, Lab, LabHue, Lch, Mix, Srgb};
use splines::{Interpolation, Key, Spline};
use std::sync::OnceLock;

pub type BaseColors = [ColorSpec; 8];
pub type BaseHues = BaseColors;

const DEFAULT_LOW_LIGHTNESS: f32 = 24.0;
const DEFAULT_LOW_CHROMA: f32 = 38.0;
const DEFAULT_HIGH_LIGHTNESS: f32 = 86.0;
const DEFAULT_HIGH_CHROMA: f32 = 78.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColorSpec {
    Angle(f32),
    Lch {
        lightness: f32,
        chroma: f32,
        hue: f32,
    },
    Rgb(Rgb),
}

pub fn palette_names() -> &'static [&'static str] {
    &[
        "default",
        "canonical",
        "rainbow",
        "warm",
        "cool",
        "pastel",
        "neon",
        "solarized",
        "solarized-light",
        "gruvbox",
        "gruvbox-light",
        "nord",
        "catppuccin",
        "catppuccin-latte",
        "catppuccin-frappe",
        "catppuccin-macchiato",
        "catppuccin-mocha",
        "tokyonight",
        "tokyonight-storm",
        "tokyonight-light",
        "dracula",
    ]
}

pub fn named_palette(name: &str) -> Option<Vec<ColorSpec>> {
    let palette = match name.to_ascii_lowercase().as_str() {
        "default" | "canonical" => default_base_hues().to_vec(),
        "rainbow" => vec![
            ColorSpec::Angle(0.0),
            ColorSpec::Angle(60.0),
            ColorSpec::Angle(120.0),
            ColorSpec::Angle(180.0),
            ColorSpec::Angle(240.0),
            ColorSpec::Angle(300.0),
        ],
        "warm" => vec![
            ColorSpec::Angle(15.0),
            ColorSpec::Angle(40.0),
            ColorSpec::Angle(65.0),
            ColorSpec::Angle(90.0),
            ColorSpec::Angle(115.0),
            ColorSpec::Angle(140.0),
        ],
        "cool" => vec![
            ColorSpec::Angle(170.0),
            ColorSpec::Angle(195.0),
            ColorSpec::Angle(220.0),
            ColorSpec::Angle(245.0),
            ColorSpec::Angle(280.0),
            ColorSpec::Angle(315.0),
        ],
        "pastel" => vec![
            ColorSpec::Lch {
                lightness: 92.0,
                chroma: 42.0,
                hue: 20.0,
            },
            ColorSpec::Lch {
                lightness: 92.0,
                chroma: 42.0,
                hue: 80.0,
            },
            ColorSpec::Lch {
                lightness: 92.0,
                chroma: 42.0,
                hue: 140.0,
            },
            ColorSpec::Lch {
                lightness: 92.0,
                chroma: 42.0,
                hue: 200.0,
            },
            ColorSpec::Lch {
                lightness: 92.0,
                chroma: 42.0,
                hue: 260.0,
            },
            ColorSpec::Lch {
                lightness: 92.0,
                chroma: 42.0,
                hue: 320.0,
            },
        ],
        "neon" => vec![
            ColorSpec::Lch {
                lightness: 88.0,
                chroma: 95.0,
                hue: 20.0,
            },
            ColorSpec::Lch {
                lightness: 88.0,
                chroma: 95.0,
                hue: 80.0,
            },
            ColorSpec::Lch {
                lightness: 88.0,
                chroma: 95.0,
                hue: 140.0,
            },
            ColorSpec::Lch {
                lightness: 88.0,
                chroma: 95.0,
                hue: 200.0,
            },
            ColorSpec::Lch {
                lightness: 88.0,
                chroma: 95.0,
                hue: 260.0,
            },
            ColorSpec::Lch {
                lightness: 88.0,
                chroma: 95.0,
                hue: 320.0,
            },
        ],
        "solarized" => theme_palette(&popular::solarized::DARK),
        "solarized-light" => theme_palette(&popular::solarized::LIGHT),
        "gruvbox" => theme_palette(&popular::gruvbox::DARK),
        "gruvbox-light" => theme_palette(&popular::gruvbox::LIGHT),
        "nord" => theme_palette(&popular::nord::THEME),
        "catppuccin" | "catppuccin-mocha" => theme_palette(&popular::catppuccin::MOCHA),
        "catppuccin-latte" => theme_palette(&popular::catppuccin::LATTE),
        "catppuccin-frappe" => theme_palette(&popular::catppuccin::FRAPPE),
        "catppuccin-macchiato" => theme_palette(&popular::catppuccin::MACCHIATO),
        "tokyonight" => theme_palette(&popular::tokyo_night::DARK),
        "tokyonight-storm" => theme_palette(&popular::tokyo_night::STORM),
        "tokyonight-light" => theme_palette(&popular::tokyo_night::LIGHT),
        "dracula" => theme_palette(&popular::dracula::THEME),
        _ => return None,
    };

    Some(palette)
}

fn theme_palette(theme: &Theme) -> Vec<ColorSpec> {
    [
        theme.red,
        theme.orange,
        theme.yellow,
        theme.green,
        theme.cyan,
        theme.blue,
        theme.purple,
        theme.magenta,
    ]
    .into_iter()
    .flatten()
    .map(color_spec_from_chromata)
    .collect()
}

fn color_spec_from_chromata(color: chromata::Color) -> ColorSpec {
    ColorSpec::Rgb(Rgb {
        r: color.r,
        g: color.g,
        b: color.b,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Gradient {
    pub low: Rgb,
    pub high: Rgb,
}

pub fn gradient_for(metric: MetricKind) -> Gradient {
    gradient_for_with_hues(metric, None)
}

pub fn gradient_for_with_hues(metric: MetricKind, hues: Option<&BaseHues>) -> Gradient {
    match metric {
        MetricKind::Cpu => wheel_gradient(0, hues),
        MetricKind::Rnd => wheel_gradient(0, hues),
        MetricKind::Sys => blend_gradients(
            gradient_for_with_hues(MetricKind::Cpu, hues),
            gradient_for_with_hues(MetricKind::Memory, hues),
        ),
        MetricKind::Gpu => wheel_gradient(2, hues),
        MetricKind::Vram => gradient_from_spec(vram_color(hues)),
        MetricKind::Gfx => blend_gradients(
            gradient_for_with_hues(MetricKind::Gpu, hues),
            gradient_for_with_hues(MetricKind::Vram, hues),
        ),
        MetricKind::Memory => wheel_gradient(1, hues),
        MetricKind::Storage => wheel_gradient(3, hues),
        MetricKind::Io => blend_gradients(
            gradient_for_with_hues(MetricKind::In, hues),
            gradient_for_with_hues(MetricKind::Out, hues),
        ),
        MetricKind::In => wheel_gradient(4, hues),
        MetricKind::Out => wheel_gradient(5, hues),
        MetricKind::Net => blend_gradients(
            gradient_for_with_hues(MetricKind::Ingress, hues),
            gradient_for_with_hues(MetricKind::Egress, hues),
        ),
        MetricKind::Ingress => wheel_gradient(6, hues),
        MetricKind::Egress => wheel_gradient(7, hues),
    }
}

pub fn split_gradients_for(metric: MetricKind) -> Option<(Gradient, Gradient)> {
    split_gradients_for_with_hues(metric, None)
}

pub fn visible_hues(count: usize, explicit: Option<&[ColorSpec]>) -> Vec<ColorSpec> {
    if let Some(values) = explicit {
        if !values.is_empty() {
            return values.to_vec();
        }
    }

    let canonical = default_base_hues();
    if count <= canonical.len() {
        return canonical[..count].to_vec();
    }

    (0..count)
        .map(|index| ColorSpec::Angle(20.0 + (360.0 * index as f32 / count as f32)))
        .collect()
}

pub fn metric_hues_for_visible_hue(metric: MetricKind, hue: ColorSpec) -> BaseHues {
    let mut hues = default_base_hues();
    match metric {
        MetricKind::Cpu => hues[0] = hue,
        MetricKind::Rnd => hues[0] = hue,
        MetricKind::Sys => {
            hues[0] = hue;
            hues[1] = hue;
        }
        MetricKind::Memory => hues[1] = hue,
        MetricKind::Gpu => hues[2] = hue,
        MetricKind::Vram | MetricKind::Gfx => {
            hues[2] = hue;
            hues[3] = hue;
        }
        MetricKind::Storage => hues[3] = hue,
        MetricKind::Io => {
            hues[4] = hue;
            hues[5] = hue;
        }
        MetricKind::In => hues[4] = hue,
        MetricKind::Out => hues[5] = hue,
        MetricKind::Net => {
            hues[6] = hue;
            hues[7] = hue;
        }
        MetricKind::Ingress => hues[6] = hue,
        MetricKind::Egress => hues[7] = hue,
    }
    hues
}

pub fn automatic_hues_for_metrics(metrics: &[MetricKind]) -> BaseHues {
    let mut hues = default_base_hues();
    let canonical = default_base_hues();
    let mut needed = [false; 8];

    for metric in metrics {
        for index in base_slots_for_metric(*metric) {
            needed[*index] = true;
        }
    }

    let indices = needed
        .iter()
        .enumerate()
        .filter_map(|(index, enabled)| enabled.then_some(index))
        .collect::<Vec<_>>();

    if indices.is_empty() {
        return hues;
    }

    for (position, index) in indices.into_iter().enumerate() {
        hues[index] = canonical[position];
    }

    hues
}

pub fn automatic_hues_for_stream(count: usize) -> BaseHues {
    let mut hues = default_base_hues();
    let canonical = default_base_hues();
    let active = count.clamp(1, 8);
    for index in 0..active {
        hues[index] = canonical[index];
    }
    hues
}

pub fn split_gradients_for_with_hues(
    metric: MetricKind,
    hues: Option<&BaseHues>,
) -> Option<(Gradient, Gradient)> {
    match metric {
        MetricKind::Sys => Some((
            gradient_for_with_hues(MetricKind::Cpu, hues),
            gradient_for_with_hues(MetricKind::Memory, hues),
        )),
        MetricKind::Gfx => Some((
            gradient_for_with_hues(MetricKind::Gpu, hues),
            gradient_for_with_hues(MetricKind::Vram, hues),
        )),
        MetricKind::Net => Some((
            gradient_for_with_hues(MetricKind::Ingress, hues),
            gradient_for_with_hues(MetricKind::Egress, hues),
        )),
        MetricKind::Io => Some((
            gradient_for_with_hues(MetricKind::In, hues),
            gradient_for_with_hues(MetricKind::Out, hues),
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
    let eased = emphasize(t.clamp(0.0, 1.0)) as f32;
    let low = rgb_to_lab(gradient.low);
    let high = rgb_to_lab(gradient.high);
    lab_to_rgb(low.mix(high, eased))
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
    easing_spline().clamped_sample(t).unwrap_or(t)
}

fn easing_spline() -> &'static Spline<f64, f64> {
    static SPLINE: OnceLock<Spline<f64, f64>> = OnceLock::new();
    SPLINE.get_or_init(|| {
        Spline::from_vec(vec![
            Key::new(-0.35, 0.0, Interpolation::CatmullRom),
            Key::new(0.0, 0.0, Interpolation::CatmullRom),
            Key::new(0.35, 0.10, Interpolation::CatmullRom),
            Key::new(0.70, 0.58, Interpolation::CatmullRom),
            Key::new(1.0, 1.0, Interpolation::CatmullRom),
            Key::new(1.35, 1.0, Interpolation::CatmullRom),
        ])
    })
}

fn wheel_gradient(index: usize, hues: Option<&BaseHues>) -> Gradient {
    gradient_from_spec(base_color(index, hues))
}

fn base_color(index: usize, hues: Option<&BaseHues>) -> ColorSpec {
    hues.map(|values| values[index])
        .unwrap_or_else(|| default_base_hues()[index])
}

fn default_base_hues() -> BaseHues {
    [
        ColorSpec::Angle(20.0),
        ColorSpec::Angle(65.0),
        ColorSpec::Angle(110.0),
        ColorSpec::Angle(155.0),
        ColorSpec::Angle(200.0),
        ColorSpec::Angle(245.0),
        ColorSpec::Angle(290.0),
        ColorSpec::Angle(335.0),
    ]
}

fn base_slots_for_metric(metric: MetricKind) -> &'static [usize] {
    match metric {
        MetricKind::Cpu => &[0],
        MetricKind::Rnd => &[0],
        MetricKind::Sys => &[0, 1],
        MetricKind::Memory => &[1],
        MetricKind::Gpu => &[2],
        MetricKind::Vram => &[2, 3],
        MetricKind::Gfx => &[2, 3],
        MetricKind::Storage => &[3],
        MetricKind::Io => &[4, 5],
        MetricKind::In => &[4],
        MetricKind::Out => &[5],
        MetricKind::Net => &[6, 7],
        MetricKind::Ingress => &[6],
        MetricKind::Egress => &[7],
    }
}

fn vram_color(hues: Option<&BaseHues>) -> ColorSpec {
    let start = base_color(2, hues);
    let end = base_color(3, hues);
    match (start, end) {
        (ColorSpec::Angle(a), ColorSpec::Angle(b)) => ColorSpec::Angle(circular_midpoint(a, b)),
        _ => ColorSpec::Rgb(blend_rgb(spec_high_rgb(start), spec_high_rgb(end))),
    }
}

fn circular_midpoint(a: f32, b: f32) -> f32 {
    let mut delta = (b - a).rem_euclid(360.0);
    if delta > 180.0 {
        delta -= 360.0;
    }
    (a + delta / 2.0).rem_euclid(360.0)
}

fn gradient_from_spec(spec: ColorSpec) -> Gradient {
    match spec {
        ColorSpec::Angle(hue_degrees) => gradient_from_hue(hue_degrees),
        ColorSpec::Lch {
            lightness,
            chroma,
            hue,
        } => gradient_from_lch(lightness, chroma, hue),
        ColorSpec::Rgb(rgb) => gradient_from_rgb(rgb),
    }
}

fn gradient_from_hue(hue_degrees: f32) -> Gradient {
    Gradient {
        low: lch_to_rgb(DEFAULT_LOW_LIGHTNESS, DEFAULT_LOW_CHROMA, hue_degrees),
        high: lch_to_rgb(DEFAULT_HIGH_LIGHTNESS, DEFAULT_HIGH_CHROMA, hue_degrees),
    }
}

fn gradient_from_lch(lightness: f32, chroma: f32, hue: f32) -> Gradient {
    let high = lch_to_rgb(lightness, chroma, hue);
    let low = lch_to_rgb(
        (lightness * (DEFAULT_LOW_LIGHTNESS / DEFAULT_HIGH_LIGHTNESS)).clamp(0.0, 100.0),
        chroma * (DEFAULT_LOW_CHROMA / DEFAULT_HIGH_CHROMA),
        hue,
    );
    Gradient { low, high }
}

fn gradient_from_rgb(rgb: Rgb) -> Gradient {
    let high_lch = rgb_to_lch(rgb);
    let low = lch_to_rgb(
        (high_lch.l * (DEFAULT_LOW_LIGHTNESS / DEFAULT_HIGH_LIGHTNESS)).clamp(0.0, 100.0),
        high_lch.chroma * (DEFAULT_LOW_CHROMA / DEFAULT_HIGH_CHROMA),
        high_lch.hue.into_degrees(),
    );
    Gradient { low, high: rgb }
}

fn spec_high_rgb(spec: ColorSpec) -> Rgb {
    match spec {
        ColorSpec::Angle(hue) => gradient_from_hue(hue).high,
        ColorSpec::Lch {
            lightness,
            chroma,
            hue,
        } => lch_to_rgb(lightness, chroma, hue),
        ColorSpec::Rgb(rgb) => rgb,
    }
}

fn lch_to_rgb(lightness: f32, chroma: f32, hue_degrees: f32) -> Rgb {
    let lch = Lch::new(lightness, chroma, LabHue::from_degrees(hue_degrees));
    let lab = Lab::from_color(lch);
    lab_to_rgb(lab)
}

fn rgb_to_lab(rgb: Rgb) -> Lab {
    let rgb = Srgb::new(rgb.r, rgb.g, rgb.b).into_format::<f32>();
    let linear = rgb.into_linear();
    Lab::from_color(linear)
}

fn rgb_to_lch(rgb: Rgb) -> Lch {
    Lch::from_color(rgb_to_lab(rgb))
}

fn lab_to_rgb(lab: Lab) -> Rgb {
    let rgb_linear = palette::LinSrgb::from_color(lab);
    let rgb: Srgb<f32> = Srgb::from_linear(rgb_linear).clamp();
    let rgb = rgb.into_format::<u8>();
    Rgb {
        r: rgb.red,
        g: rgb.green,
        b: rgb.blue,
    }
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
        let low_gain = i32::from(brightness(interpolate(gradient, 0.50)))
            - i32::from(brightness(interpolate(gradient, 0.25)));
        let high_gain = i32::from(brightness(interpolate(gradient, 1.00)))
            - i32::from(brightness(interpolate(gradient, 0.75)));
        assert!(high_gain > low_gain);
    }

    #[test]
    fn gpu_and_vram_have_different_gradients() {
        assert_ne!(
            gradient_for(MetricKind::Gpu),
            gradient_for(MetricKind::Vram)
        );
    }

    #[test]
    fn combined_metric_gradients_are_midpoints() {
        assert_ne!(gradient_for(MetricKind::Sys), gradient_for(MetricKind::Cpu));
        assert_ne!(gradient_for(MetricKind::Gfx), gradient_for(MetricKind::Gpu));
        assert_ne!(
            gradient_for(MetricKind::Net),
            gradient_for(MetricKind::Ingress)
        );
    }

    #[test]
    fn visible_hues_prefers_explicit_values() {
        assert_eq!(
            visible_hues(
                3,
                Some(&[
                    ColorSpec::Angle(5.0),
                    ColorSpec::Angle(15.0),
                    ColorSpec::Angle(25.0)
                ])
            ),
            vec![
                ColorSpec::Angle(5.0),
                ColorSpec::Angle(15.0),
                ColorSpec::Angle(25.0)
            ]
        );
    }

    #[test]
    fn visible_hues_spreads_evenly_past_canonical_set() {
        assert_eq!(
            visible_hues(8, None),
            vec![
                ColorSpec::Angle(20.0),
                ColorSpec::Angle(65.0),
                ColorSpec::Angle(110.0),
                ColorSpec::Angle(155.0),
                ColorSpec::Angle(200.0),
                ColorSpec::Angle(245.0),
                ColorSpec::Angle(290.0),
                ColorSpec::Angle(335.0)
            ]
        );
    }

    #[test]
    fn visible_metric_hues_follow_metric_shape() {
        let sys = metric_hues_for_visible_hue(MetricKind::Sys, ColorSpec::Angle(42.0));
        assert_eq!(sys[0], ColorSpec::Angle(42.0));
        assert_eq!(sys[1], ColorSpec::Angle(42.0));

        let io = metric_hues_for_visible_hue(MetricKind::Io, ColorSpec::Angle(210.0));
        assert_eq!(io[4], ColorSpec::Angle(210.0));
        assert_eq!(io[5], ColorSpec::Angle(210.0));

        let net = metric_hues_for_visible_hue(MetricKind::Net, ColorSpec::Angle(275.0));
        assert_eq!(net[6], ColorSpec::Angle(275.0));
        assert_eq!(net[7], ColorSpec::Angle(275.0));
    }

    #[test]
    fn automatic_hues_for_metrics_assigns_only_needed_slots() {
        let hues = automatic_hues_for_metrics(&[MetricKind::Cpu, MetricKind::Net]);
        assert_eq!(hues[0], ColorSpec::Angle(20.0));
        assert_eq!(hues[6], ColorSpec::Angle(65.0));
        assert_eq!(hues[7], ColorSpec::Angle(110.0));
    }

    #[test]
    fn automatic_hues_for_stream_clamps_to_eight_channels() {
        assert_eq!(automatic_hues_for_stream(8), default_base_hues());
    }

    #[test]
    fn split_gradients_exist_only_for_combined_metrics() {
        assert!(split_gradients_for(MetricKind::Cpu).is_none());
        assert!(split_gradients_for(MetricKind::Sys).is_some());
        assert!(split_gradients_for(MetricKind::Gfx).is_some());
        assert!(split_gradients_for(MetricKind::Io).is_some());
        assert!(split_gradients_for(MetricKind::Net).is_some());
    }

    #[test]
    fn circular_midpoint_wraps_across_zero() {
        assert!((circular_midpoint(350.0, 10.0) - 0.0).abs() < f32::EPSILON);
        assert!((circular_midpoint(10.0, 350.0) - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn vram_gradient_uses_midpoint_between_gpu_and_storage_hues() {
        let hues = [
            ColorSpec::Angle(20.0),
            ColorSpec::Angle(65.0),
            ColorSpec::Angle(140.0),
            ColorSpec::Angle(220.0),
            ColorSpec::Angle(260.0),
            ColorSpec::Angle(320.0),
            ColorSpec::Angle(20.0),
            ColorSpec::Angle(20.0),
        ];
        let midpoint_gradient = gradient_from_hue(180.0);
        assert_eq!(
            gradient_for_with_hues(MetricKind::Vram, Some(&hues)),
            midpoint_gradient
        );
    }

    #[test]
    fn explicit_rgb_spec_uses_given_high_color() {
        let gradient = gradient_for_with_hues(
            MetricKind::Cpu,
            Some(&metric_hues_for_visible_hue(
                MetricKind::Cpu,
                ColorSpec::Rgb(Rgb {
                    r: 0xff,
                    g: 0x88,
                    b: 0x00,
                }),
            )),
        );
        assert_eq!(
            gradient.high,
            Rgb {
                r: 0xff,
                g: 0x88,
                b: 0x00,
            }
        );
    }

    #[test]
    fn explicit_lch_spec_roundtrips_into_distinct_gradient() {
        let gradient = gradient_for_with_hues(
            MetricKind::Cpu,
            Some(&metric_hues_for_visible_hue(
                MetricKind::Cpu,
                ColorSpec::Lch {
                    lightness: 86.0,
                    chroma: 78.0,
                    hue: 20.0,
                },
            )),
        );
        assert_eq!(gradient.high, gradient_from_hue(20.0).high);
    }

    #[test]
    fn named_default_palette_matches_canonical_hues() {
        assert_eq!(
            named_palette("default").unwrap(),
            default_base_hues().to_vec()
        );
        assert_eq!(
            named_palette("canonical").unwrap(),
            default_base_hues().to_vec()
        );
    }

    #[test]
    fn named_palettes_are_case_insensitive() {
        assert_eq!(named_palette("WARM"), named_palette("warm"));
    }

    #[test]
    fn named_palette_reports_known_names() {
        assert!(palette_names().contains(&"default"));
        assert!(palette_names().contains(&"pastel"));
        assert!(palette_names().contains(&"neon"));
        assert!(palette_names().contains(&"solarized"));
        assert!(palette_names().contains(&"gruvbox"));
        assert!(palette_names().contains(&"nord"));
        assert!(palette_names().contains(&"catppuccin"));
        assert!(palette_names().contains(&"tokyonight"));
        assert!(palette_names().contains(&"dracula"));
    }

    #[test]
    fn chromata_theme_palettes_expand_to_eight_rgb_colors() {
        for name in [
            "solarized",
            "gruvbox",
            "nord",
            "catppuccin",
            "tokyonight",
            "dracula",
        ] {
            let palette = named_palette(name).unwrap();
            assert_eq!(palette.len(), 8, "{name}");
            assert!(
                palette
                    .iter()
                    .all(|color| matches!(color, ColorSpec::Rgb(_))),
                "{name}"
            );
        }
    }
}
