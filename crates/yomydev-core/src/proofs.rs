use super::*;

const fn reaches_the_camera(desk: Desk) -> bool {
    let unlocked = !matches!(desk.screen, Screen::Locked);
    let awake = matches!(desk.display, Display::Awake);
    let recent = match desk.idle {
        Idle::Since(seconds) => seconds.get() < 60,
        Idle::Unknown => false,
    };
    unlocked && awake && !recent
}

const fn long_idle(desk: Desk) -> bool {
    matches!(desk.idle, Idle::Since(seconds) if seconds.get() >= 300)
}

#[kani::proof]
fn camera_is_read_only_in_the_ambiguous_branch() {
    let desk: Desk = kani::any();
    let sight: Sight = kani::any();
    let mut opened = false;
    let basis = decide(desk, || {
        opened = true;
        sight
    });
    assert_eq!(opened, reaches_the_camera(desk));
    kani::cover!(basis == Basis::ScreenLocked);
    kani::cover!(basis == Basis::DisplayAsleep);
    kani::cover!(basis == Basis::RecentInput);
    kani::cover!(basis == Basis::PersonSeen);
    kani::cover!(basis == Basis::NobodySeen);
    kani::cover!(basis == Basis::DarkAfterLongIdle);
    kani::cover!(basis == Basis::TooDark);
    kani::cover!(basis == Basis::FeaturelessView);
    kani::cover!(basis == Basis::Camera(CameraFault::NotAuthorized));
    kani::cover!(basis == Basis::Camera(CameraFault::Missing));
    kani::cover!(basis == Basis::Camera(CameraFault::Failed));
    kani::cover!(basis == Basis::Camera(CameraFault::TimedOut));
}

#[kani::proof]
fn an_unavailable_camera_never_reads_absent() {
    let desk: Desk = kani::any();
    let fault: CameraFault = kani::any();
    kani::assume(reaches_the_camera(desk));
    let basis = decide(desk, || Sight::Unavailable(fault));
    assert_eq!(basis, Basis::Camera(fault));
    assert_eq!(basis.presence(), Presence::Unknown);
}

#[kani::proof]
fn an_empty_view_reads_absent_exactly_when_clear_or_dark_after_long_idle() {
    let desk: Desk = kani::any();
    let view: View = kani::any();
    kani::assume(reaches_the_camera(desk));
    let basis = decide(desk, || Sight::Nobody(view));
    let dark = view.brightest.get() < 40;
    let clear = !dark && view.flattest.get() >= 5;
    assert_eq!(
        basis.presence() == Presence::Absent,
        clear || (dark && long_idle(desk))
    );
    kani::cover!(clear);
    kani::cover!(dark && long_idle(desk));
    kani::cover!(dark && !long_idle(desk));
    kani::cover!(!dark && !clear);
}

#[kani::proof]
fn a_seen_person_is_never_absent() {
    let desk: Desk = kani::any();
    let mut opened = false;
    let basis = decide(desk, || {
        opened = true;
        Sight::Person
    });
    if opened {
        assert_eq!(basis, Basis::PersonSeen);
        assert_eq!(basis.presence(), Presence::Present);
    }
    kani::cover!(opened);
}

#[kani::proof]
fn locked_or_asleep_is_absent_without_the_camera() {
    let desk: Desk = kani::any();
    let sight: Sight = kani::any();
    kani::assume(matches!(desk.screen, Screen::Locked) || matches!(desk.display, Display::Asleep));
    let mut opened = false;
    let basis = decide(desk, || {
        opened = true;
        sight
    });
    assert!(!opened);
    assert_eq!(basis.presence(), Presence::Absent);
}

#[kani::proof]
fn presence_is_never_absent_for_an_unknown_idle_without_a_clear_view() {
    let desk: Desk = kani::any();
    let sight: Sight = kani::any();
    kani::assume(matches!(desk.idle, Idle::Unknown));
    let basis = decide(desk, || sight);
    if basis.presence() == Presence::Absent {
        let clear_view = matches!(
            sight,
            Sight::Nobody(view) if view.brightest.get() >= 40 && view.flattest.get() >= 5
        );
        let locked_or_asleep =
            matches!(desk.screen, Screen::Locked) || matches!(desk.display, Display::Asleep);
        assert!(clear_view || locked_or_asleep);
    }
}
