#![feature(box_syntax)]

#[macro_use]
extern crate lazy_static;

#[macro_use]
extern crate log;

#[macro_use]
extern crate bitflags;

extern crate glutin;
extern crate gleam;
extern crate euclid;
extern crate winit;
extern crate servoapi;

use euclid::{TypedPoint2D, TypedVector2D};
use gleam::gl;
use std::rc::Rc;
use std::collections::HashMap;
use servoapi::{DrawableGeometry, GLMethods, EventLoopWaker, TouchEventType, ScrollLocation};
use servoapi::{Key, KeyModifiers as ServoKeyModifiers, KeyState};
use servoapi::{ALT, CONTROL, SHIFT, SUPER};
use servoapi::WindowEvent as ServoWindowEvent;
use servoapi::Cursor as ServoCursor;
use servoapi::{MouseWindowEvent, MouseButton};
use std::cell::{Cell, RefCell};

lazy_static! {
    static ref LOOP: glutin::EventsLoop = {
        glutin::EventsLoop::new()
    };
}

thread_local! {
    static WINDOWS_STATE: RefCell<HashMap<GLWindowId, WindowState>> = RefCell::new(HashMap::new());
}

bitflags! {
    flags KeyModifiers: u8 {
        const LEFT_CONTROL = 1,
        const RIGHT_CONTROL = 2,
        const LEFT_SHIFT = 4,
        const RIGHT_SHIFT = 8,
        const LEFT_ALT = 16,
        const RIGHT_ALT = 32,
        const LEFT_SUPER = 64,
        const RIGHT_SUPER = 128,
    }
}

pub use glutin::WindowId as GLWindowId;


#[derive(Debug)]
pub struct WindowState {
    mouse_position: (i32, i32),
    key_modifiers: Cell<KeyModifiers>,
    pending_key_event_char: Cell<Option<char>>,
    pressed_key_map: RefCell<Vec<(glutin::ScanCode, char)>>,
}

impl WindowState {
    pub fn glutin_event_to_servo_event(&mut self,
                                       event: &glutin::WindowEvent)
                                       -> Option<ServoWindowEvent> {
        match *event {
            glutin::WindowEvent::MouseMoved(x, y) => {
                self.mouse_position = (x, y);
                let servo_event =
                    ServoWindowEvent::MouseWindowMoveEventClass(TypedPoint2D::new(x as f32,
                                                                                  y as f32));
                Some(servo_event)
            }
            glutin::WindowEvent::MouseWheel(delta, phase) => {
                let (mut dx, mut dy) = match delta {
                    // FIXME: magic value
                    glutin::MouseScrollDelta::LineDelta(dx, dy) => (dx, dy * 38.),
                    glutin::MouseScrollDelta::PixelDelta(dx, dy) => (dx, dy),
                };
                if dy.abs() >= dx.abs() {
                    dx = 0.0;
                } else {
                    dy = 0.0;
                }
                let scroll_location = ScrollLocation::Delta(TypedVector2D::new(dx, dy));
                let phase = match phase {
                    glutin::TouchPhase::Started => TouchEventType::Down,
                    glutin::TouchPhase::Moved => TouchEventType::Move,
                    glutin::TouchPhase::Ended => TouchEventType::Up,
                    glutin::TouchPhase::Cancelled => TouchEventType::Cancel,
                };
                let (x, y) = self.mouse_position;
                Some(ServoWindowEvent::Scroll(scroll_location, TypedPoint2D::new(x, y), phase))
            }
            glutin::WindowEvent::MouseInput(glutin::ElementState::Released, glutin::MouseButton::Left) => {
                let (x, y) = self.mouse_position;
                let mouse_event = MouseWindowEvent::Click(MouseButton::Left, TypedPoint2D::new(x as f32, y as f32));
                Some(ServoWindowEvent::MouseWindowEventClass(mouse_event))
            }
            glutin::WindowEvent::ReceivedCharacter(ch) => {
                if !ch.is_control() {
                    self.pending_key_event_char.set(Some(ch));
                }
                None
            }
            glutin::WindowEvent::KeyboardInput(element_state, scan_code, Some(virtual_key_code), _mods) => {


                let m = match virtual_key_code {
                    glutin::VirtualKeyCode::LControl => Some(LEFT_CONTROL),
                    glutin::VirtualKeyCode::RControl => Some(RIGHT_CONTROL),
                    glutin::VirtualKeyCode::LShift => Some(LEFT_SHIFT),
                    glutin::VirtualKeyCode::RShift => Some(RIGHT_SHIFT),
                    glutin::VirtualKeyCode::LAlt => Some(LEFT_ALT),
                    glutin::VirtualKeyCode::RAlt => Some(RIGHT_ALT),
                    glutin::VirtualKeyCode::LWin => Some(LEFT_SUPER),
                    glutin::VirtualKeyCode::RWin => Some(RIGHT_SUPER),
                    _ => None
                };

                // FIXME: use _mods!

                if let Some(modifier) = m {
                    let mut modifiers = self.key_modifiers.get();
                    modifiers.toggle(modifier);
                    self.key_modifiers.set(modifiers);
                }

                let ch = match element_state {
                    glutin::ElementState::Pressed => {
                        let ch = self.pending_key_event_char
                            .get()
                            .and_then(|ch| filter_nonprintable(ch, virtual_key_code));
                        self.pending_key_event_char.set(None);
                        if let Some(ch) = ch {
                            self.pressed_key_map.borrow_mut().push((scan_code, ch));
                        }
                        ch
                    }
                    glutin::ElementState::Released => {
                        let idx = self.pressed_key_map
                            .borrow()
                            .iter()
                            .position(|&(code, _)| code == scan_code);
                        idx.map(|idx| self.pressed_key_map.borrow_mut().swap_remove(idx).1)
                    }
                };

                if let Ok(key) = glutin_key_to_script_key(virtual_key_code) {
                    let state = match element_state {
                        glutin::ElementState::Pressed => KeyState::Pressed,
                        glutin::ElementState::Released => KeyState::Released,
                    };
                    let modifiers = glutin_mods_to_script_mods(self.key_modifiers.get());
                    Some(ServoWindowEvent::KeyEvent(ch, key, state, modifiers))
                } else {
                    None
                }
            }

            _ => {
                None /* FIXME */
            }
        }
    }
}

