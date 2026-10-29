#![no_std]
#![forbid(unsafe_code)]

use core::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
pub enum Screen {
    Locked,
    Unlocked,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
pub enum Display {
    Asleep,
    Awake,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
pub struct IdleSeconds(u32);

impl IdleSeconds {
    #[must_use]
    pub const fn new(seconds: u32) -> Self {
        Self(seconds)
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
pub enum Idle {
    Since(IdleSeconds),
    Unknown,
}

impl Idle {
    const fn shorter_than(self, limit: IdleSeconds) -> bool {
        match self {
            Self::Since(seconds) => seconds.0 < limit.0,
            Self::Unknown => false,
        }
    }

    const fn at_least(self, limit: IdleSeconds) -> bool {
        match self {
            Self::Since(seconds) => seconds.0 >= limit.0,
            Self::Unknown => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
pub struct Desk {
    pub screen: Screen,
    pub display: Display,
    pub idle: Idle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
pub struct Luma(u8);

impl Luma {
    #[must_use]
    pub const fn new(mean: u8) -> Self {
        Self(mean)
    }

    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Detail(u8);

impl Detail {
    pub const WHOLE: u8 = 100;

    #[must_use]
    pub const fn new(percent: u8) -> Option<Self> {
        if percent <= Self::WHOLE {
            Some(Self(percent))
        } else {
            None
        }
    }

    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

#[cfg(kani)]
impl kani::Arbitrary for Detail {
    fn any() -> Self {
        Self(kani::any_where(|percent: &u8| *percent <= Self::WHOLE))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
pub struct View {
    pub brightest: Luma,
    pub flattest: Detail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
pub enum CameraFault {
    NotAuthorized,
    Missing,
    Failed,
    TimedOut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
pub enum Sight {
    Person,
    Nobody(View),
    Unavailable(CameraFault),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    Present,
    Absent,
    Unknown,
}

impl Presence {
    pub const ALL: [Self; 3] = [Self::Present, Self::Absent, Self::Unknown];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Present => "present",
            Self::Absent => "absent",
            Self::Unknown => "unknown",
        }
    }
}

impl fmt::Display for Presence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Basis {
    ScreenLocked,
    DisplayAsleep,
    RecentInput,
    PersonSeen,
    NobodySeen,
    DarkAfterLongIdle,
    TooDark,
    FeaturelessView,
    Camera(CameraFault),
}

impl Basis {
    pub const ALL: [Self; 12] = [
        Self::ScreenLocked,
        Self::DisplayAsleep,
        Self::RecentInput,
        Self::PersonSeen,
        Self::NobodySeen,
        Self::DarkAfterLongIdle,
        Self::TooDark,
        Self::FeaturelessView,
        Self::Camera(CameraFault::NotAuthorized),
        Self::Camera(CameraFault::Missing),
        Self::Camera(CameraFault::Failed),
        Self::Camera(CameraFault::TimedOut),
    ];

    #[must_use]
    pub const fn presence(self) -> Presence {
        match self {
            Self::RecentInput | Self::PersonSeen => Presence::Present,
            Self::ScreenLocked
            | Self::DisplayAsleep
            | Self::NobodySeen
            | Self::DarkAfterLongIdle => Presence::Absent,
            Self::TooDark | Self::FeaturelessView | Self::Camera(_) => Presence::Unknown,
        }
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::ScreenLocked => "screen_locked",
            Self::DisplayAsleep => "display_asleep",
            Self::RecentInput => "recent_input",
            Self::PersonSeen => "person_seen",
            Self::NobodySeen => "nobody_seen",
            Self::DarkAfterLongIdle => "dark_after_long_idle",
            Self::TooDark => "too_dark",
            Self::FeaturelessView => "featureless_view",
            Self::Camera(CameraFault::NotAuthorized) => "camera_not_authorized",
            Self::Camera(CameraFault::Missing) => "camera_missing",
            Self::Camera(CameraFault::Failed) => "camera_failed",
            Self::Camera(CameraFault::TimedOut) => "camera_timed_out",
        }
    }
}

impl fmt::Display for Basis {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

pub const RECENT_INPUT: IdleSeconds = IdleSeconds(60);
pub const LONG_IDLE: IdleSeconds = IdleSeconds(300);
pub const DARK: Luma = Luma(40);
pub const FLAT: Detail = Detail(5);

#[derive(Debug, PartialEq, Eq)]
pub struct CameraNeeded {
    idle: Idle,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Decided(Basis),
    NeedsCamera(CameraNeeded),
}

#[must_use]
pub const fn without_camera(desk: Desk) -> Step {
    match (desk.screen, desk.display) {
        (Screen::Locked, Display::Asleep | Display::Awake) => Step::Decided(Basis::ScreenLocked),
        (Screen::Unlocked | Screen::Unknown, Display::Asleep) => {
            Step::Decided(Basis::DisplayAsleep)
        },
        (Screen::Unlocked | Screen::Unknown, Display::Awake) => {
            if desk.idle.shorter_than(RECENT_INPUT) {
                Step::Decided(Basis::RecentInput)
            } else {
                Step::NeedsCamera(CameraNeeded { idle: desk.idle })
            }
        },
    }
}

#[must_use]
#[expect(
    clippy::needless_pass_by_value,
    reason = "taking the token by value lets each camera decision follow exactly one desk decision"
)]
pub const fn with_camera(needed: CameraNeeded, sight: Sight) -> Basis {
    match sight {
        Sight::Unavailable(fault) => Basis::Camera(fault),
        Sight::Person => Basis::PersonSeen,
        Sight::Nobody(view) => {
            if view.brightest.0 < DARK.0 {
                if needed.idle.at_least(LONG_IDLE) {
                    Basis::DarkAfterLongIdle
                } else {
                    Basis::TooDark
                }
            } else if view.flattest.0 < FLAT.0 {
                Basis::FeaturelessView
            } else {
                Basis::NobodySeen
            }
        },
    }
}

pub fn decide(desk: Desk, camera: impl FnOnce() -> Sight) -> Basis {
    match without_camera(desk) {
        Step::Decided(basis) => basis,
        Step::NeedsCamera(needed) => with_camera(needed, camera()),
    }
}

#[cfg(kani)]
mod proofs;
