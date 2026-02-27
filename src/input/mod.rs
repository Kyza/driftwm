mod actions;
mod pointer;

use smithay::{
    backend::input::{
        AbsolutePositionEvent, Event, InputBackend, InputEvent, KeyState, KeyboardKeyEvent,
    },
    desktop::{layer_map_for_output, WindowSurfaceType},
    input::keyboard::FilterResult,
    input::pointer::MotionEvent,
    utils::{Point, SERIAL_COUNTER},
    wayland::shell::wlr_layer::Layer as WlrLayer,
};

use driftwm::canvas::{ScreenPos, screen_to_canvas};
use crate::state::{DriftWm, FocusTarget};

impl DriftWm {
    /// Process a single input event from any backend (winit, libinput, etc).
    pub fn process_input_event<I: InputBackend>(&mut self, event: InputEvent<I>) {
        match event {
            InputEvent::Keyboard { event } => self.on_keyboard::<I>(event),
            InputEvent::PointerMotionAbsolute { event } => {
                self.on_pointer_motion_absolute::<I>(event)
            }
            InputEvent::PointerButton { event } => self.on_pointer_button::<I>(event),
            InputEvent::PointerAxis { event } => self.on_pointer_axis::<I>(event),
            _ => {}
        }
    }

    fn on_keyboard<I: InputBackend>(&mut self, event: I::KeyboardKeyEvent) {
        let serial = SERIAL_COUNTER.next_serial();
        let time = Event::time_msec(&event);
        let key_state = event.state();
        let keycode = event.key_code();
        let keycode_u32: u32 = keycode.into();

        // Clear key repeat on release of the held key
        if key_state == KeyState::Released
            && let Some((held_keycode, _, _)) = &self.held_action
            && *held_keycode == keycode_u32
        {
            self.held_action = None;
        }

        let keyboard = self.seat.get_keyboard().unwrap();

        let action = keyboard.input(
            self,
            keycode,
            key_state,
            serial,
            time,
            |state, modifiers, handle| {
                // If cycling is active and the cycle modifier was released, end cycle
                if state.cycle_state.is_some()
                    && !state.config.cycle_modifier.is_pressed(modifiers)
                {
                    state.end_cycle();
                    return FilterResult::Forward;
                }

                if key_state == KeyState::Pressed {
                    let sym = handle.modified_sym();

                    if let Some(action) = state.config.lookup(modifiers, sym) {
                        return FilterResult::Intercept(action.clone());
                    }
                }
                FilterResult::Forward
            },
        );

        if let Some(ref action) = action {
            // Set up key repeat for repeatable actions
            if action.is_repeatable() {
                let delay = std::time::Duration::from_millis(self.config.repeat_delay as u64);
                self.held_action = Some((keycode_u32, action.clone(), std::time::Instant::now() + delay));
            } else {
                // Non-repeatable action pressed — cancel any active repeat
                self.held_action = None;
            }
            self.execute_action(action);
        }
    }

    fn on_pointer_motion_absolute<I: InputBackend>(
        &mut self,
        event: I::PointerMotionAbsoluteEvent,
    ) {
        let output = match self.space.outputs().next() {
            Some(o) => o.clone(),
            None => return,
        };
        let output_geo = self.space.output_geometry(&output).unwrap();

        // position_transformed gives screen-local coords (0..width, 0..height)
        let screen_pos = event.position_transformed(output_geo.size);
        let canvas_pos = screen_to_canvas(ScreenPos(screen_pos), self.camera, self.zoom).0;
        let serial = SERIAL_COUNTER.next_serial();
        let time = Event::time_msec(&event);
        let pointer = self.seat.get_pointer().unwrap();

        // Pointer always stays in canvas coords so cursor rendering and grabs
        // work consistently. Layer surface focus locations are adjusted to
        // compensate (see layer_surface_under).

        // Check Overlay and Top layers at screen coords
        if let Some(hit) = self.layer_surface_under(screen_pos, canvas_pos, &[WlrLayer::Overlay, WlrLayer::Top]) {
            self.pointer_over_layer = true;
            pointer.motion(self, Some(hit), &MotionEvent { location: canvas_pos, serial, time });
            pointer.frame(self);
            return;
        }

        // Check canvas windows at canvas coords
        let under = self.surface_under(canvas_pos);
        if under.is_some() {
            self.pointer_over_layer = false;
            pointer.motion(self, under, &MotionEvent { location: canvas_pos, serial, time });
            pointer.frame(self);
            return;
        }

        // Check Bottom and Background layers at screen coords
        if let Some(hit) = self.layer_surface_under(screen_pos, canvas_pos, &[WlrLayer::Bottom, WlrLayer::Background]) {
            self.pointer_over_layer = true;
            pointer.motion(self, Some(hit), &MotionEvent { location: canvas_pos, serial, time });
            pointer.frame(self);
            return;
        }

        // No hit — empty canvas
        self.pointer_over_layer = false;
        pointer.motion(self, None, &MotionEvent { location: canvas_pos, serial, time });
        pointer.frame(self);
    }

    /// Find the Wayland surface and local coordinates under the given canvas position.
    /// This is the foundation for all hit-testing — focus, gestures, resize grabs.
    pub fn surface_under(
        &self,
        pos: Point<f64, smithay::utils::Logical>,
    ) -> Option<(FocusTarget, Point<f64, smithay::utils::Logical>)> {
        self.space
            .element_under(pos)
            .and_then(|(window, window_loc)| {
                window
                    .surface_under(
                        pos - window_loc.to_f64(),
                        WindowSurfaceType::ALL,
                    )
                    .map(|(surface, surface_loc)| {
                        (FocusTarget(surface), (surface_loc + window_loc).to_f64())
                    })
            })
    }

    /// Find a layer surface under the given screen-space position.
    /// Checks the given layers in order.
    ///
    /// Returns a focus target with a *canvas-adjusted* location: smithay computes
    /// surface-local coords as `pointer_pos - focus_loc`, and the pointer is always
    /// in canvas coords, so we offset the screen-space location by `canvas_pos - screen_pos`
    /// to keep the surface-local math correct.
    pub(crate) fn layer_surface_under(
        &self,
        screen_pos: Point<f64, smithay::utils::Logical>,
        canvas_pos: Point<f64, smithay::utils::Logical>,
        layers: &[WlrLayer],
    ) -> Option<(FocusTarget, Point<f64, smithay::utils::Logical>)> {
        let output = self.space.outputs().next()?;
        let map = layer_map_for_output(output);
        for &layer in layers {
            if let Some(surface) = map.layer_under(layer, screen_pos) {
                let geo = map.layer_geometry(surface).unwrap_or_default();
                let surface_local = screen_pos - geo.loc.to_f64();
                if let Some((wl_surface, sub_loc)) =
                    surface.surface_under(surface_local, WindowSurfaceType::ALL)
                {
                    let screen_loc = (sub_loc + geo.loc).to_f64();
                    // Adjust so: canvas_pos - adjusted = screen_pos - screen_loc
                    let adjusted = screen_loc + (canvas_pos - screen_pos);
                    return Some((FocusTarget(wl_surface), adjusted));
                }
            }
        }
        None
    }
}
