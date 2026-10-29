#![expect(
    unsafe_code,
    reason = "reading the CoreGraphics session dictionary needs a typed view of an untyped dictionary"
)]

use std::time::Duration;

use objc2_core_foundation::{CFBoolean, CFDictionary, CFRetained, CFString, CFType};
use objc2_core_graphics::{
    CGDisplayIsAsleep, CGEventSource, CGEventSourceStateID, CGEventType, CGMainDisplayID,
    CGSessionCopyCurrentDictionary,
};
use yomydev_core::{Desk, Display, Idle, IdleSeconds, Screen};

const ANY_INPUT: CGEventType = CGEventType(u32::MAX);

pub(crate) fn observe() -> Desk {
    Desk {
        screen: screen(),
        display: display(),
        idle: idle(),
    }
}

fn screen() -> Screen {
    let Some(session) = CGSessionCopyCurrentDictionary() else {
        return Screen::Unknown;
    };
    // SAFETY: the CoreGraphics session dictionary maps CFString keys to CoreFoundation values.
    let session = unsafe { CFRetained::cast_unchecked::<CFDictionary<CFString, CFType>>(session) };
    let key = CFString::from_static_str("CGSSessionScreenIsLocked");
    match session.get(&key) {
        None => Screen::Unlocked,
        Some(value) => match value.downcast_ref::<CFBoolean>() {
            Some(locked) if locked.as_bool() => Screen::Locked,
            Some(_) => Screen::Unlocked,
            None => Screen::Unknown,
        },
    }
}

fn display() -> Display {
    if CGDisplayIsAsleep(CGMainDisplayID()) {
        Display::Asleep
    } else {
        Display::Awake
    }
}

fn idle() -> Idle {
    let seconds = CGEventSource::seconds_since_last_event_type(
        CGEventSourceStateID::HIDSystemState,
        ANY_INPUT,
    );
    match Duration::try_from_secs_f64(seconds) {
        Ok(elapsed) => Idle::Since(IdleSeconds::new(
            u32::try_from(elapsed.as_secs()).unwrap_or(u32::MAX),
        )),
        Err(_) => Idle::Unknown,
    }
}