pub fn run<F: FnMut(ServoWindowEvent, Option<GLWindowId>)>(mut callback: F) {
    loop {
        LOOP.run_forever(|e| {
            match e {
                glutin::Event::WindowEvent {event, window_id} => {
                    WINDOWS_STATE.with(|windows| {
                        let mut windows = windows.borrow_mut();
                        let win_state = windows.get_mut(&window_id);
                        match win_state {
                            Some(win_state) => {
                                match win_state.glutin_event_to_servo_event(&event) {
                                    Some(servo_event) => callback(servo_event, Some(window_id)),
                                    None => {
                                        warn!("Got unknown glutin event: {:?}", event);
                                    }
                                }
                            },
                            None => {
                                // Apparently, Awakened comes with GLWindowId(0),
                                // which is a non existing window
                                match event {
                                    glutin::WindowEvent::Awakened => {
                                        // FIXME: it's surprising that we have Awakened + the interrupt.
                                        // Idle is sent twice.
                                        callback(ServoWindowEvent::Idle, None);
                                    }
                                    _ => {
                                        warn!("Unexpected event ({:?} for unknown Windows ({:?})", event, window_id);
                                    }
                                }

                            }
                        }
                    });
                }
            }
        });
        callback(ServoWindowEvent::Idle, None);
    }
}

pub struct GLWindow {
    gl: Rc<gl::Gl>,
    glutin_window: glutin::Window,
}

impl GLMethods for GLWindow {
    fn swap_buffers(&self) {
        self.glutin_window.swap_buffers().unwrap();
    }
    fn make_current(&self) -> Result<(),()> {
        unsafe {
            self.glutin_window.make_current().map_err(|_| ())
        }
    }
    fn get_gl(&self) -> Rc<gl::Gl> {
        self.gl.clone()
    }
}

impl GLWindow {
    pub fn new(width: u32, height: u32) -> GLWindow {
        let glutin_window = glutin::WindowBuilder::new()
            .with_gl(glutin::GlRequest::Specific(glutin::Api::OpenGl, (3, 2)))
            .with_dimensions(width, height)
            .with_vsync()
            .build(&LOOP)
            .expect("Failed to create window.");

        let gl = unsafe {
            glutin_window
                .make_current()
                .expect("Couldn't make window current");
            gl::GlFns::load_with(|s| glutin_window.get_proc_address(s) as *const _)
        };

        gl.clear_color(1.0, 1.0, 1.0, 1.0);
        gl.clear(gleam::gl::COLOR_BUFFER_BIT);
        gl.finish();

        WINDOWS_STATE.with(|windows| {
                               windows
                                   .borrow_mut()
                                   .insert(glutin_window.id(),
                                           WindowState {
                                               key_modifiers: Cell::new(KeyModifiers::empty()),
                                               mouse_position: (0, 0),
                                               pending_key_event_char: Cell::new(None),
                                               pressed_key_map: RefCell::new(vec![]),
                                           });
                           });

        GLWindow {
            glutin_window: glutin_window,
            gl: gl,
        }
    }

