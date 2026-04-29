use crate::{
    renderer::{CastFrom, Rndr},
    view::{Position, PositionState},
};
use std::cell::Cell;
use std::{cell::RefCell, panic::Location, rc::Rc};
use web_sys::{Comment, Element, Node, Text};

// ---------------- [HYD] hydration trace instrumentation ----------------
// All of this is unconditional debug instrumentation (no cfg gate) used to
// diagnose the stage hydration panic. Restore the branch to remove.

thread_local! {
    static HYD_DEPTH: Cell<usize> = const { Cell::new(0) };
}

/// `[HYD]` trace: increment the indent depth.
pub fn hyd_depth_inc() {
    HYD_DEPTH.with(|d| d.set(d.get().saturating_add(1)));
}

/// `[HYD]` trace: decrement the indent depth.
pub fn hyd_depth_dec() {
    HYD_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
}

fn hyd_indent() -> String {
    "  ".repeat(HYD_DEPTH.with(|d| d.get()))
}

/// `[HYD]` trace: log a message at current indent.
pub fn hyd_log_msg(msg: &str) {
    web_sys::console::log_1(&wasm_bindgen::JsValue::from_str(&format!(
        "[HYD]{} {}",
        hyd_indent(),
        msg
    )));
}

/// `[HYD]` trace: log a message + a DOM node at current indent.
pub fn hyd_log_node(msg: &str, node: &Node) {
    web_sys::console::log_2(
        &wasm_bindgen::JsValue::from_str(&format!(
            "[HYD]{} {}",
            hyd_indent(),
            msg
        )),
        node,
    );
}

/// `[HYD]` trace: log a cursor walk's from-node and to-node at current indent.
pub fn hyd_log_2nodes(msg: &str, from: &Node, to: &Node) {
    web_sys::console::log_3(
        &wasm_bindgen::JsValue::from_str(&format!(
            "[HYD]{} {} from=",
            hyd_indent(),
            msg
        )),
        from,
        &wasm_bindgen::JsValue::from_str("to="),
    );
    web_sys::console::log_2(
        &wasm_bindgen::JsValue::from_str(&format!(
            "[HYD]{}   ...landed on=",
            hyd_indent()
        )),
        to,
    );
}


#[cfg(feature = "mark_branches")]
const COMMENT_NODE: u16 = 8;

/// Hydration works by walking over the DOM, adding interactivity as needed.
///
/// This cursor tracks the location in the DOM that is currently being hydrated. Each that type
/// implements [`RenderHtml`](crate::view::RenderHtml) knows how to advance the cursor to access
/// the nodes it needs.
#[derive(Debug)]
pub struct Cursor(Rc<RefCell<crate::renderer::types::Node>>);

impl Clone for Cursor {
    fn clone(&self) -> Self {
        Self(Rc::clone(&self.0))
    }
}

impl Cursor
where
    crate::renderer::types::Element: AsRef<crate::renderer::types::Node>,
{
    /// Creates a new cursor starting at the root element.
    pub fn new(root: crate::renderer::types::Element) -> Self {
        let root = <crate::renderer::types::Element as AsRef<
            crate::renderer::types::Node,
        >>::as_ref(&root)
        .clone();
        Self(Rc::new(RefCell::new(root)))
    }

    /// Returns the node at which the cursor is currently located.
    pub fn current(&self) -> crate::renderer::types::Node {
        self.0.borrow().clone()
    }

    /// Advances to the next child of the node at which the cursor is located.
    ///
    /// Does nothing if there is no child.
    #[track_caller]
    pub fn child(&self) {
        let caller = Location::caller();
        let from = self.0.borrow().clone();
        let mut inner = self.0.borrow_mut();
        if let Some(node) = Rndr::first_child(&inner) {
            *inner = node;
        }

        #[cfg(feature = "mark_branches")]
        {
            while inner.node_type() == COMMENT_NODE {
                if let Some(content) = inner.text_content() {
                    if content.starts_with("bo") || content.starts_with("bc") {
                        if let Some(sibling) = Rndr::next_sibling(&inner) {
                            *inner = sibling;
                            continue;
                        }
                    }
                }

                break;
            }
        }
        let to = inner.clone();
        drop(inner);
        hyd_log_2nodes(&format!("Cursor::child   @ {}", caller), &from, &to);
    }

    /// Advances to the next sibling of the node at which the cursor is located.
    ///
    /// Does nothing if there is no sibling.
    #[track_caller]
    pub fn sibling(&self) {
        let caller = Location::caller();
        let from = self.0.borrow().clone();
        let mut inner = self.0.borrow_mut();
        if let Some(node) = Rndr::next_sibling(&inner) {
            *inner = node;
        }

        #[cfg(feature = "mark_branches")]
        {
            while inner.node_type() == COMMENT_NODE {
                if let Some(content) = inner.text_content() {
                    if content.starts_with("bo") || content.starts_with("bc") {
                        if let Some(sibling) = Rndr::next_sibling(&inner) {
                            *inner = sibling;
                            continue;
                        }
                    }
                }
                break;
            }
        }
        let to = inner.clone();
        drop(inner);
        hyd_log_2nodes(&format!("Cursor::sibling @ {}", caller), &from, &to);
    }

    /// Moves to the parent of the node at which the cursor is located.
    ///
    /// Does nothing if there is no parent.
    #[track_caller]
    pub fn parent(&self) {
        let caller = Location::caller();
        let from = self.0.borrow().clone();
        let mut inner = self.0.borrow_mut();
        if let Some(node) = Rndr::get_parent(&inner) {
            *inner = node;
        }
        let to = inner.clone();
        drop(inner);
        hyd_log_2nodes(&format!("Cursor::parent  @ {}", caller), &from, &to);
    }

    /// Sets the cursor to some node.
    #[track_caller]
    pub fn set(&self, node: crate::renderer::types::Node) {
        let caller = Location::caller();
        let from = self.0.borrow().clone();
        *self.0.borrow_mut() = node.clone();
        hyd_log_2nodes(&format!("Cursor::set     @ {}", caller), &from, &node);
    }

    /// Advances to the next placeholder node and returns it
    #[track_caller]
    pub fn next_placeholder(
        &self,
        position: &PositionState,
    ) -> crate::renderer::types::Placeholder {
        let caller = Location::caller();
        hyd_log_msg(&format!(
            "next_placeholder ENTER @ {} position={:?}",
            caller,
            position.get()
        ));
        self.advance_to_placeholder(position);
        let marker = self.current();
        hyd_log_node("next_placeholder result=", &marker);
        crate::renderer::types::Placeholder::cast_from(marker.clone())
            .unwrap_or_else(|| failed_to_cast_marker_node(marker))
    }

    /// Advances to the next placeholder node.
    #[track_caller]
    pub fn advance_to_placeholder(&self, position: &PositionState) {
        let caller = Location::caller();
        hyd_log_msg(&format!(
            "advance_to_placeholder @ {} pos_in={:?}",
            caller,
            position.get()
        ));
        if position.get() == Position::FirstChild {
            self.child();
        } else {
            self.sibling();
        }
        position.set(Position::NextChild);
    }
}

