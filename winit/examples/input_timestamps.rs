//! Print input sample ages and intervals in microseconds.

use std::collections::HashMap;

use softbuffer::{Context, Surface};
use tracing::info;
use winit::Instant;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalPosition;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop, OwnedDisplayHandle};
#[cfg(web_platform)]
use winit::platform::web::WindowAttributesWeb;
use winit::window::{Window, WindowAttributes, WindowId};

#[path = "util/fill.rs"]
mod fill;
#[path = "util/tracing.rs"]
mod tracing;

fn main() -> Result<(), impl std::error::Error> {
    tracing::init();
    EventLoop::new().unwrap().run_app(InputTimestamps::default())
}

#[derive(Default)]
struct InputTimestamps {
    surface: Option<Surface<OwnedDisplayHandle, Box<dyn Window>>>,
    position: Option<PhysicalPosition<f64>>,
    previous: HashMap<&'static str, Instant>,
}

impl ApplicationHandler for InputTimestamps {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        let attributes = WindowAttributes::default().with_title("Input timestamps");
        #[cfg(web_platform)]
        let attributes = attributes
            .with_platform_attributes(Box::new(WindowAttributesWeb::default().with_append(true)));
        let window = event_loop.create_window(attributes).unwrap();
        let context = Context::new(event_loop.owned_display_handle()).unwrap();
        self.surface = Some(Surface::new(&context, window).unwrap());
    }

    fn destroy_surfaces(&mut self, _event_loop: &dyn ActiveEventLoop) {
        self.surface = None;
    }

    fn window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        _window_id: WindowId,
        timestamp: Instant,
        event: WindowEvent,
    ) {
        let kind = match event {
            WindowEvent::RedrawRequested => {
                let surface = self.surface.as_mut().unwrap();
                surface.window().pre_present_notify();
                fill::fill(surface);
                return;
            },
            WindowEvent::PointerMoved { position, .. } => {
                self.position = Some(position);
                "PointerMoved"
            },
            WindowEvent::PointerButton { position, .. } => {
                self.position = Some(position);
                "PointerButton"
            },
            WindowEvent::MouseWheel { .. } => "MouseWheel",
            WindowEvent::KeyboardInput { .. } => "KeyboardInput",
            WindowEvent::CloseRequested => {
                event_loop.exit();
                return;
            },
            _ => return,
        };
        let age = Instant::now().saturating_duration_since(timestamp).as_micros();
        let delta = self.previous.insert(kind, timestamp).map(|previous| {
            if timestamp >= previous {
                timestamp.duration_since(previous).as_micros() as i128
            } else {
                -(previous.duration_since(timestamp).as_micros() as i128)
            }
        });
        info!(
            "{kind}, position={:?}, timestamp age={age} µs, delta since previous={delta:?} µs",
            self.position,
        );
    }
}
