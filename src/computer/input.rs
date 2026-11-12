//! Whether the person used the keyboard or a mouse button since a moment,
//! which is a takeover of the desktop's input.
//!
//! SOURCING: Core Graphics' own per-type event counters for the HID system
//! state (`CGEventSourceCounterForEventType`), through `objc2-core-graphics`.
//! They count events from hardware only, so input Cua Driver delivers to a
//! process or through Accessibility does not count as the person's, and they
//! need no Input Monitoring permission. Pointer movement alone is not a
//! takeover: a nudged mouse should not stop Theo.

use objc2_core_graphics::{CGEventSource, CGEventSourceStateID, CGEventType};

const TAKEOVER: [CGEventType; 5] = [
    CGEventType::KeyDown,
    CGEventType::LeftMouseDown,
    CGEventType::RightMouseDown,
    CGEventType::OtherMouseDown,
    CGEventType::ScrollWheel,
];

/// The hardware event counts at one moment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mark([u32; TAKEOVER.len()]);

impl Mark {
    pub fn now() -> Mark {
        Mark(TAKEOVER.map(|kind| {
            CGEventSource::counter_for_event_type(CGEventSourceStateID::HIDSystemState, kind)
        }))
    }

    /// Whether any key or button went down, or the wheel turned, since.
    pub fn person_acted(&self) -> bool {
        Mark::now() != *self
    }
}
