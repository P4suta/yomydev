use yomydev_core::{
    Basis, CameraFault, Desk, Detail, Display, Idle, IdleSeconds, Luma, Presence, Screen, Sight,
    Step, View, decide, with_camera, without_camera,
};

const SCREENS: [Screen; 3] = [Screen::Locked, Screen::Unlocked, Screen::Unknown];
const DISPLAYS: [Display; 2] = [Display::Asleep, Display::Awake];
const IDLES: [Idle; 7] = [
    Idle::Unknown,
    Idle::Since(IdleSeconds::new(0)),
    Idle::Since(IdleSeconds::new(59)),
    Idle::Since(IdleSeconds::new(60)),
    Idle::Since(IdleSeconds::new(299)),
    Idle::Since(IdleSeconds::new(300)),
    Idle::Since(IdleSeconds::new(u32::MAX)),
];
const FAULTS: [CameraFault; 4] = [
    CameraFault::NotAuthorized,
    CameraFault::Missing,
    CameraFault::Failed,
    CameraFault::TimedOut,
];

const fn nobody(brightest: u8, flattest: u8) -> Option<Sight> {
    match Detail::new(flattest) {
        Some(flattest) => Some(Sight::Nobody(View {
            brightest: Luma::new(brightest),
            flattest,
        })),
        None => None,
    }
}

const fn after_idle(seconds: u32, sight: Option<Sight>) -> Option<Basis> {
    let desk = Desk {
        screen: Screen::Unlocked,
        display: Display::Awake,
        idle: Idle::Since(IdleSeconds::new(seconds)),
    };
    match (without_camera(desk), sight) {
        (Step::NeedsCamera(needed), Some(sight)) => Some(with_camera(needed, sight)),
        (Step::NeedsCamera(_) | Step::Decided(_), _) => None,
    }
}

fn sights() -> Vec<Sight> {
    let mut sights = vec![Sight::Person];
    for brightest in [0, 39, 40, u8::MAX] {
        for flattest in [0, 4, 5, 100] {
            sights.extend(nobody(brightest, flattest));
        }
    }
    sights.extend(FAULTS.map(Sight::Unavailable));
    sights
}

fn expected(desk: Desk, sight: Sight) -> Basis {
    if desk.screen == Screen::Locked {
        return Basis::ScreenLocked;
    }
    if desk.display == Display::Asleep {
        return Basis::DisplayAsleep;
    }
    let idle = match desk.idle {
        Idle::Since(seconds) => Some(seconds.get()),
        Idle::Unknown => None,
    };
    if idle.is_some_and(|seconds| seconds < 60) {
        return Basis::RecentInput;
    }
    match sight {
        Sight::Unavailable(fault) => Basis::Camera(fault),
        Sight::Person => Basis::PersonSeen,
        Sight::Nobody(view) if view.brightest.get() < 40 => {
            if idle.is_some_and(|seconds| seconds >= 300) {
                Basis::DarkAfterLongIdle
            } else {
                Basis::TooDark
            }
        },
        Sight::Nobody(view) if view.flattest.get() < 5 => Basis::FeaturelessView,
        Sight::Nobody(_) => Basis::NobodySeen,
    }
}

#[test]
fn every_desk_and_sight_matches_the_decision_table() {
    let mut seen = Vec::new();
    for screen in SCREENS {
        for display in DISPLAYS {
            for idle in IDLES {
                let desk = Desk {
                    screen,
                    display,
                    idle,
                };
                for sight in sights() {
                    let mut opened = false;
                    let basis = decide(desk, || {
                        opened = true;
                        sight
                    });
                    assert_eq!(basis, expected(desk, sight), "{desk:?} {sight:?}");
                    assert_eq!(
                        opened,
                        matches!(without_camera(desk), Step::NeedsCamera(_)),
                        "{desk:?}"
                    );
                    if !seen.contains(&basis) {
                        seen.push(basis);
                    }
                }
            }
        }
    }
    for basis in Basis::ALL {
        assert!(seen.contains(&basis), "{basis} is unreachable");
    }
}

#[test]
fn the_camera_step_uses_the_idle_carried_from_the_desk() {
    let dark = nobody(39, 100);
    assert_eq!(after_idle(299, dark), Some(Basis::TooDark));
    assert_eq!(after_idle(300, dark), Some(Basis::DarkAfterLongIdle));
}

#[test]
fn a_bright_view_without_edges_is_not_absent_at_any_idle() {
    for seconds in [68, 300, u32::MAX] {
        let basis = after_idle(seconds, nobody(120, 0));
        assert_eq!(basis, Some(Basis::FeaturelessView), "{seconds}");
        assert_eq!(basis.map(Basis::presence), Some(Presence::Unknown));
    }
    assert_eq!(after_idle(68, nobody(120, 5)), Some(Basis::NobodySeen));
}

#[test]
fn detail_is_a_percentage() {
    assert_eq!(Detail::new(100).map(Detail::get), Some(100));
    assert_eq!(Detail::new(101), None);
}

#[test]
fn presence_follows_the_basis() {
    let presences = Basis::ALL.map(Basis::presence);
    assert_eq!(
        presences,
        [
            Presence::Absent,
            Presence::Absent,
            Presence::Present,
            Presence::Present,
            Presence::Absent,
            Presence::Absent,
            Presence::Unknown,
            Presence::Unknown,
            Presence::Unknown,
            Presence::Unknown,
            Presence::Unknown,
            Presence::Unknown,
        ]
    );
}

#[test]
fn names_are_unique_snake_case() {
    let names = Basis::ALL.map(Basis::name);
    for (index, name) in names.iter().enumerate() {
        assert!(
            name.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
            "{name}"
        );
        assert!(!names[index + 1..].contains(name), "{name}");
    }
    assert_eq!(
        Presence::ALL.map(|presence| presence.to_string()),
        ["present", "absent", "unknown"]
    );
}
