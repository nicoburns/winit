use std::cell::OnceCell;
use std::f64;
use std::time::Duration;

use dpi::{LogicalPosition, PhysicalPosition, Position};
use smol_str::SmolStr;
use tracing::warn;
use wasm_bindgen::prelude::wasm_bindgen;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Event, KeyboardEvent, MouseEvent, Navigator, PointerEvent, WheelEvent};
use web_time::Instant;
use winit_core::event::{
    ButtonSource, FingerId, Force, MouseButton, MouseScrollDelta, PointerKind, PointerSource,
    TabletToolAngle, TabletToolButton, TabletToolData, TabletToolKind, TabletToolTilt,
};
use winit_core::keyboard::{
    Key, KeyCode, KeyLocation, ModifiersState, NamedKey, NativeKey, NativeKeyCode, PhysicalKey,
};

use super::Engine;

pub fn event_time(event: &Event) -> Instant {
    thread_local! {
        static PERFORMANCE: web_sys::Performance = {
            #[wasm_bindgen]
            extern "C" {
                type Global;

                #[wasm_bindgen(method, getter)]
                fn performance(this: &Global) -> web_sys::Performance;
            }

            let global: Global = js_sys::global().unchecked_into();
            global.performance()
        };
    }

    PERFORMANCE.with(|performance| {
        let age = ((performance.now() - event.time_stamp()).max(0.0)) / 1000.0;
        Instant::now() - Duration::from_secs_f64(age)
    })
}

bitflags::bitflags! {
    // https://www.w3.org/TR/pointerevents3/#the-buttons-property
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct ButtonsState: u16 {
        const LEFT    = 0b000001;
        const RIGHT   = 0b000010;
        const MIDDLE  = 0b000100;
        const BACK    = 0b001000;
        const FORWARD = 0b010000;
        const ERASER  = 0b100000;
}}

impl From<TabletToolButton> for ButtonsState {
    fn from(tool: TabletToolButton) -> Self {
        match tool {
            TabletToolButton::Contact => ButtonsState::LEFT,
            TabletToolButton::Barrel => ButtonsState::RIGHT,
            TabletToolButton::Other(value) => Self::from_bits_retain(value),
        }
    }
}

pub fn pointer_buttons(event: &MouseEvent) -> ButtonsState {
    #[allow(clippy::disallowed_methods)]
    ButtonsState::from_bits_retain(event.buttons())
}

pub fn raw_button(event: &MouseEvent) -> Option<u16> {
    // https://www.w3.org/TR/pointerevents3/#the-button-property
    #[allow(clippy::disallowed_methods)]
    let button = event.button();

    if button == -1 {
        None
    } else {
        Some(button.try_into().expect("unexpected negative mouse button value"))
    }
}

pub fn mouse_button(button: u16) -> ButtonSource {
    match button {
        0 => MouseButton::Left.into(),
        1 => MouseButton::Middle.into(),
        2 => MouseButton::Right.into(),
        3 => MouseButton::Back.into(),
        4 => MouseButton::Forward.into(),
        // Codes above 4 are not observed on Firefox or Chromium. 5 is defined as an eraser,
        // which is not a mouse button. No other codes are defined.
        i => ButtonSource::Unknown(i),
    }
}

pub fn tool_button(button: u16) -> TabletToolButton {
    match button {
        0 => TabletToolButton::Contact,
        2 => TabletToolButton::Barrel,
        other => TabletToolButton::Other(other),
    }
}

#[derive(Clone, Copy)]
pub enum WebPointerType {
    Mouse,
    Touch,
    Pen,
}

impl WebPointerType {
    pub fn from_event(event: &PointerEvent) -> Option<Self> {
        #[allow(clippy::disallowed_methods)]
        let r#type = event.pointer_type();

        match r#type.as_ref() {
            "mouse" => Some(Self::Mouse),
            "touch" => Some(Self::Touch),
            "pen" => Some(Self::Pen),
            r#type => {
                warn!("found unknown pointer type: {type}");
                None
            },
        }
    }
}

