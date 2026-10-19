//! Window: presents the newest decoded picture and forwards input.

use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::Arc;

use lindesk_proto::InputEvent;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::platform::scancode::PhysicalKeyExtScancode;
use winit::window::{Fullscreen, Window, WindowId};

use crate::decode::Latest;
use crate::session::Session;

pub enum UserEvent {
    Frame,
    Title(String),
    Quit,
}

const BTN_LEFT: u16 = 0x110;
const BTN_RIGHT: u16 = 0x111;
const BTN_MIDDLE: u16 = 0x112;
const BTN_SIDE: u16 = 0x113;
const BTN_EXTRA: u16 = 0x114;

struct App {
    session: Arc<Session>,
    latest: Latest,
    fullscreen: bool,
    window: Option<Rc<Window>>,
    surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
    ctx: Option<softbuffer::Context<Rc<Window>>>,
    shown: u64,
    cursor: (f64, f64),
}

/// Destination rectangle of the picture inside the window (aspect preserved).
fn layout(win: (u32, u32), pic: (usize, usize)) -> (u32, u32, u32, u32) {
    let (ww, wh) = (win.0 as u64, win.1 as u64);
    let (pw, ph) = (pic.0 as u64, pic.1 as u64);
    if pw == 0 || ph == 0 {
        return (0, 0, win.0, win.1);
    }
    let (dw, dh) = if ww * ph > wh * pw { (wh * pw / ph, wh) } else { (ww, ww * ph / pw) };
    (((ww - dw) / 2) as u32, ((wh - dh) / 2) as u32, dw as u32, dh as u32)
}

impl App {
    fn redraw(&mut self) {
        let (Some(win), Some(surface)) = (self.window.as_ref(), self.surface.as_mut()) else { return };
        let size = win.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else { return };
        if surface.resize(w, h).is_err() {
            return;
        }
        let Ok(mut buf) = surface.buffer_mut() else { return };
        let pic = self.latest.lock().unwrap();
        self.shown = pic.serial;
        buf.fill(0);
        if pic.width == 0 {
            let _ = buf.present();
            return;
        }
        let (ox, oy, dw, dh) = layout((size.width, size.height), (pic.width, pic.height));
        let stride = size.width as usize;
        if dw as usize == pic.width && dh as usize == pic.height {
            for y in 0..dh as usize {
                let dst = (oy as usize + y) * stride + ox as usize;
                buf[dst..dst + dw as usize].copy_from_slice(&pic.pixels[y * pic.width..(y + 1) * pic.width]);
            }
        } else {
            // Nearest-neighbour; fast enough on the CPU, upgrade path is wgpu.
            let xmap: Vec<usize> = (0..dw as usize).map(|x| x * pic.width / dw as usize).collect();
            for y in 0..dh as usize {
                let sy = y * pic.height / dh as usize;
                let src = &pic.pixels[sy * pic.width..(sy + 1) * pic.width];
                let dst = &mut buf[(oy as usize + y) * stride + ox as usize..][..dw as usize];
                for (d, sx) in dst.iter_mut().zip(&xmap) {
                    *d = src[*sx];
                }
            }
        }
        let _ = buf.present();
    }

    fn normalized(&self, x: f64, y: f64) -> Option<(u16, u16)> {
        let win = self.window.as_ref()?;
        let size = win.inner_size();
        let pic = self.latest.lock().unwrap();
        let (ox, oy, dw, dh) = layout((size.width, size.height), (pic.width, pic.height));
        let nx = ((x - ox as f64) / dw.max(1) as f64).clamp(0.0, 1.0);
        let ny = ((y - oy as f64) / dh.max(1) as f64).clamp(0.0, 1.0);
        Some(((nx * 65535.0) as u16, (ny * 65535.0) as u16))
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("lindesk")
            .with_inner_size(PhysicalSize::new(1280, 720))
            .with_fullscreen(self.fullscreen.then_some(Fullscreen::Borderless(None)));
        let win = Rc::new(el.create_window(attrs).expect("create window"));
        let ctx = softbuffer::Context::new(win.clone()).expect("softbuffer context");
        self.surface = Some(softbuffer::Surface::new(&ctx, win.clone()).expect("softbuffer surface"));
        self.ctx = Some(ctx);
        self.window = Some(win);
    }

    fn user_event(&mut self, el: &ActiveEventLoop, ev: UserEvent) {
        if matches!(ev, UserEvent::Quit) {
            el.exit();
            return;
        }
        if let UserEvent::Title(t) = ev {
            if let Some(w) = &self.window {
                w.set_title(&t);
            }
            return;
        }
        let newest = self.latest.lock().unwrap().serial;
        if newest != self.shown {
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _: WindowId, ev: WindowEvent) {
        match ev {
            WindowEvent::CloseRequested => {
                self.session.bye();
                el.exit();
            }
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::Resized(_) => {
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            WindowEvent::Focused(false) => self.session.input(InputEvent::ReleaseAll),
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x, position.y);
                if let Some((x, y)) = self.normalized(position.x, position.y) {
                    self.session.input(InputEvent::MouseAbs { x, y });
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let code = match button {
                    MouseButton::Left => BTN_LEFT,
                    MouseButton::Right => BTN_RIGHT,
                    MouseButton::Middle => BTN_MIDDLE,
                    MouseButton::Back => BTN_SIDE,
                    MouseButton::Forward => BTN_EXTRA,
                    MouseButton::Other(_) => return,
                };
                // Make sure the pointer is where the click is before the click.
                if let Some((x, y)) = self.normalized(self.cursor.0, self.cursor.1) {
                    self.session.input(InputEvent::MouseAbs { x, y });
                }
                self.session.input(InputEvent::Button { code, down: state == ElementState::Pressed });
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (dx, dy) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (x, y),
                    MouseScrollDelta::PixelDelta(p) => (p.x as f32 / 40.0, p.y as f32 / 40.0),
                };
                self.session.input(InputEvent::Wheel { dx: dx.round() as i16, dy: dy.round() as i16 });
            }
            WindowEvent::KeyboardInput { event, .. } => {
                // F11 is local: toggle fullscreen instead of forwarding.
                if event.physical_key == PhysicalKey::Code(KeyCode::F11) {
                    if event.state == ElementState::Pressed && !event.repeat {
                        if let Some(w) = &self.window {
                            let on = w.fullscreen().is_none();
                            w.set_fullscreen(on.then_some(Fullscreen::Borderless(None)));
                        }
                    }
                    return;
                }
                // xkb keycodes are evdev codes + 8.
                let Some(code) = event.physical_key.to_scancode().and_then(|s| s.checked_sub(8)) else { return };
                let state = match (event.state, event.repeat) {
                    (ElementState::Released, _) => 0,
                    (ElementState::Pressed, false) => 1,
                    (ElementState::Pressed, true) => 2,
                };
                self.session.input(InputEvent::Key { code: code as u16, state });
            }
            _ => {}
        }
    }
}

pub fn run(session: Arc<Session>, latest: Latest, fullscreen: bool, build: impl FnOnce(winit::event_loop::EventLoopProxy<UserEvent>)) -> anyhow::Result<()> {
    let el = EventLoop::<UserEvent>::with_user_event().build()?;
    build(el.create_proxy());
    let mut app = App { session, latest, fullscreen, window: None, surface: None, ctx: None, shown: 0, cursor: (0.0, 0.0) };
    el.run_app(&mut app)?;
    Ok(())
}
