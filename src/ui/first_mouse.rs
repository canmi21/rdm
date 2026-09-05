//! The press that brings the window back from another application asked for the window, not
//! for what lay under the pointer. This element, drawn first in the root, registers the first
//! mouse listener of every frame and swallows that press in the capture phase, before any
//! row, button or backdrop -- however far above the root, and whatever occludes it -- can act
//! on it. An element rather than a listener on the root, because a listener on the root fires
//! only while the root is hovered, and a sheet's backdrop takes that away. See spec/ui.md.

use gpui::{
	App, Bounds, DispatchPhase, Element, ElementId, GlobalElementId, IntoElement, LayoutId,
	MouseDownEvent, Pixels, Style, Window,
};

pub struct FirstMouseGuard;

impl IntoElement for FirstMouseGuard {
	type Element = Self;

	fn into_element(self) -> Self::Element {
		self
	}
}

impl Element for FirstMouseGuard {
	type RequestLayoutState = ();
	type PrepaintState = ();

	fn id(&self) -> Option<ElementId> {
		None
	}

	fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
		None
	}

	fn request_layout(
		&mut self,
		_id: Option<&GlobalElementId>,
		_inspector_id: Option<&gpui::InspectorElementId>,
		window: &mut Window,
		cx: &mut App,
	) -> (LayoutId, Self::RequestLayoutState) {
		(window.request_layout(Style::default(), [], cx), ())
	}

	fn prepaint(
		&mut self,
		_id: Option<&GlobalElementId>,
		_inspector_id: Option<&gpui::InspectorElementId>,
		_bounds: Bounds<Pixels>,
		_request_layout: &mut Self::RequestLayoutState,
		_window: &mut Window,
		_cx: &mut App,
	) -> Self::PrepaintState {
	}

	fn paint(
		&mut self,
		_id: Option<&GlobalElementId>,
		_inspector_id: Option<&gpui::InspectorElementId>,
		_bounds: Bounds<Pixels>,
		_request_layout: &mut Self::RequestLayoutState,
		_prepaint: &mut Self::PrepaintState,
		window: &mut Window,
		_cx: &mut App,
	) {
		window.on_mouse_event(|event: &MouseDownEvent, phase, _, cx| {
			if phase == DispatchPhase::Capture && event.first_mouse {
				cx.stop_propagation();
			}
		});
	}
}
