//! What the site panel shows about the focused page: its title and
//! description, theme color, fonts and a palette of the colors it paints.
//!
//! SOURCING: `kmeans_colors` clusters the sampled pixels in `palette`'s Lab
//! space, and `serde_json` reads the script's answer. The script
//! (`scripts/site-panel.js`) and this module's shape are ours: no crate models
//! a site's identity. Read on demand, when the panel opens, and never queued
//! for the search index or the model.

use kmeans_colors::{Calculate, Kmeans, Sort, get_kmeans};
use palette::{FromColor, Lab, Srgb};
use serde_json::Value;
use url::Url;

/// A color as red, green and blue bytes.
pub type Rgb = [u8; 3];

/// Pixels the palette looks at, at most: points spread evenly over the frame.
const MAX_SAMPLES: usize = 8192;
/// Colors in a palette, at most.
const COLORS: usize = 6;
/// Clusterings tried, each from its own seed; the tightest is kept.
const RUNS: u64 = 3;
const MAX_ITERATIONS: usize = 20;
/// The movement of the centers, in Lab, under which a clustering is done.
const CONVERGED: f32 = 5.0;

#[derive(Clone, Debug, PartialEq)]
pub struct SiteInfo {
    /// The document the rest was read from.
    pub url: Option<Url>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub theme_color: Option<Rgb>,
    pub fonts: Vec<SiteFont>,
    /// The frame's colors, the largest share first.
    pub palette: Vec<Swatch>,
}

/// A family in use on the page and what it is used for.
#[derive(Clone, Debug, PartialEq)]
pub struct SiteFont {
    pub family: String,
    pub roles: Vec<FontRole>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontRole {
    Headings,
    Body,
    Code,
}

impl FontRole {
    pub fn label(self) -> &'static str {
        match self {
            Self::Headings => "Headings",
            Self::Body => "Body",
            Self::Code => "Code",
        }
    }

    fn named(name: &str) -> Option<Self> {
        match name {
            "headings" => Some(Self::Headings),
            "body" => Some(Self::Body),
            "code" => Some(Self::Code),
            _ => None,
        }
    }
}

/// A color of the frame and the part of the frame that is near it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Swatch {
    pub color: Rgb,
    /// From 0 to 1.
    pub share: f32,
}

pub fn hex([r, g, b]: Rgb) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

impl SiteInfo {
    /// The script's answer, with the palette read from the frame. A field the
    /// answer lacks or gets wrong is left out; only an answer that is not an
    /// object is refused.
    pub fn from_answer(answer: &str, palette: Vec<Swatch>) -> Option<Self> {
        let answer: Value = serde_json::from_str(answer).ok()?;
        let answer = answer.as_object()?;
        let text = |key: &str| {
            answer
                .get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_owned)
        };
        Some(Self {
            url: text("url").and_then(|url| Url::parse(&url).ok()),
            title: text("title"),
            description: text("description"),
            theme_color: answer.get("themeColor").and_then(color),
            fonts: answer
                .get("fonts")
                .and_then(Value::as_array)
                .map(|fonts| fonts.iter().filter_map(font).collect())
                .unwrap_or_default(),
            palette,
        })
    }
}

fn color(value: &Value) -> Option<Rgb> {
    let [r, g, b] = value.as_array()?.as_slice() else {
        return None;
    };
    let byte = |value: &Value| u8::try_from(value.as_u64()?).ok();
    Some([byte(r)?, byte(g)?, byte(b)?])
}

fn font(value: &Value) -> Option<SiteFont> {
    let family = value.get("family")?.as_str()?.trim();
    let roles: Vec<FontRole> = value
        .get("roles")?
        .as_array()?
        .iter()
        .filter_map(|role| FontRole::named(role.as_str()?))
        .collect();
    (!family.is_empty() && !roles.is_empty()).then(|| SiteFont {
        family: family.to_owned(),
        roles,
    })
}

/// The colors of a frame of BGRA pixels, `stride` bytes to a row, the largest
/// share first. The frame is sampled down to a bounded number of pixels, so
/// the cost does not grow with the window; clear pixels are left out.
pub fn palette(bgra: &[u8], width: usize, height: usize, stride: usize) -> Vec<Swatch> {
    let lab: Vec<Lab> = sample(bgra, width, height, stride)
        .into_iter()
        .map(|[r, g, b]| Lab::from_color(Srgb::new(r, g, b).into_format::<f32>()))
        .collect();
    if lab.is_empty() {
        return Vec::new();
    }
    // The sum of each point's squared distance to its center.
    let spread = |run: &Kmeans<Lab>| -> f32 {
        lab.iter()
            .zip(&run.indices)
            .map(|(point, &index)| Lab::difference(point, &run.centroids[usize::from(index)]))
            .sum()
    };
    let Some(best) = (0..RUNS)
        .map(|seed| get_kmeans(COLORS, MAX_ITERATIONS, CONVERGED, false, &lab, seed))
        .min_by(|a, b| spread(a).total_cmp(&spread(b)))
    else {
        return Vec::new();
    };
    let mut swatches: Vec<Swatch> = Lab::sort_indexed_colors(&best.centroids, &best.indices)
        .into_iter()
        .map(|center| {
            let rgb = Srgb::<f32>::from_color(center.centroid).into_format::<u8>();
            Swatch {
                color: [rgb.red, rgb.green, rgb.blue],
                share: center.percentage,
            }
        })
        .collect();
    swatches.sort_by(|a, b| b.share.total_cmp(&a.share));
    swatches
}

