use companion::{Calling, Companion, Scope, StopRequest};
use presence::{Activity, Applied, Phase, Signal, Standing};
use theo::engine::{FaceOptions, Liveness};
use theo::layers::Layer;

fn activity(id: &str, generation: u64, form: &str) -> Activity {
    let mut activity = Activity::new(id, generation);
    activity.phase = Phase::Active;
    activity.preferred_form = Some(form.to_owned());
    activity
}

fn form(theo: &Companion) -> Option<String> {
    theo.face
        .player
        .holding(Layer::Form)
        .and_then(|held| held.name.clone())
}

#[test]
fn the_face_takes_the_form_of_whichever_activity_is_primary() {
    let mut theo = Companion::new(FaceOptions::default());
    assert_eq!(form(&theo), None);
    theo.update(activity("run", 1, "orb"));
    assert_eq!(form(&theo).as_deref(), Some("orb@1"));
    theo.update(activity("sound", 1, "boombox"));
    assert_eq!(form(&theo).as_deref(), Some("boombox@1"));
    // The sound ends and the run, still going, has the face again.
    let mut over = activity("sound", 1, "boombox");
    over.revision = 1;
    over.phase = Phase::Completed;
    assert_eq!(theo.update(over), Applied::Taken { changed: true });
    assert_eq!(form(&theo).as_deref(), Some("orb@1"));
    // A late end for a run of the sound that is long gone ends nothing.
    let mut late = activity("sound", 1, "boombox");
    late.revision = 2;
    late.phase = Phase::Completed;
    assert_eq!(theo.update(late), Applied::Stale);
    assert_eq!(form(&theo).as_deref(), Some("orb@1"));
    let mut done = activity("run", 1, "orb");
    done.revision = 1;
    done.phase = Phase::Failed;
    theo.update(done);
    assert_eq!(form(&theo), None);
    assert_eq!(theo.formed(), None);
}

#[test]
fn a_form_the_book_does_not_have_is_said_and_not_made_up() {
    let mut theo = Companion::new(FaceOptions::default());
    let mut capture = activity("capture", 1, "camera");
    capture.standing = Standing::Brief;
    theo.update(activity("run", 1, "orb"));
    theo.update(capture);
    let formed = theo.formed().expect("the capture is primary");
    assert_eq!(formed.shown.form, "camera");
    assert_eq!(formed.invocation, None);
    assert!(formed.refused.is_some());
    assert_eq!(form(&theo), None);
    // It is not asked for again each frame, and the run comes back after it.
    theo.frame(1_000.0);
    theo.frame(1_016.0);
    assert!(theo.formed().unwrap().refused.is_some());
    let mut over = activity("capture", 1, "camera");
    over.revision = 1;
    over.phase = Phase::Completed;
    theo.update(over);
    assert_eq!(form(&theo).as_deref(), Some("orb@1"));
}

#[test]
fn the_faces_own_rules_do_not_take_the_form_from_the_primary_activity() {
    let mut theo = Companion::new(FaceOptions::default());
    theo.update(activity("sound", 1, "boombox"));
    // A run begins: the face's own rule would make it an orb.
    theo.set_liveness(Liveness::Work);
    assert_eq!(form(&theo).as_deref(), Some("boombox@1"));
    theo.set_liveness(Liveness::Idle);
    assert_eq!(form(&theo).as_deref(), Some("boombox@1"));
}

#[test]
fn hiding_ends_nothing_and_a_stop_names_what_it_is_for() {
    let mut theo = Companion::new(FaceOptions::default());
    let scope = Scope {
        conversation: Some("c1".to_owned()),
        surface: Some("page:7".to_owned()),
        ..Scope::default()
    };
    theo.show(scope.clone());
    theo.open_composer();
    theo.update(activity("music", 3, "boombox"));
    assert_eq!(
        theo.stop(),
        Some(StopRequest {
            activity: "music".to_owned(),
            generation: 3
        })
    );
    // Asking for the stop ended nothing; hiding ends nothing.
    theo.hide();
    assert!(!theo.visible() && !theo.composer_open());
    assert_eq!(theo.presence().activities().count(), 1);
    assert_eq!(form(&theo).as_deref(), Some("boombox@1"));
    // Shown again by its composer, it is where it was called from.
    theo.open_composer();
    assert!(theo.visible() && theo.composer_open());
    assert_eq!(theo.scope(), &scope);
    assert_eq!(Companion::new(FaceOptions::default()).stop(), None);
}

#[test]
fn two_right_clicks_call_it_only_when_asked_to_and_only_together() {
    let mut theo = Companion::new(FaceOptions::default());
    assert!(!theo.right_click(0.0, 10.0, 10.0));
    assert!(!theo.right_click(100.0, 10.0, 10.0));
    theo.set_calling(Calling {
        double_right_click: true,
        ..Calling::default()
    });
    assert!(!theo.right_click(1_000.0, 10.0, 10.0));
    assert!(theo.right_click(1_300.0, 12.0, 11.0));
    // The third is a first again.
    assert!(!theo.right_click(1_400.0, 12.0, 11.0));
    // Too late, then too far.
    assert!(!theo.right_click(2_000.0, 12.0, 11.0));
    assert!(!theo.right_click(2_100.0, 80.0, 11.0));
    assert!(theo.right_click(2_200.0, 80.0, 11.0));
}

#[test]
fn a_measurement_moves_the_face_only_while_it_is_fresh() {
    let mut theo = Companion::new(FaceOptions::default());
    let mut sound = activity("sound", 1, "boombox");
    sound.signals.push(Signal {
        name: "sound.level".to_owned(),
        value: 0.8,
        unit: "ratio".to_owned(),
        source: "playback".to_owned(),
        at: 1_000.0,
        fresh_for: 100.0,
    });
    theo.update(sound);
    theo.frame(1_000.0);
    theo.frame(1_050.0);
    assert_eq!(theo.face.signals.read("sound.level"), 0.8);
    theo.frame(1_200.0);
    assert_eq!(theo.face.signals.read("sound.level"), 0.0);
}