    pub fn id(&self) -> GLWindowId {
        self.glutin_window.id()
    }

    pub fn create_event_loop_waker(&self) -> Box<GLWindowEventLoopWaker> {
        box GLWindowEventLoopWaker
    }

    pub fn show(&self) {
        self.glutin_window.show()
    }

    pub fn set_cursor(&self, cursor: ServoCursor) {
        let glutin_cursor = servo_cursor_to_glutin_cursor(cursor);
        self.glutin_window.set_cursor(glutin_cursor);
    }

    pub fn set_title(&self, title: &str) {
        self.glutin_window.set_title(title);
    }

    pub fn get_geometry(&self) -> DrawableGeometry {
        DrawableGeometry {
            view_size: self.glutin_window
                .get_inner_size()
                .expect("Failed to get window inner size."),
            margins: (0, 0, 0, 0),
            position: self.glutin_window
                .get_position()
                .expect("Failed to get window position."),
            hidpi_factor: self.glutin_window.hidpi_factor(),
        }
    }
}

pub struct GLWindowEventLoopWaker;

impl EventLoopWaker for GLWindowEventLoopWaker {
    fn clone(&self) -> Box<EventLoopWaker + Send> {
        box GLWindowEventLoopWaker
    }
    fn wake(&self) {
        LOOP.interrupt();
    }
}

fn servo_cursor_to_glutin_cursor(servo_cursor: ServoCursor) -> glutin::MouseCursor {
    match servo_cursor {
        ServoCursor::None => glutin::MouseCursor::NoneCursor,
        ServoCursor::Default => glutin::MouseCursor::Default,
        ServoCursor::Pointer => glutin::MouseCursor::Hand,
        ServoCursor::ContextMenu => glutin::MouseCursor::ContextMenu,
        ServoCursor::Help => glutin::MouseCursor::Help,
        ServoCursor::Progress => glutin::MouseCursor::Progress,
        ServoCursor::Wait => glutin::MouseCursor::Wait,
        ServoCursor::Cell => glutin::MouseCursor::Cell,
        ServoCursor::Crosshair => glutin::MouseCursor::Crosshair,
        ServoCursor::Text => glutin::MouseCursor::Text,
        ServoCursor::VerticalText => glutin::MouseCursor::VerticalText,
        ServoCursor::Alias => glutin::MouseCursor::Alias,
        ServoCursor::Copy => glutin::MouseCursor::Copy,
        ServoCursor::Move => glutin::MouseCursor::Move,
        ServoCursor::NoDrop => glutin::MouseCursor::NoDrop,
        ServoCursor::NotAllowed => glutin::MouseCursor::NotAllowed,
        ServoCursor::Grab => glutin::MouseCursor::Grab,
        ServoCursor::Grabbing => glutin::MouseCursor::Grabbing,
        ServoCursor::EResize => glutin::MouseCursor::EResize,
        ServoCursor::NResize => glutin::MouseCursor::NResize,
        ServoCursor::NeResize => glutin::MouseCursor::NeResize,
        ServoCursor::NwResize => glutin::MouseCursor::NwResize,
        ServoCursor::SResize => glutin::MouseCursor::SResize,
        ServoCursor::SeResize => glutin::MouseCursor::SeResize,
        ServoCursor::SwResize => glutin::MouseCursor::SwResize,
        ServoCursor::WResize => glutin::MouseCursor::WResize,
        ServoCursor::EwResize => glutin::MouseCursor::EwResize,
        ServoCursor::NsResize => glutin::MouseCursor::NsResize,
        ServoCursor::NeswResize => glutin::MouseCursor::NeswResize,
        ServoCursor::NwseResize => glutin::MouseCursor::NwseResize,
        ServoCursor::ColResize => glutin::MouseCursor::ColResize,
        ServoCursor::RowResize => glutin::MouseCursor::RowResize,
        ServoCursor::AllScroll => glutin::MouseCursor::AllScroll,
        ServoCursor::ZoomIn => glutin::MouseCursor::ZoomIn,
        ServoCursor::ZoomOut => glutin::MouseCursor::ZoomOut,
    }
}