thread_local! {
    static CURRENTLY_HYDRATING: Cell<Option<&'static Location<'static>>> = const { Cell::new(None) };
}

pub(crate) fn set_currently_hydrating(
    location: Option<&'static Location<'static>>,
) {
    CURRENTLY_HYDRATING.set(location);
}

pub(crate) fn failed_to_cast_element(tag_name: &str, node: Node) -> Element {
    let hydrating = CURRENTLY_HYDRATING
        .take()
        .map(|n| n.to_string())
        .unwrap_or_else(|| "{unknown}".to_string());
    hyd_log_node(
        &format!(
            "*** PANIC *** failed_to_cast_element tag=<{}> hydrating={} found=",
            tag_name, hydrating
        ),
        &node,
    );
    web_sys::console::error_3(
        &wasm_bindgen::JsValue::from_str(&format!(
            "A hydration error occurred while trying to hydrate an \
             element defined at {hydrating}.\n\nThe framework expected an \
             HTML <{tag_name}> element, but found this instead: ",
        )),
        &node,
        &wasm_bindgen::JsValue::from_str(
            "\n\nThe hydration mismatch may have occurred slightly \
             earlier, but this is the first time the framework found a \
             node of an unexpected type.",
        ),
    );
    panic!(
        "Unrecoverable hydration error. Please read the error message \
         directly above this for more details."
    );
}

pub(crate) fn failed_to_cast_marker_node(node: Node) -> Comment {
    let hydrating = CURRENTLY_HYDRATING
        .take()
        .map(|n| n.to_string())
        .unwrap_or_else(|| "{unknown}".to_string());
    hyd_log_node(
        &format!(
            "*** PANIC *** failed_to_cast_marker_node hydrating={} found=",
            hydrating
        ),
        &node,
    );
    web_sys::console::error_3(
        &wasm_bindgen::JsValue::from_str(&format!(
            "A hydration error occurred while trying to hydrate an \
             element defined at {hydrating}.\n\nThe framework expected a \
             marker node, but found this instead: ",
        )),
        &node,
        &wasm_bindgen::JsValue::from_str(
            "\n\nThe hydration mismatch may have occurred slightly \
             earlier, but this is the first time the framework found a \
             node of an unexpected type.",
        ),
    );
    panic!(
        "Unrecoverable hydration error. Please read the error message \
         directly above this for more details."
    );
}

pub(crate) fn failed_to_cast_text_node(node: Node) -> Text {
    let hydrating = CURRENTLY_HYDRATING
        .take()
        .map(|n| n.to_string())
        .unwrap_or_else(|| "{unknown}".to_string());
    hyd_log_node(
        &format!(
            "*** PANIC *** failed_to_cast_text_node hydrating={} found=",
            hydrating
        ),
        &node,
    );
    web_sys::console::error_3(
        &wasm_bindgen::JsValue::from_str(&format!(
            "A hydration error occurred while trying to hydrate an \
             element defined at {hydrating}.\n\nThe framework expected a \
             text node, but found this instead: ",
        )),
        &node,
        &wasm_bindgen::JsValue::from_str(
            "\n\nThe hydration mismatch may have occurred slightly \
             earlier, but this is the first time the framework found a \
             node of an unexpected type.",
        ),
    );
    panic!(
        "Unrecoverable hydration error. Please read the error message \
         directly above this for more details."
    );
}