/// Points spread evenly over the frame, as red, green and blue.
fn sample(bgra: &[u8], width: usize, height: usize, stride: usize) -> Vec<Rgb> {
    if width == 0
        || height == 0
        || stride < width * 4
        || bgra.len() < stride * (height - 1) + width * 4
    {
        return Vec::new();
    }
    let step = ((width * height) as f64 / MAX_SAMPLES as f64)
        .sqrt()
        .ceil()
        .max(1.) as usize;
    let mut colors = Vec::new();
    for y in (step / 2..height).step_by(step) {
        let row = &bgra[y * stride..y * stride + width * 4];
        for x in (step / 2..width).step_by(step) {
            let [b, g, r, a] = [row[x * 4], row[x * 4 + 1], row[x * 4 + 2], row[x * 4 + 3]];
            if a != 0 {
                colors.push([r, g, b]);
            }
        }
    }
    colors
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(answer: &str) -> Option<SiteInfo> {
        SiteInfo::from_answer(answer, Vec::new())
    }

    #[test]
    fn a_whole_answer_is_read_field_by_field() {
        let swatch = Swatch {
            color: [1, 2, 3],
            share: 0.5,
        };
        let info = SiteInfo::from_answer(
            r##"{"url": "https://example.com/a?b=1", "title": "  Example  ",
                "description": "A site.", "themeColor": [31, 78, 121],
                "fonts": [
                    {"family": "Georgia", "roles": ["headings", "body"]},
                    {"family": "Menlo", "roles": ["code"]}]}"##,
            vec![swatch],
        )
        .unwrap();
        assert_eq!(info.url.unwrap().as_str(), "https://example.com/a?b=1");
        assert_eq!(info.title.as_deref(), Some("Example"));
        assert_eq!(info.description.as_deref(), Some("A site."));
        assert_eq!(info.theme_color, Some([31, 78, 121]));
        assert_eq!(
            info.fonts,
            [
                SiteFont {
                    family: "Georgia".into(),
                    roles: vec![FontRole::Headings, FontRole::Body]
                },
                SiteFont {
                    family: "Menlo".into(),
                    roles: vec![FontRole::Code]
                },
            ]
        );
        assert_eq!(info.palette, [swatch]);
    }

    #[test]
    fn missing_fields_are_left_out() {
        let empty = info("{}").unwrap();
        assert_eq!(
            empty,
            SiteInfo {
                url: None,
                title: None,
                description: None,
                theme_color: None,
                fonts: Vec::new(),
                palette: Vec::new(),
            }
        );
        // The script answers with empty strings and null for what a page lacks.
        let bare = info(
            r#"{"url": "", "title": " ", "description": "", "themeColor": null, "fonts": []}"#,
        )
        .unwrap();
        assert_eq!(bare.title, None);
        assert_eq!(bare.description, None);
        assert_eq!(bare.theme_color, None);
        assert!(bare.fonts.is_empty());
    }

    #[test]
    fn malformed_fields_are_left_out_one_at_a_time() {
        let info = info(
            r#"{"url": "not a url", "title": 7, "description": ["x"],
                "themeColor": [1, 2],
                "fonts": [
                    "Georgia",
                    {"family": "", "roles": ["body"]},
                    {"family": "No roles", "roles": []},
                    {"family": "Unknown role", "roles": ["footer"]},
                    {"family": "Not a list", "roles": "body"},
                    {"roles": ["body"]},
                    {"family": "Kept", "roles": ["footer", "code"]}]}"#,
        )
        .unwrap();
        assert_eq!(info.url, None);
        assert_eq!(info.title, None);
        assert_eq!(info.description, None);
        assert_eq!(info.theme_color, None);
        assert_eq!(
            info.fonts,
            [SiteFont {
                family: "Kept".into(),
                roles: vec![FontRole::Code]
            }]
        );
    }

    #[test]
    fn a_theme_color_is_three_bytes() {
        let theme = |value: &str| {
            info(&format!(r#"{{"themeColor": {value}}}"#))
                .unwrap()
                .theme_color
        };
        assert_eq!(theme("[0, 128, 255]"), Some([0, 128, 255]));
        assert_eq!(theme("[0, 128, 256]"), None);
        assert_eq!(theme("[0, 128, -1]"), None);
        assert_eq!(theme("[0, 128, 1.5]"), None);
        assert_eq!(theme("[0, 128, 255, 255]"), None);
        assert_eq!(theme(r##""#1f4e79""##), None);
        assert_eq!(theme("null"), None);
    }

    #[test]
    fn an_answer_that_is_not_an_object_is_refused() {
        for answer in ["", "null", "[]", "\"text\"", "{", "undefined"] {
            assert_eq!(info(answer), None, "{answer:?}");
        }
    }

    #[test]
    fn colors_print_as_hex() {
        assert_eq!(hex([31, 78, 121]), "#1f4e79");
        assert_eq!(hex([0, 0, 0]), "#000000");
        assert_eq!(FontRole::Headings.label(), "Headings");
    }

    /// A frame of BGRA rows, `pad` bytes of 0xff after each; `blocks` are
    /// bands of rows, from the first row down, each as red, green, blue, and
    /// the count of rows.
    fn frame(width: usize, blocks: &[(Rgb, usize)], pad: usize) -> (Vec<u8>, usize, usize) {
        let stride = width * 4 + pad;
        let mut bytes = Vec::new();
        for &([r, g, b], rows) in blocks {
            for _ in 0..rows {
                for _ in 0..width {
                    bytes.extend([b, g, r, 255]);
                }
                bytes.extend(std::iter::repeat_n(0xff, pad));
            }
        }
        let height = blocks.iter().map(|&(_, rows)| rows).sum();
        (bytes, height, stride)
    }

    fn near(a: Rgb, b: Rgb) -> bool {
        a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 2)
    }

    #[test]
    fn flat_blocks_come_back_exactly_with_their_shares() {
        let blue = [31, 78, 121];
        let yellow = [232, 185, 35];
        let red = [192, 57, 43];
        // Rows are in the buffer's order; the shares are of the whole frame.
        let (bytes, height, stride) = frame(200, &[(yellow, 30), (blue, 50), (red, 20)], 24);
        let swatches = palette(&bytes, 200, height, stride);
        assert_eq!(swatches.len(), 3, "{swatches:?}");
        let expected = [(blue, 0.5), (yellow, 0.3), (red, 0.2)];
        for (swatch, (color, share)) in swatches.iter().zip(expected) {
            assert!(near(swatch.color, color), "{swatch:?} is not {color:?}");
            assert!(
                (swatch.share - share).abs() < 0.03,
                "{swatch:?} is not {share}"
            );
        }
    }

    #[test]
    fn the_padding_of_a_row_is_not_a_color() {
        // Padding of 0xff would make a white swatch if it were read.
        let (bytes, height, stride) = frame(30, &[([10, 20, 30], 40)], 64);
        let swatches = palette(&bytes, 30, height, stride);
        assert_eq!(swatches.len(), 1);
        assert!(near(swatches[0].color, [10, 20, 30]));
        assert!((swatches[0].share - 1.).abs() < 1e-6);
    }

    #[test]
    fn bytes_are_blue_green_red() {
        let bytes: Vec<u8> = [0x10, 0x80, 0xf0, 0xff].repeat(16);
        let swatches = palette(&bytes, 4, 4, 16);
        assert_eq!(swatches.len(), 1);
        assert!(near(swatches[0].color, [0xf0, 0x80, 0x10]), "{swatches:?}");
    }

    #[test]
    fn many_colors_make_at_most_six_swatches_that_add_up() {
        let colors: Vec<(Rgb, usize)> = (0..12u8)
            .map(|i| {
                (
                    [i * 20, 255 - i * 20, i.wrapping_mul(70)],
                    10 + usize::from(i),
                )
            })
            .collect();
        let (bytes, height, stride) = frame(120, &colors, 0);
        let swatches = palette(&bytes, 120, height, stride);
        assert!(
            !swatches.is_empty() && swatches.len() <= COLORS,
            "{swatches:?}"
        );
        let total: f32 = swatches.iter().map(|swatch| swatch.share).sum();
        assert!((total - 1.).abs() < 1e-4, "{total}");
        assert!(
            swatches
                .windows(2)
                .all(|pair| pair[0].share >= pair[1].share)
        );
    }

    #[test]
    fn a_bigger_frame_is_sampled_down_and_gives_the_same_answer_again() {
        let (bytes, height, stride) =
            frame(2000, &[([240, 240, 240], 700), ([20, 20, 60], 700)], 0);
        let started = std::time::Instant::now();
        let swatches = palette(&bytes, 2000, height, stride);
        eprintln!("palette of a 2000x1400 frame: {:?}", started.elapsed());
        assert_eq!(swatches.len(), 2);
        assert!(near(swatches[0].color, [240, 240, 240]) || near(swatches[0].color, [20, 20, 60]));
        assert_eq!(swatches, palette(&bytes, 2000, height, stride));
    }

    #[test]
    fn a_frame_that_shows_nothing_has_no_palette() {
        let clear = [0u8, 0, 0, 0].repeat(64);
        assert!(palette(&clear, 8, 8, 32).is_empty());
        assert!(palette(&[], 0, 0, 0).is_empty());
        assert!(
            palette(&clear, 8, 8, 16).is_empty(),
            "a row shorter than the picture"
        );
        assert!(
            palette(&clear[..40], 8, 8, 32).is_empty(),
            "fewer bytes than rows"
        );
    }
}