pub fn pointer_kind(event: &PointerEvent, pointer_id: i32) -> PointerKind {
    match WebPointerType::from_event(event) {
        Some(WebPointerType::Mouse) => PointerKind::Mouse,
        Some(WebPointerType::Touch) => PointerKind::Touch(FingerId::from_raw(pointer_id as usize)),
        Some(WebPointerType::Pen) => {
            PointerKind::TabletTool(if pointer_buttons(event).contains(ButtonsState::ERASER) {
                TabletToolKind::Eraser
            } else {
                TabletToolKind::Pen
            })
        },
        None => PointerKind::Unknown,
    }
}

pub fn pointer_source(event: &PointerEvent, kind: PointerKind) -> PointerSource {
    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(extends = PointerEvent, extends = MouseEvent, extends = Event)]
        pub type PointerEventExt;

        #[wasm_bindgen(method, getter, js_name = altitudeAngle)]
        pub fn altitude_angle(this: &PointerEventExt) -> Option<f64>;

        #[wasm_bindgen(method, getter, js_name = azimuthAngle)]
        pub fn azimuth_angle(this: &PointerEventExt) -> f64;
    }

    let event: &PointerEventExt = event.unchecked_ref();

    match kind {
        PointerKind::Mouse => PointerSource::Mouse,
        PointerKind::Touch(id) => PointerSource::Touch {
            finger_id: id,
            force: Some(Force::Normalized(event.pressure().into())),
        },
        PointerKind::TabletTool(tool) => {
            let data = TabletToolData {
                force: Some(Force::Normalized(event.pressure().into())),
                tangential_force: Some(event.tangential_pressure()),
                twist: Some(event.twist().try_into().expect("found invalid `twist`")),
                tilt: Some(TabletToolTilt {
                    x: event.tilt_x().try_into().expect("found invalid `tiltX`"),
                    y: event.tilt_y().try_into().expect("found invalid `tiltY`"),
                }),
                angle: event
                    .altitude_angle()
                    .map(|altitude| TabletToolAngle { altitude, azimuth: event.azimuth_angle() }),
            };

            PointerSource::TabletTool { kind: tool, data }
        },
        _ => PointerSource::Unknown,
    }
}

pub fn pointer_position(event: &MouseEvent) -> LogicalPosition<f64> {
    #[wasm_bindgen]
    extern "C" {
        type MouseEventExt;

        #[wasm_bindgen(method, getter, js_name = offsetX)]
        fn offset_x(this: &MouseEventExt) -> f64;

        #[wasm_bindgen(method, getter, js_name = offsetY)]
        fn offset_y(this: &MouseEventExt) -> f64;
    }

    let event: &MouseEventExt = event.unchecked_ref();

    LogicalPosition { x: event.offset_x(), y: event.offset_y() }
}

// TODO: Remove this when Firefox supports correct movement values in coalesced events and browsers
// have agreed on what coordinate space `movementX/Y` is using.
// See <https://bugzilla.mozilla.org/show_bug.cgi?id=1753724>.
// See <https://github.com/w3c/pointerlock/issues/42>.
pub enum MouseDelta {
    Chromium,
    Gecko { old_position: LogicalPosition<f64>, old_delta: LogicalPosition<f64> },
    Other,
}

impl MouseDelta {
    pub fn init(navigator: &Navigator, event: &PointerEvent) -> Self {
        match super::engine(navigator) {
            Some(Engine::Chromium) => Self::Chromium,
            // Firefox has wrong movement values in coalesced events.
            Some(Engine::Gecko) if has_coalesced_events_support(event) => Self::Gecko {
                old_position: pointer_position(event),
                old_delta: LogicalPosition::new(
                    event.movement_x() as f64,
                    event.movement_y() as f64,
                ),
            },
            _ => Self::Other,
        }
    }

