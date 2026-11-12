//! Programs laid out in time, against what web Theo's checker makes of the
//! same programs: every built-in performance at three seeds and intensities,
//! and written ones that use each step and each refusal.
mod common;

use common::{differs, fixture, list};
use theo::channels::Channels;
use theo::json::Json;
use theo::program::{Context, Program, as_program, compile};
use theo::random::to_u32;
use theo::tables::tables;

fn lookup(name: &str) -> Option<Program> {
    let name = name.split('@').next().unwrap_or("");
    tables()
        .book
        .iter()
        .find(|entry| entry.str_at("name") == Some(name))
        .and_then(|entry| as_program(entry.get("program")?))
}

#[test]
fn a_program_compiles_to_what_the_web_compiles_it_to() {
    let scores = fixture("scores");
    let channels = Channels::new(&tables().identity.features);
    let mut refused = 0;
    let mut laid_out = 0;
    for case in list(&scores, "scores") {
        let name = case.str_at("name").unwrap();
        let given = case.get("context").unwrap();
        let mut context = Context::new(&channels);
        context.lookup = Some(&lookup);
        context.seed = given.num_at("seed").map(to_u32);
        context.intensity = given.num_at("intensity");
        if let Some(args) = given.get("args").and_then(Json::obj) {
            context.args = args
                .iter()
                .map(|(name, value)| (name.clone(), value.num().unwrap()))
                .collect();
        }
        let ours = match compile(case.get("input").unwrap(), &context) {
            Ok(score) => {
                laid_out += 1;
                Json::Obj(vec![
                    ("ok".to_owned(), Json::Bool(true)),
                    ("score".to_owned(), score.to_json()),
                ])
            }
            Err(refusal) => {
                refused += 1;
                Json::Obj(vec![
                    ("ok".to_owned(), Json::Bool(false)),
                    ("path".to_owned(), Json::Str(refusal.path)),
                    ("reason".to_owned(), Json::Str(refusal.reason)),
                ])
            }
        };
        let theirs = case.get("checked").unwrap();
        if let Some(difference) = differs(&ours, theirs, 1e-9, "checked") {
            panic!("{name} with {given}: {difference}\n ours   {ours}\n theirs {theirs}");
        }
    }
    assert_eq!((laid_out, refused), (100, 18));
}
