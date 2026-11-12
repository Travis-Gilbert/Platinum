//! The tables, the generator, the seating and the matter's own numbers,
//! against what web Theo computes.
mod common;

use common::{close, fixture, list, num, numbers};
use theo::feature::{Shape, seat_point};
use theo::json::Json;
use theo::particles::Field;
use theo::random::{Random, seed_of};
use theo::tables::{Side, param_index, tables};

#[test]
fn the_data_is_from_the_commit_the_fixtures_are_from() {
    for name in ["specs", "seats", "field", "scores", "traces"] {
        let from = fixture(name);
        let commit = from
            .get("source")
            .and_then(|source| source.str_at("commit"));
        assert_eq!(commit, Some(tables().commit), "{name}");
    }
}

#[test]
fn the_generator_draws_what_the_web_draws() {
    let specs = fixture("specs");
    for case in list(&specs, "random") {
        let mut random = Random::new(num(case, "seed") as u32);
        for (index, draw) in numbers(case.get("draws").unwrap()).iter().enumerate() {
            assert_eq!(
                random.next(),
                *draw,
                "seed {} draw {index}",
                num(case, "seed")
            );
        }
    }
    for case in list(&specs, "seedOf") {
        let text = case.str_at("text").unwrap();
        assert_eq!(f64::from(seed_of(text)), num(case, "seed"), "{text:?}");
    }
}

fn shape_from(written: &Json) -> Shape {
    let mut shape = Shape::rest(Side::Centre);
    for (key, value) in written.obj().expect("a shape") {
        match key.as_str() {
            "side" => shape.side = value.num().unwrap(),
            "spun" => shape.spun = value.num().unwrap(),
            "stirred" => shape.stirred = value.num().unwrap(),
            "gains" => {
                for (index, gain) in numbers(value).iter().enumerate() {
                    shape.gains[index] = *gain as f32;
                }
            }
            param => shape.set(
                param_index(param).unwrap_or_else(|| panic!("no param {param}")),
                value.num().unwrap(),
            ),
        }
    }
    shape
}

#[test]
fn a_grain_sits_where_the_web_seats_it() {
    let seats = fixture("seats");
    let mut checked = 0;
    for case in list(&seats, "seats") {
        let name = case.str_at("name").unwrap();
        let shape = shape_from(case.get("shape").unwrap());
        for point in list(case, "points") {
            let [theta, u, x, y, light] = numbers(point)[..] else {
                panic!("a point is five numbers")
            };
            let seat = seat_point(&shape, theta, u);
            assert!(
                close(seat.x, x, 1e-9) && close(seat.y, y, 1e-9) && close(seat.light, light, 1e-9),
                "{name} at theta {theta} u {u}: ({}, {}, {}) is not ({x}, {y}, {light})",
                seat.x,
                seat.y,
                seat.light
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 16 * 160);
}

#[test]
fn the_matter_is_laid_out_as_the_web_lays_it() {
    let field = fixture("field");
    for case in list(&field, "fields") {
        let seed = num(case, "seed") as u32;
        let made = Field::new(&tables().identity, seed);
        let (theta, u, phase) = made.seats();
        for (name, ours, theirs) in [
            ("theta", theta, numbers(case.get("theta").unwrap())),
            ("u", u, numbers(case.get("u").unwrap())),
            ("phase", phase, numbers(case.get("phase").unwrap())),
        ] {
            assert_eq!(ours.len(), theirs.len(), "{name}");
            for (index, (a, b)) in ours.iter().zip(&theirs).enumerate() {
                assert!(
                    close(f64::from(*a), *b, 1e-6),
                    "seed {seed} {name}[{index}]: {a} is not {b}"
                );
            }
        }
    }
}
