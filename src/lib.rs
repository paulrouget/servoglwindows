#![feature(box_syntax)]

#[macro_use]
extern crate lazy_static;

#[macro_use]
extern crate log;

extern crate glutin;
extern crate gleam;
extern crate euclid;
extern crate winit;
extern crate synchro_servo;

use euclid::TypedPoint2D;
use gleam::gl;
use std::rc::Rc;
use std::cell::RefCell;
use std::collections::HashMap;
use synchro_servo::{DrawableGeometry, EventLoopRiser, TouchEventType, ScrollLocation};
use synchro_servo::WindowEvent as ServoWindowEvent;
use synchro_servo::Cursor as ServoCursor;

lazy_static! {
    static ref LOOP: glutin::EventsLoop = {
        glutin::EventsLoop::new()
    };
}

thread_local! {
    // FIXME: anyway to not use a refcell?
    static WINDOWS_STATE: RefCell<HashMap<GLWindowId, WindowState>> = RefCell::new(HashMap::new());
}

pub use glutin::WindowId as GLWindowId;

#[derive(Debug)]
pub struct WindowState {
    mouse_position: (i32, i32),
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
                let scroll_location = ScrollLocation::Delta(TypedPoint2D::new(dx, dy));
                let phase = match phase {
                    glutin::TouchPhase::Started => TouchEventType::Down,
                    glutin::TouchPhase::Moved => TouchEventType::Move,
                    glutin::TouchPhase::Ended => TouchEventType::Up,
                    glutin::TouchPhase::Cancelled => TouchEventType::Cancel,
                };
                let (x, y) = self.mouse_position;
                Some(ServoWindowEvent::Scroll(scroll_location, TypedPoint2D::new(x, y), phase))
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

impl GLWindow {
    pub fn new() -> GLWindow {
        let glutin_window = glutin::WindowBuilder::new()
            .with_gl(glutin::GlRequest::Specific(glutin::Api::OpenGl, (3, 2)))
            .with_dimensions(800, 600)
            .with_vsync()
            .build(&LOOP)
            .expect("Failed to create window.");

        let gl = unsafe {
            // FIXME: make_current here?
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
                                           WindowState { mouse_position: (0, 0) });
                           });

        GLWindow {
            glutin_window: glutin_window,
            gl: gl,
        }
    }

    pub fn create_event_loop_riser(&self) -> Box<GLWindowEventLoopRiser> {
        box GLWindowEventLoopRiser
    }

    pub fn show(&self) {
        self.glutin_window.show()
    }

    pub fn swap_buffers(&self) {
        self.glutin_window.swap_buffers().unwrap()
    }

    pub fn set_cursor(&self, cursor: ServoCursor) {
        let glutin_cursor = servo_cursor_to_glutin_cursor(cursor);
        self.glutin_window.set_cursor(glutin_cursor);
    }

    pub fn set_title(&self, title: &str) {
        self.glutin_window.set_title(title);
    }

    pub fn get_gl(&self) -> Rc<gl::Gl> {
        self.gl.clone()
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

pub struct GLWindowEventLoopRiser;

impl EventLoopRiser for GLWindowEventLoopRiser {
    fn clone(&self) -> Box<EventLoopRiser + Send> {
        box GLWindowEventLoopRiser
    }
    fn rise(&self) {
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