fn glutin_key_to_script_key(key: glutin::VirtualKeyCode) -> Result<Key, ()> {
    match key {
        glutin::VirtualKeyCode::A => Ok(Key::A),
        glutin::VirtualKeyCode::B => Ok(Key::B),
        glutin::VirtualKeyCode::C => Ok(Key::C),
        glutin::VirtualKeyCode::D => Ok(Key::D),
        glutin::VirtualKeyCode::E => Ok(Key::E),
        glutin::VirtualKeyCode::F => Ok(Key::F),
        glutin::VirtualKeyCode::G => Ok(Key::G),
        glutin::VirtualKeyCode::H => Ok(Key::H),
        glutin::VirtualKeyCode::I => Ok(Key::I),
        glutin::VirtualKeyCode::J => Ok(Key::J),
        glutin::VirtualKeyCode::K => Ok(Key::K),
        glutin::VirtualKeyCode::L => Ok(Key::L),
        glutin::VirtualKeyCode::M => Ok(Key::M),
        glutin::VirtualKeyCode::N => Ok(Key::N),
        glutin::VirtualKeyCode::O => Ok(Key::O),
        glutin::VirtualKeyCode::P => Ok(Key::P),
        glutin::VirtualKeyCode::Q => Ok(Key::Q),
        glutin::VirtualKeyCode::R => Ok(Key::R),
        glutin::VirtualKeyCode::S => Ok(Key::S),
        glutin::VirtualKeyCode::T => Ok(Key::T),
        glutin::VirtualKeyCode::U => Ok(Key::U),
        glutin::VirtualKeyCode::V => Ok(Key::V),
        glutin::VirtualKeyCode::W => Ok(Key::W),
        glutin::VirtualKeyCode::X => Ok(Key::X),
        glutin::VirtualKeyCode::Y => Ok(Key::Y),
        glutin::VirtualKeyCode::Z => Ok(Key::Z),

        glutin::VirtualKeyCode::Numpad0 => Ok(Key::Kp0),
        glutin::VirtualKeyCode::Numpad1 => Ok(Key::Kp1),
        glutin::VirtualKeyCode::Numpad2 => Ok(Key::Kp2),
        glutin::VirtualKeyCode::Numpad3 => Ok(Key::Kp3),
        glutin::VirtualKeyCode::Numpad4 => Ok(Key::Kp4),
        glutin::VirtualKeyCode::Numpad5 => Ok(Key::Kp5),
        glutin::VirtualKeyCode::Numpad6 => Ok(Key::Kp6),
        glutin::VirtualKeyCode::Numpad7 => Ok(Key::Kp7),
        glutin::VirtualKeyCode::Numpad8 => Ok(Key::Kp8),
        glutin::VirtualKeyCode::Numpad9 => Ok(Key::Kp9),

        glutin::VirtualKeyCode::Key0 => Ok(Key::Num0),
        glutin::VirtualKeyCode::Key1 => Ok(Key::Num1),
        glutin::VirtualKeyCode::Key2 => Ok(Key::Num2),
        glutin::VirtualKeyCode::Key3 => Ok(Key::Num3),
        glutin::VirtualKeyCode::Key4 => Ok(Key::Num4),
        glutin::VirtualKeyCode::Key5 => Ok(Key::Num5),
        glutin::VirtualKeyCode::Key6 => Ok(Key::Num6),
        glutin::VirtualKeyCode::Key7 => Ok(Key::Num7),
        glutin::VirtualKeyCode::Key8 => Ok(Key::Num8),
        glutin::VirtualKeyCode::Key9 => Ok(Key::Num9),

        glutin::VirtualKeyCode::Return => Ok(Key::Enter),
        glutin::VirtualKeyCode::Space => Ok(Key::Space),
        glutin::VirtualKeyCode::Escape => Ok(Key::Escape),
        glutin::VirtualKeyCode::Equals => Ok(Key::Equal),
        glutin::VirtualKeyCode::Minus => Ok(Key::Minus),
        glutin::VirtualKeyCode::Back => Ok(Key::Backspace),
        glutin::VirtualKeyCode::PageDown => Ok(Key::PageDown),
        glutin::VirtualKeyCode::PageUp => Ok(Key::PageUp),

        glutin::VirtualKeyCode::Insert => Ok(Key::Insert),
        glutin::VirtualKeyCode::Home => Ok(Key::Home),
        glutin::VirtualKeyCode::Delete => Ok(Key::Delete),
        glutin::VirtualKeyCode::End => Ok(Key::End),

        glutin::VirtualKeyCode::Left => Ok(Key::Left),
        glutin::VirtualKeyCode::Up => Ok(Key::Up),
        glutin::VirtualKeyCode::Right => Ok(Key::Right),
        glutin::VirtualKeyCode::Down => Ok(Key::Down),

        glutin::VirtualKeyCode::LShift => Ok(Key::LeftShift),
        glutin::VirtualKeyCode::LControl => Ok(Key::LeftControl),
        glutin::VirtualKeyCode::LAlt => Ok(Key::LeftAlt),
        glutin::VirtualKeyCode::LWin => Ok(Key::LeftSuper),
        glutin::VirtualKeyCode::RShift => Ok(Key::RightShift),
        glutin::VirtualKeyCode::RControl => Ok(Key::RightControl),
        glutin::VirtualKeyCode::RAlt => Ok(Key::RightAlt),
        glutin::VirtualKeyCode::RWin => Ok(Key::RightSuper),

        glutin::VirtualKeyCode::Apostrophe => Ok(Key::Apostrophe),
        glutin::VirtualKeyCode::Backslash => Ok(Key::Backslash),
        glutin::VirtualKeyCode::Comma => Ok(Key::Comma),
        glutin::VirtualKeyCode::Grave => Ok(Key::GraveAccent),
        glutin::VirtualKeyCode::LBracket => Ok(Key::LeftBracket),
        glutin::VirtualKeyCode::Period => Ok(Key::Period),
        glutin::VirtualKeyCode::RBracket => Ok(Key::RightBracket),
        glutin::VirtualKeyCode::Semicolon => Ok(Key::Semicolon),
        glutin::VirtualKeyCode::Slash => Ok(Key::Slash),
        glutin::VirtualKeyCode::Tab => Ok(Key::Tab),
        glutin::VirtualKeyCode::Subtract => Ok(Key::Minus),

        glutin::VirtualKeyCode::F1 => Ok(Key::F1),
        glutin::VirtualKeyCode::F2 => Ok(Key::F2),
        glutin::VirtualKeyCode::F3 => Ok(Key::F3),
        glutin::VirtualKeyCode::F4 => Ok(Key::F4),
        glutin::VirtualKeyCode::F5 => Ok(Key::F5),
        glutin::VirtualKeyCode::F6 => Ok(Key::F6),
        glutin::VirtualKeyCode::F7 => Ok(Key::F7),
        glutin::VirtualKeyCode::F8 => Ok(Key::F8),
        glutin::VirtualKeyCode::F9 => Ok(Key::F9),
        glutin::VirtualKeyCode::F10 => Ok(Key::F10),
        glutin::VirtualKeyCode::F11 => Ok(Key::F11),
        glutin::VirtualKeyCode::F12 => Ok(Key::F12),

        glutin::VirtualKeyCode::NavigateBackward => Ok(Key::NavigateBackward),
        glutin::VirtualKeyCode::NavigateForward => Ok(Key::NavigateForward),
        _ => Err(()),
    }
}


