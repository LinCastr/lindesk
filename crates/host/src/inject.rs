//! Input injection through uinput. Works on any Wayland compositor (and X11)
//! because the kernel, not the display server, receives the events. Needs
//! write access to `/dev/uinput` (the `input` group on most distros).

use std::collections::HashSet;

use anyhow::{Context, Result};
use evdev::uinput::VirtualDevice;
use evdev::{
    AbsInfo, AbsoluteAxisCode, AttributeSet, EventType, InputEvent as Ev, KeyCode, RelativeAxisCode,
    UinputAbsSetup,
};
use lindesk_proto::InputEvent;

const BTN_FIRST: u16 = 0x110;
const BTN_LAST: u16 = 0x11f;
const KEY_MAX_KBD: u16 = 248;

pub struct Injector {
    kbd: VirtualDevice,
    ptr: VirtualDevice,
    last_seq: Option<u32>,
    held_keys: HashSet<u16>,
    held_btns: HashSet<u16>,
}

impl Injector {
    pub fn new() -> Result<Self> {
        let mut keys = AttributeSet::<KeyCode>::new();
        for c in 1..=KEY_MAX_KBD {
            keys.insert(KeyCode::new(c));
        }
        let kbd = VirtualDevice::builder()
            .context("open /dev/uinput (add yourself to the `input` group)")?
            .name("lindesk keyboard")
            .with_keys(&keys)?
            .build()?;

        let mut btns = AttributeSet::<KeyCode>::new();
        for c in BTN_FIRST..=BTN_LAST {
            btns.insert(KeyCode::new(c));
        }
        let mut rel = AttributeSet::<RelativeAxisCode>::new();
        rel.insert(RelativeAxisCode::REL_WHEEL);
        rel.insert(RelativeAxisCode::REL_HWHEEL);
        // Same shape as a QEMU/VM "tablet": absolute axes + buttons + wheel.
        let axis = |a| UinputAbsSetup::new(a, AbsInfo::new(0, 0, 65535, 0, 0, 0));
        let ptr = VirtualDevice::builder()?
            .name("lindesk pointer")
            .with_keys(&btns)?
            .with_relative_axes(&rel)?
            .with_absolute_axis(&axis(AbsoluteAxisCode::ABS_X))?
            .with_absolute_axis(&axis(AbsoluteAxisCode::ABS_Y))?
            .build()?;
        Ok(Self { kbd, ptr, last_seq: None, held_keys: HashSet::new(), held_btns: HashSet::new() })
    }

    /// `seq` dedups the redundant copies of key/button events.
    pub fn apply(&mut self, seq: u32, ev: InputEvent) {
        let reliable = matches!(ev, InputEvent::Button { .. } | InputEvent::Key { .. } | InputEvent::ReleaseAll);
        if reliable {
            if let Some(last) = self.last_seq {
                if seq == last || seq.wrapping_sub(last) > u32::MAX / 2 {
                    return;
                }
            }
            self.last_seq = Some(seq);
        }
        let r = match ev {
            InputEvent::MouseAbs { x, y } => self.ptr.emit(&[
                Ev::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_X.0, x as i32),
                Ev::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_Y.0, y as i32),
            ]),
            InputEvent::Button { code, down } if (BTN_FIRST..=BTN_LAST).contains(&code) => {
                if down {
                    self.held_btns.insert(code);
                } else {
                    self.held_btns.remove(&code);
                }
                self.ptr.emit(&[Ev::new(EventType::KEY.0, code, down as i32)])
            }
            InputEvent::Key { code, state } if (1..=KEY_MAX_KBD).contains(&code) && state <= 2 => {
                match state {
                    0 => self.held_keys.remove(&code),
                    1 => self.held_keys.insert(code),
                    _ => false,
                };
                self.kbd.emit(&[Ev::new(EventType::KEY.0, code, state as i32)])
            }
            InputEvent::Wheel { dx, dy } => {
                let mut v = Vec::new();
                if dy != 0 {
                    v.push(Ev::new(EventType::RELATIVE.0, RelativeAxisCode::REL_WHEEL.0, dy as i32));
                }
                if dx != 0 {
                    v.push(Ev::new(EventType::RELATIVE.0, RelativeAxisCode::REL_HWHEEL.0, dx as i32));
                }
                self.ptr.emit(&v)
            }
            InputEvent::ReleaseAll => {
                self.release_all();
                Ok(())
            }
            _ => Ok(()), // out-of-range codes are ignored, never forwarded
        };
        if let Err(e) = r {
            log::warn!("inject: {e}");
        }
    }

    /// Called on disconnect/timeout so a dropped link never leaves a key stuck.
    pub fn release_all(&mut self) {
        let keys: Vec<u16> = self.held_keys.drain().collect();
        for c in keys {
            let _ = self.kbd.emit(&[Ev::new(EventType::KEY.0, c, 0)]);
        }
        let btns: Vec<u16> = self.held_btns.drain().collect();
        for c in btns {
            let _ = self.ptr.emit(&[Ev::new(EventType::KEY.0, c, 0)]);
        }
        self.last_seq = None;
    }
}