    pub fn delta(&mut self, event: &MouseEvent) -> Position {
        match self {
            MouseDelta::Chromium => {
                PhysicalPosition::new(event.movement_x(), event.movement_y()).into()
            },
            MouseDelta::Gecko { old_position, old_delta } => {
                let new_position = pointer_position(event);
                let x = new_position.x - old_position.x + old_delta.x;
                let y = new_position.y - old_position.y + old_delta.y;
                *old_position = new_position;
                *old_delta = LogicalPosition::new(0., 0.);
                LogicalPosition::new(x, y).into()
            },
            MouseDelta::Other => {
                LogicalPosition::new(event.movement_x(), event.movement_y()).into()
            },
        }
    }
}

pub fn mouse_scroll_delta(
    window: &web_sys::Window,
    event: &WheelEvent,
) -> Option<MouseScrollDelta> {
    let x = -event.delta_x();
    let y = -event.delta_y();

    match event.delta_mode() {
        WheelEvent::DOM_DELTA_LINE => Some(MouseScrollDelta::LineDelta(x as f32, y as f32)),
        WheelEvent::DOM_DELTA_PIXEL => {
            let delta = LogicalPosition::new(x, y).to_physical(super::scale_factor(window));
            Some(MouseScrollDelta::PixelDelta(delta))
        },
        _ => None,
    }
}

pub fn key_code(event: &KeyboardEvent) -> PhysicalKey {
    // Use keyboard-types' parsing (it is based on the W3C standard).
    match event.code().parse() {
        Ok(KeyCode::Unidentified) => PhysicalKey::Unidentified(NativeKeyCode::Unidentified),
        Ok(code) => PhysicalKey::Code(code),
        Err(err) => {
            tracing::warn!("unknown keyboard input: {err}");
            PhysicalKey::Unidentified(NativeKeyCode::Unidentified)
        },
    }
}

pub fn key(event: &KeyboardEvent) -> Key {
    let key = event.key();
    // Use keyboard-types' parsing (it is based on the W3C standard).
    match key.parse() {
        Ok(NamedKey::Unidentified) => {
            Key::Unidentified(NativeKey::Web(SmolStr::new("Unidentified")))
        },
        Ok(NamedKey::Dead) => Key::Dead(None),
        Ok(named) => Key::Named(named),
        Err(_) => Key::Character(SmolStr::new(key)),
    }
}

pub fn key_text(event: &KeyboardEvent) -> Option<SmolStr> {
    let key = key(event);
    match &key {
        Key::Character(text) => Some(text.clone()),
        Key::Named(NamedKey::Tab) => Some(SmolStr::new("\t")),
        Key::Named(NamedKey::Enter) => Some(SmolStr::new("\r")),
        _ => None,
    }
    .map(SmolStr::new)
}

pub fn key_location(event: &KeyboardEvent) -> KeyLocation {
    match event.location() {
        KeyboardEvent::DOM_KEY_LOCATION_LEFT => KeyLocation::Left,
        KeyboardEvent::DOM_KEY_LOCATION_RIGHT => KeyLocation::Right,
        KeyboardEvent::DOM_KEY_LOCATION_NUMPAD => KeyLocation::Numpad,
        KeyboardEvent::DOM_KEY_LOCATION_STANDARD => KeyLocation::Standard,
        location => {
            tracing::warn!("Unexpected key location: {location}");
            KeyLocation::Standard
        },
    }
}

pub fn keyboard_modifiers(event: &KeyboardEvent) -> ModifiersState {
    let mut state = ModifiersState::empty();

    if event.shift_key() {
        state |= ModifiersState::SHIFT;
    }
    if event.ctrl_key() {
        state |= ModifiersState::CONTROL;
    }
    if event.alt_key() {
        state |= ModifiersState::ALT;
    }
    if event.meta_key() {
        state |= ModifiersState::META;
    }

    state
}

