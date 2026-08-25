use super::*;

impl X11Window {
    pub(super) fn a11y_init_impl(&self, callbacks: gpui::A11yCallbacks) {
        let activation_handler = TrivialActivationHandler {
            callback: callbacks.activation,
        };
        let action_handler = TrivialActionHandler(callbacks.action);
        let deactivation_handler = TrivialDeactivationHandler {
            callback: callbacks.deactivation,
        };

        let adapter =
            accesskit_unix::Adapter::new(activation_handler, action_handler, deactivation_handler);

        self.0.state.borrow_mut().accesskit_adapter = Some(adapter);
    }

    pub(super) fn a11y_tree_update_impl(&self, tree_update: ::accesskit::TreeUpdate) {
        let mut state = self.0.state.borrow_mut();
        if let Some(adapter) = state.accesskit_adapter.as_mut() {
            adapter.update_if_active(|| tree_update);
        }
    }

    pub(super) fn a11y_update_window_bounds_impl(&self) {
        let mut state = self.0.state.borrow_mut();
        let scale = state.scale_factor;
        let bounds = state.bounds;
        let [left, right, top, bottom] = state.last_insets;

        let x = f32::from(bounds.origin.x);
        let y = f32::from(bounds.origin.y);
        let width = f32::from(bounds.size.width);
        let height = f32::from(bounds.size.height);

        let outer = ::accesskit::Rect {
            x0: (x * scale) as f64,
            y0: (y * scale) as f64,
            x1: ((x + width) * scale) as f64,
            y1: ((y + height) * scale) as f64,
        };

        let inner = ::accesskit::Rect {
            x0: (x * scale) as f64 + left as f64,
            y0: (y * scale) as f64 + top as f64,
            x1: ((x + width) * scale) as f64 - right as f64,
            y1: ((y + height) * scale) as f64 - bottom as f64,
        };

        if let Some(adapter) = state.accesskit_adapter.as_mut() {
            adapter.set_root_window_bounds(outer, inner);
        }
    }
}

struct TrivialActivationHandler {
    callback: Box<dyn Fn() -> Option<::accesskit::TreeUpdate> + Send + 'static>,
}

impl ::accesskit::ActivationHandler for TrivialActivationHandler {
    fn request_initial_tree(&mut self) -> Option<::accesskit::TreeUpdate> {
        (self.callback)()
    }
}

struct TrivialActionHandler(Box<dyn Fn(::accesskit::ActionRequest) + Send + 'static>);

impl ::accesskit::ActionHandler for TrivialActionHandler {
    fn do_action(&mut self, request: ::accesskit::ActionRequest) {
        (self.0)(request);
    }
}

struct TrivialDeactivationHandler {
    callback: Box<dyn Fn() + Send + 'static>,
}

impl ::accesskit::DeactivationHandler for TrivialDeactivationHandler {
    fn deactivate_accessibility(&mut self) {
        (self.callback)();
    }
}
