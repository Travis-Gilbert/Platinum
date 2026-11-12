use presence::{Activity, Applied, ENDED, LIVE, Phase, Presence, Signal, Standing};

fn activity(id: &str, generation: u64, form: &str) -> Activity {
    let mut activity = Activity::new(id, generation);
    activity.phase = Phase::Active;
    activity.preferred_form = Some(form.to_owned());
    activity
}

fn form(presence: &Presence) -> Option<String> {
    presence.primary().map(|shown| shown.form)
}

#[test]
fn a_brief_activity_is_shown_and_the_earlier_one_comes_back() {
    let mut presence = Presence::new();
    assert_eq!(
        presence.apply(activity("music", 1, "record-player")),
        Applied::Taken { changed: true }
    );
    let mut capture = activity("capture", 1, "camera");
    capture.standing = Standing::Brief;
    assert_eq!(presence.apply(capture), Applied::Taken { changed: true });
    assert_eq!(form(&presence).as_deref(), Some("camera"));
    // The receipt arrives: the capture is over, and the music is still playing.
    assert_eq!(presence.end("capture", 1), Applied::Taken { changed: true });
    assert_eq!(form(&presence).as_deref(), Some("record-player"));
}

#[test]
fn a_run_outlives_the_sound_that_was_shown_over_it() {
    let mut presence = Presence::new();
    presence.apply(activity("run", 1, "orb"));
    presence.apply(activity("sound", 1, "boombox"));
    assert_eq!(form(&presence).as_deref(), Some("boombox"));
    presence.end("sound", 1);
    assert_eq!(form(&presence).as_deref(), Some("orb"));
    // And something in the background never takes the face from either.
    let mut sync = activity("sync", 1, "spinner");
    sync.standing = Standing::Ambient;
    assert_eq!(presence.apply(sync), Applied::Taken { changed: false });
    presence.end("run", 1);
    assert_eq!(form(&presence).as_deref(), Some("spinner"));
}

#[test]
fn a_late_stop_for_an_older_track_does_not_stop_a_later_one() {
    let mut presence = Presence::new();
    presence.apply(activity("music", 1, "record-player"));
    presence.apply(activity("music", 2, "record-player"));
    assert_eq!(presence.end("music", 1), Applied::Stale);
    assert_eq!(presence.get("music").map(|music| music.generation), Some(2));
    // Nor does an old account of it replace a newer one.
    let mut old = activity("music", 2, "boombox");
    old.revision = 0;
    assert_eq!(presence.apply(old), Applied::Stale);
    let mut newer = activity("music", 2, "boombox");
    newer.revision = 1;
    assert_eq!(presence.apply(newer), Applied::Taken { changed: true });

    assert_eq!(presence.end("music", 2), Applied::Taken { changed: true });
    assert_eq!(presence.primary(), None);
    // Once ended, nothing older begins it again; a later run does.
    assert_eq!(
        presence.apply(activity("music", 2, "record-player")),
        Applied::Stale
    );
    assert_eq!(
        presence.apply(activity("music", 1, "record-player")),
        Applied::Stale
    );
    assert_eq!(
        presence.apply(activity("music", 3, "record-player")),
        Applied::Taken { changed: true }
    );
    assert_eq!(presence.end("never-began", 1), Applied::Stale);
}

#[test]
fn a_view_made_anew_shows_the_same_thing() {
    let mut presence = Presence::new();
    presence.apply(activity("run", 4, "orb"));
    presence.apply(activity("sound", 2, "boombox"));
    let mut waiting = activity("ask", 1, "ask");
    waiting.phase = Phase::Unknown;
    presence.apply(waiting);
    let shown = presence.primary();

    let mut again = Presence::new();
    assert_eq!(again.restore(presence.snapshot()), 0);
    assert_eq!(again.primary(), shown);
    assert_eq!(again.snapshot(), presence.snapshot());
    // A later run of the earlier activity is the latest begun, before and after.
    presence.apply(activity("run", 5, "orb"));
    assert_eq!(form(&presence).as_deref(), Some("orb"));
    assert_eq!(again.restore(presence.snapshot()), 0);
    assert_eq!(again.primary(), presence.primary());

    // An ended activity in a snapshot is not brought back.
    let mut done = activity("old", 1, "orb");
    done.phase = Phase::Failed;
    assert_eq!(again.restore(vec![done, activity("run", 4, "orb")]), 1);
    assert_eq!(again.activities().count(), 1);
}

#[test]
fn what_is_held_is_bounded_and_says_so() {
    let mut presence = Presence::new();
    for index in 0..LIVE {
        assert!(matches!(
            presence.apply(activity(&format!("a{index}"), 1, "orb")),
            Applied::Taken { .. }
        ));
    }
    assert_eq!(
        presence.apply(activity("one-more", 1, "orb")),
        Applied::Full
    );
    assert_eq!(presence.activities().count(), LIVE);
    // Room is made by an end, not by forgetting one that is still going on.
    presence.end("a0", 1);
    assert!(matches!(
        presence.apply(activity("one-more", 1, "orb")),
        Applied::Taken { .. }
    ));

    // The ended are remembered up to a bound, the oldest forgotten first.
    let mut presence = Presence::new();
    for index in 0..ENDED + 1 {
        let id = format!("e{index}");
        presence.apply(activity(&id, 1, "orb"));
        presence.end(&id, 1);
    }
    assert_eq!(
        presence.apply(activity(&format!("e{ENDED}"), 1, "orb")),
        Applied::Stale
    );
    assert!(matches!(
        presence.apply(activity("e0", 1, "orb")),
        Applied::Taken { .. }
    ));
}

#[test]
fn a_measurement_is_fresh_only_for_as_long_as_it_says() {
    let level = Signal {
        name: "sound.level".to_owned(),
        value: 0.4,
        unit: "ratio".to_owned(),
        source: "playback".to_owned(),
        at: 1_000.0,
        fresh_for: 250.0,
    };
    assert!(level.fresh(1_000.0) && level.fresh(1_250.0));
    assert!(!level.fresh(1_251.0) && !level.fresh(999.0));
    assert_eq!(Phase::parse("awaiting_user"), Some(Phase::AwaitingUser));
    assert!(Phase::Failed.ended() && !Phase::Unknown.ended());
}