pub fn mouse_modifiers(event: &MouseEvent) -> ModifiersState {
    let mut state = ModifiersState::empty();

    if event.shift_key() {
        state |= ModifiersState::SHIFT;
    }
    if event.ctrl_key() {
        state |= ModifiersState::CONTROL;
    }
    if event.alt_key() {
        state |= ModifiersState::ALT;
    }
    if event.meta_key() {
        state |= ModifiersState::META;
    }

    state
}

pub fn pointer_move_event(event: PointerEvent) -> impl Iterator<Item = PointerEvent> {
    // make a single iterator depending on the availability of coalesced events
    if has_coalesced_events_support(&event) {
        None.into_iter().chain(
            Some(event.get_coalesced_events().into_iter().map(PointerEvent::unchecked_from_js))
                .into_iter()
                .flatten(),
        )
    } else {
        Some(event).into_iter().chain(None.into_iter().flatten())
    }
}

// TODO: Remove when Safari supports `getCoalescedEvents`.
// See <https://bugs.webkit.org/show_bug.cgi?id=210454>.
pub fn has_coalesced_events_support(event: &PointerEvent) -> bool {
    thread_local! {
        static COALESCED_EVENTS_SUPPORT: OnceCell<bool> = const { OnceCell::new() };
    }

    COALESCED_EVENTS_SUPPORT.with(|support| {
        *support.get_or_init(|| {
            #[wasm_bindgen]
            extern "C" {
                type PointerCoalescedEventsSupport;

                #[wasm_bindgen(method, getter, js_name = getCoalescedEvents)]
                fn has_get_coalesced_events(this: &PointerCoalescedEventsSupport) -> JsValue;
            }

            let support: &PointerCoalescedEventsSupport = event.unchecked_ref();
            !support.has_get_coalesced_events().is_undefined()
        })
    })
}

#[cfg(test)]
mod timestamp_tests {
    use wasm_bindgen_test::wasm_bindgen_test;

    use super::*;

    fn sample(timestamp: f64) -> PointerEvent {
        let event = js_sys::Object::new();
        js_sys::Reflect::set(&event, &"timeStamp".into(), &timestamp.into()).unwrap();
        event.unchecked_into()
    }

    fn coalesced(samples: &[PointerEvent]) -> PointerEvent {
        let event = sample(0.0);
        let array = samples.iter().collect::<js_sys::Array>();
        js_sys::Reflect::set(&event, &"samples".into(), &array).unwrap();
        js_sys::Reflect::set(
            &event,
            &"getCoalescedEvents".into(),
            &js_sys::Function::new_no_args("return this.samples"),
        )
        .unwrap();
        event
    }

    #[wasm_bindgen_test]
    fn coalesced_samples_keep_their_times() {
        let performance: web_sys::Performance =
            js_sys::Reflect::get(&js_sys::global(), &"performance".into())
                .unwrap()
                .unchecked_into();
        let sample_interval = performance.now() / 2.0;
        let event = coalesced(&[sample(0.0), sample(sample_interval)]);
        let times: Vec<_> = pointer_move_event(event).map(|event| event_time(&event)).collect();
        assert_eq!(times.len(), 2);
        let interval = times[1].duration_since(times[0]);
        assert!(
            interval.abs_diff(Duration::from_secs_f64(sample_interval / 1000.0))
                < Duration::from_millis(10)
        );
    }

    #[wasm_bindgen_test]
    fn empty_coalesced_samples() {
        assert_eq!(pointer_move_event(coalesced(&[])).count(), 0);
    }

    #[wasm_bindgen_test]
    fn future_timestamp_is_clamped_to_now() {
        let before = Instant::now();
        let timestamp = event_time(&sample(1e30));
        let after = Instant::now();
        assert!(before <= timestamp && timestamp <= after);
    }
}