fn glutin_mods_to_script_mods(modifiers: KeyModifiers) -> ServoKeyModifiers {
    let mut result = ServoKeyModifiers::empty();
    if modifiers.intersects(LEFT_SHIFT | RIGHT_SHIFT) {
        result.insert(SHIFT);
    }
    if modifiers.intersects(LEFT_CONTROL | RIGHT_CONTROL) {
        result.insert(CONTROL);
    }
    if modifiers.intersects(LEFT_ALT | RIGHT_ALT) {
        result.insert(ALT);
    }
    if modifiers.intersects(LEFT_SUPER | RIGHT_SUPER) {
        result.insert(SUPER);
    }
    result
}


fn is_printable(key_code: glutin::VirtualKeyCode) -> bool {
    use glutin::VirtualKeyCode::*;
    match key_code {
        Escape |
        F1 |
        F2 |
        F3 |
        F4 |
        F5 |
        F6 |
        F7 |
        F8 |
        F9 |
        F10 |
        F11 |
        F12 |
        F13 |
        F14 |
        F15 |
        Snapshot |
        Scroll |
        Pause |
        Insert |
        Home |
        Delete |
        End |
        PageDown |
        PageUp |
        Left |
        Up |
        Right |
        Down |
        Back |
        LAlt |
        LControl |
        LMenu |
        LShift |
        LWin |
        Mail |
        MediaSelect |
        MediaStop |
        Mute |
        MyComputer |
        NavigateForward |
        NavigateBackward |
        NextTrack |
        NoConvert |
        PlayPause |
        Power |
        PrevTrack |
        RAlt |
        RControl |
        RMenu |
        RShift |
        RWin |
        Sleep |
        Stop |
        VolumeDown |
        VolumeUp |
        Wake |
        WebBack |
        WebFavorites |
        WebForward |
        WebHome |
        WebRefresh |
        WebSearch |
        WebStop => false,
        _ => true,
    }
}

fn filter_nonprintable(ch: char, key_code: glutin::VirtualKeyCode) -> Option<char> {
    if is_printable(key_code) {
        Some(ch)
    } else {
        None
    }
}

