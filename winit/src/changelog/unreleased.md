The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

The sections should follow the order `Added`, `Changed`, `Deprecated`,
`Removed`, and `Fixed`.

Platform specific changed should be added to the end of the section and grouped
by platform name. Common API additions should have `, implemented` at the end
for platforms where the API was initially implemented. See the following example
on how to add them:

```md
### Added

- Add `Window::turbo()`, implemented on X11, Wayland, and Web.
- On X11, add `Window::some_rare_api`.
- On X11, add `Window::even_more_rare_api`.
- On Wayland, add `Window::common_api`.
- On Windows, add `Window::some_rare_api`.
```

When the change requires non-trivial amount of work for users to comply
with it, the migration guide should be added below the entry, like:

```md
- Deprecate `Window` creation outside of `EventLoop::run`

  This was done to simply migration in the future. Consider the
  following code:

  // Code snippet.

  To migrate it we should do X, Y, and then Z, for example:

  // Code snippet.

```

The migration guide could reference other migration examples in the current
changelog entry.

## Unreleased

### Changed

- `ApplicationHandler::window_event` and `ApplicationHandler::device_event` now take an additional `timestamp: Instant` parameter between the id and the event. The timestamp is derived from the OS-provided event time where available (macOS via `NSEvent.timestamp`, X11 via `xev.time`, Wayland via the `time` field on `wl_keyboard`/`wl_pointer`/`wl_touch` events and the `utime_hi`/`utime_lo` pair on `zwp_relative_pointer_v1`) and otherwise is sampled close to when winit received the event. Using this timestamp instead of `Instant::now()` when the event is received eliminates the polling-delay latency previously seen under load.

  To migrate, add `timestamp: Instant` to your implementations of `window_event` and `device_event`. Use the `winit::Instant` re-export rather than `std::time::Instant` so the signature compiles on `wasm32-unknown-unknown` (where winit uses `web_time::Instant`) without `cfg`-gated imports:

  ```rust
  use winit::Instant;

  fn window_event(
      &mut self,
      event_loop: &dyn ActiveEventLoop,
      window_id: WindowId,
      timestamp: Instant, // new
      event: WindowEvent,
  ) { /* ... */ }
  ```

