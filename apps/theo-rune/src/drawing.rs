//! Geometry an authored behavior draws: paths in transformed groups, in
//! points about the face's centre. The engine's features are superellipses
//! with fixed parameters; a path can be any outline at all, so a behavior can
//! give Theo a form the rig has no feature for.
//!
//! The limits below bound the cost of drawing, not what may be drawn.

use rune::Color;
use rune::shape::{LineCap, Shape};
use rune_luau::Data;

pub const MAX_PATHS: usize = 512;
pub const MAX_POINTS: usize = 8_192;
/// How far from the face's centre a point may be, in points.
pub const MAX_REACH: f32 = 4_096.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub x: f32,
    pub y: f32,
    /// Radians, clockwise on screen.
    pub rotate: f32,
    pub scale: f32,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            rotate: 0.0,
            scale: 1.0,
        }
    }
}

impl Transform {
    /// Scales, then rotates, then moves a point.
    pub fn apply(self, [x, y]: [f32; 2]) -> [f32; 2] {
        let (sin, cos) = self.rotate.sin_cos();
        let (x, y) = (x * self.scale, y * self.scale);
        [x * cos - y * sin + self.x, x * sin + y * cos + self.y]
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Path {
    pub points: Vec<[f32; 2]>,
    pub closed: bool,
    pub fill: Option<Color>,
    pub stroke: Option<(Color, f32)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub transform: Transform,
    pub paths: Vec<Path>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Drawing {
    pub groups: Vec<Group>,
}

impl Drawing {
    /// Reads what a script passed to `theo.draw`: a list of groups, each
    /// `{ x, y, rotate, scale, paths = { { points = {{x, y}, ...}, closed,
    /// fill = "#rrggbb", stroke = "#rrggbb", width } } }`. Says where the
    /// first thing it cannot read is.
    pub fn decode(data: &Data) -> Result<Drawing, String> {
        let groups = match data {
            Data::Nil => return Ok(Drawing::default()),
            Data::List(groups) => groups,
            _ => return Err("a drawing is a list of groups".into()),
        };
        let mut budget = Budget::default();
        let groups = groups
            .iter()
            .enumerate()
            .map(|(i, g)| decode_group(g, &format!("[{}]", i + 1), &mut budget))
            .collect::<Result<_, _>>()?;
        Ok(Drawing { groups })
    }

    pub fn is_empty(&self) -> bool {
        self.groups.iter().all(|g| g.paths.is_empty())
    }

    /// Each path's points where they land, about `centre`.
    pub fn placed(&self, centre: (f64, f64)) -> impl Iterator<Item = (&Path, Vec<[f32; 2]>)> {
        let (cx, cy) = (centre.0 as f32, centre.1 as f32);
        self.groups.iter().flat_map(move |group| {
            group.paths.iter().map(move |path| {
                let points = path
                    .points
                    .iter()
                    .map(|&p| {
                        let [x, y] = group.transform.apply(p);
                        [x + cx, y + cy]
                    })
                    .collect();
                (path, points)
            })
        })
    }

    /// Paints the drawing onto `shape` about `centre`.
    pub fn paint(&self, mut shape: Shape, centre: (f64, f64), alpha: f32) -> Shape {
        for (path, points) in self.placed(centre) {
            if let Some(fill) = path.fill {
                shape = shape.fill(points.clone(), fill.alpha(fill.a * alpha));
            }
            if let Some((colour, width)) = path.stroke {
                shape = shape.stroke_with(
                    points,
                    path.closed,
                    width,
                    LineCap::Round,
                    colour.alpha(colour.a * alpha),
                );
            }
        }
        shape
    }
}

/// What a drawing has spent of its limits so far.
#[derive(Default)]
struct Budget {
    paths: usize,
    points: usize,
}

fn number(data: &Data, at: &str, default: f32) -> Result<f32, String> {
    match data {
        Data::Nil => Ok(default),
        _ => data
            .as_f64()
            .map(|n| n as f32)
            .filter(|n| n.is_finite())
            .ok_or_else(|| format!("{at}: not a number")),
    }
}

fn colour(data: &Data, at: &str) -> Result<Option<Color>, String> {
    match data {
        Data::Nil => Ok(None),
        Data::String(hex) => rune::color::hex(hex)
            .map(Some)
            .ok_or_else(|| format!("{at}: \"{hex}\" is not #rgb, #rrggbb or #rrggbbaa")),
        _ => Err(format!("{at}: a colour is a hex string")),
    }
}

fn decode_group(data: &Data, at: &str, budget: &mut Budget) -> Result<Group, String> {
    let transform = Transform {
        x: number(data.get("x"), &format!("{at}.x"), 0.0)?,
        y: number(data.get("y"), &format!("{at}.y"), 0.0)?,
        rotate: number(data.get("rotate"), &format!("{at}.rotate"), 0.0)?,
        scale: number(data.get("scale"), &format!("{at}.scale"), 1.0)?,
    };
    let paths = match data.get("paths") {
        Data::Nil => Vec::new(),
        Data::List(paths) => paths
            .iter()
            .enumerate()
            .map(|(i, p)| decode_path(p, &format!("{at}.paths[{}]", i + 1), budget))
            .collect::<Result<_, _>>()?,
        _ => return Err(format!("{at}.paths: a list of paths")),
    };
    Ok(Group { transform, paths })
}

fn decode_path(data: &Data, at: &str, budget: &mut Budget) -> Result<Path, String> {
    budget.paths += 1;
    if budget.paths > MAX_PATHS {
        return Err(format!("{at}: more than {MAX_PATHS} paths"));
    }
    let Data::List(raw) = data.get("points") else {
        return Err(format!("{at}.points: a list of {{x, y}}"));
    };
    budget.points += raw.len();
    if budget.points > MAX_POINTS {
        return Err(format!(
            "{at}.points: more than {MAX_POINTS} points in the drawing"
        ));
    }
    let points = raw
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let here = format!("{at}.points[{}]", i + 1);
            let Data::List(xy) = p else {
                return Err(format!("{here}: not a point {{x, y}}"));
            };
            let [x, y] = [0, 1].map(|j| xy.get(j).and_then(Data::as_f64).map(|n| n as f32));
            match (x, y) {
                (Some(x), Some(y)) if x.abs() <= MAX_REACH && y.abs() <= MAX_REACH => Ok([x, y]),
                (Some(_), Some(_)) => {
                    Err(format!("{here}: farther than {MAX_REACH} from the face"))
                }
                _ => Err(format!("{here}: not a point {{x, y}}")),
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    let fill = colour(data.get("fill"), &format!("{at}.fill"))?;
    let stroke = colour(data.get("stroke"), &format!("{at}.stroke"))?
        .map(|c| {
            number(data.get("width"), &format!("{at}.width"), 1.5).map(|w| (c, w.clamp(0.25, 64.0)))
        })
        .transpose()?;
    let closed = matches!(data.get("closed"), Data::Bool(true))
        || (fill.is_some() && !matches!(data.get("closed"), Data::Bool(false)));
    Ok(Path {
        points,
        closed,
        fill,
        stroke,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn map(entries: &[(&str, Data)]) -> Data {
        Data::Map(
            entries
                .iter()
                .map(|(k, v)| ((*k).to_owned(), v.clone()))
                .collect::<BTreeMap<_, _>>(),
        )
    }

    fn point(x: f64, y: f64) -> Data {
        Data::List(vec![Data::Number(x), Data::Number(y)])
    }

    fn triangle() -> Data {
        map(&[
            (
                "points",
                Data::List(vec![
                    point(0.0, -10.0),
                    point(10.0, 10.0),
                    point(-10.0, 10.0),
                ]),
            ),
            ("closed", Data::Bool(true)),
            ("fill", Data::String("#ffcc00".into())),
        ])
    }

    #[test]
    fn a_group_scales_then_rotates_then_moves() {
        let t = Transform {
            x: 5.0,
            y: 5.0,
            rotate: std::f32::consts::FRAC_PI_2,
            scale: 2.0,
        };
        let [x, y] = t.apply([10.0, 0.0]);
        assert!((x - 5.0).abs() < 1e-4 && (y - 25.0).abs() < 1e-4, "{x},{y}");
    }

    #[test]
    fn an_authored_outline_lands_about_the_face() {
        let data = Data::List(vec![map(&[
            ("x", Data::Number(0.0)),
            ("y", Data::Integer(-40)),
            ("paths", Data::List(vec![triangle()])),
        ])]);
        let drawing = Drawing::decode(&data).unwrap();
        let placed: Vec<_> = drawing.placed((100.0, 100.0)).collect();
        assert_eq!(placed.len(), 1);
        assert_eq!(
            placed[0].1,
            vec![[100.0, 50.0], [110.0, 70.0], [90.0, 70.0]]
        );
        assert_eq!(placed[0].0.fill, rune::color::hex("#ffcc00"));
        assert!(!drawing.paint(Shape::new(), (100.0, 100.0), 1.0).is_empty());
    }

    #[test]
    fn what_cannot_be_read_is_named_where_it_is() {
        let bad_colour = Data::List(vec![map(&[(
            "paths",
            Data::List(vec![map(&[
                ("points", Data::List(vec![point(0.0, 0.0), point(1.0, 1.0)])),
                ("fill", Data::String("yellowish".into())),
            ])]),
        )])]);
        let err = Drawing::decode(&bad_colour).unwrap_err();
        assert!(err.starts_with("[1].paths[1].fill"), "{err}");

        let bad_point = Data::List(vec![map(&[(
            "paths",
            Data::List(vec![map(&[(
                "points",
                Data::List(vec![point(0.0, 0.0), Data::String("x".into())]),
            )])]),
        )])]);
        let err = Drawing::decode(&bad_point).unwrap_err();
        assert!(err.starts_with("[1].paths[1].points[2]"), "{err}");
    }

    #[test]
    fn cost_is_bounded_not_meaning() {
        let many: Vec<Data> = (0..MAX_POINTS + 1)
            .map(|i| point(i as f64 % 100.0, 0.0))
            .collect();
        let data = Data::List(vec![map(&[(
            "paths",
            Data::List(vec![map(&[
                ("points", Data::List(many)),
                ("stroke", Data::String("#fff".into())),
            ])]),
        )])]);
        assert!(Drawing::decode(&data).unwrap_err().contains("points"));

        let far = Data::List(vec![map(&[(
            "paths",
            Data::List(vec![map(&[(
                "points",
                Data::List(vec![point(0.0, 0.0), point(1e6, 0.0)]),
            )])]),
        )])]);
        assert!(Drawing::decode(&far).is_err());
    }

    #[test]
    fn nothing_drawn_is_an_empty_drawing() {
        assert!(Drawing::decode(&Data::List(vec![])).unwrap().is_empty());
        assert!(
            Drawing::decode(&Data::Nil).unwrap().is_empty(),
            "nil clears"
        );
    }
}
