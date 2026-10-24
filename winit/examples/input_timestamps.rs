//! Print input sample ages and intervals in microseconds.

use std::collections::HashMap;

use tracing::info;
use winit::Instant;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalPosition;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

#[path = "util/tracing.rs"]
mod tracing;

fn main() -> Result<(), impl std::error::Error> {
    tracing::init();
    EventLoop::new().unwrap().run_app(InputTimestamps::default())
}

#[derive(Default)]
struct InputTimestamps {
    window: Option<Box<dyn Window>>,
    position: Option<PhysicalPosition<f64>>,
    previous: HashMap<&'static str, Instant>,
}

impl ApplicationHandler for InputTimestamps {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.window = Some(
            event_loop
                .create_window(WindowAttributes::default().with_title("Input timestamps"))
                .unwrap(),
        );
    }

    fn window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        _window_id: WindowId,
        timestamp: Instant,
        event: WindowEvent,
    ) {
        let kind = match event {
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
