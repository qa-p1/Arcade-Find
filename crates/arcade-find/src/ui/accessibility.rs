//! Native accessibility for the software overlay. Settings uses eframe's
//! AccessKit feature and the kit's widget semantics. No rendering or fonts
//! live here; names remain complete even when the canvas ellipsizes them.
use std::sync::{Arc, Mutex};

use accesskit::{Action, ActionData, ActionHandler, ActionRequest, ActivationHandler, DeactivationHandler, Live, Node, NodeId, Rect, Role, Tree, TreeUpdate};

use super::model::{metrics, Context, Effect, Entry, Key, KeyEvent, Mode, Overlay};
use crate::service::{Service, UiMsg};

const ROOT: NodeId = NodeId(0);
const INPUT: NodeId = NodeId(1);
const LIST: NodeId = NodeId(2);
const COUNT: NodeId = NodeId(3);
const INPUT_TEXT: NodeId = NodeId(4);
const TOAST: NodeId = NodeId(5);
const KEEP: NodeId = NodeId(10);
const TRASH: NodeId = NodeId(11);
const FIRST_ROW: u64 = 100;

#[derive(Clone)]
pub struct Shared(Arc<Mutex<TreeUpdate>>);

impl Shared {
    pub fn new(overlay: &Overlay) -> Self {
        Self(Arc::new(Mutex::new(build(overlay, 1.0, 0.0))))
    }

    pub fn publish(&self, overlay: &Overlay, scale: f32, margin: f32) {
        *self.0.lock().unwrap_or_else(|error| error.into_inner()) = build(overlay, scale, margin);
    }

    pub fn snapshot(&self) -> TreeUpdate {
        self.0.lock().unwrap_or_else(|error| error.into_inner()).clone()
    }
}

impl ActivationHandler for Shared {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        Some(self.snapshot())
    }
}

pub struct Actions(pub Arc<Service>);
impl ActionHandler for Actions {
    fn do_action(&mut self, request: ActionRequest) {
        self.0.send(UiMsg::Accessibility(request));
    }
}

pub struct Deactivated;
impl DeactivationHandler for Deactivated {
    fn deactivate_accessibility(&mut self) {}
}

fn text_node(label: impl Into<Box<str>>) -> Node {
    let mut node = Node::new(Role::Label);
    node.set_value(label);
    node
}

fn build(overlay: &Overlay, scale: f32, margin: f32) -> TreeUpdate {
    let mut root = Node::new(Role::Window);
    root.set_label("Search — Arcade Find");
    let mut tree = Tree::new(ROOT);
    tree.toolkit_name = Some("Arcade design kit".into());
    tree.toolkit_version = Some(crate::VERSION.into());
    if !overlay.visible {
        root.set_hidden();
        return TreeUpdate { nodes: vec![(ROOT, root)], tree: Some(tree), focus: ROOT };
    }
    let bounds = |x: f32, y: f32, w: f32, h: f32| {
        Rect::new(((x + margin) * scale) as f64, ((y + margin) * scale) as f64, ((x + w + margin) * scale) as f64, ((y + h + margin) * scale) as f64)
    };
    let (label, value) = match &overlay.mode {
        Mode::Rename { input, .. } => ("New name", input.text.as_str()),
        Mode::Actions { filter, .. } => ("Search actions", filter.text.as_str()),
        _ => ("Search your files", overlay.input.text.as_str()),
    };
    let mut input = Node::new(Role::TextInput);
    input.set_label(label);
    input.set_value(value);
    input.set_bounds(bounds(0.0, 0.0, metrics::WIDTH, metrics::BAR));
    input.add_action(Action::Focus);
    input.add_action(Action::SetValue);
    input.set_children(vec![INPUT_TEXT]);
    let mut text = Node::new(Role::TextRun);
    text.set_value(value);
    text.set_character_lengths(value.chars().map(|character| character.len_utf8() as u8).collect::<Vec<_>>());
    let mut nodes = vec![(INPUT, input), (INPUT_TEXT, text)];
    let mut children = vec![INPUT];
    let mut focus = INPUT;
    match &overlay.mode {
        Mode::ConfirmTrash { targets } => {
            let names = targets.iter().map(|row| row.name.as_str()).collect::<Vec<_>>().join(", ");
            let mut notice = text_node(format!("Move {names} to Trash? You can restore these items from Trash."));
            notice.set_live(Live::Polite);
            nodes.push((COUNT, notice));
            children.push(COUNT);
            for (id, label) in [(KEEP, "Keep"), (TRASH, "Move to Trash")] {
                let mut button = Node::new(Role::Button);
                button.set_label(label);
                button.add_action(Action::Click);
                nodes.push((id, button));
                children.push(id);
            }
            focus = KEEP;
        }
        Mode::Details { row } => {
            nodes.push((COUNT, text_node(format!("{}\n{}", row.name, row.path.display()))));
            children.push(COUNT);
        }
        _ => {
            let mut list = Node::new(Role::ListBox);
            list.set_label(if matches!(overlay.mode, Mode::Actions { .. }) { "Actions" } else { "Search results" });
            let mut entries = Vec::new();
            let actions = matches!(overlay.mode, Mode::Actions { .. });
            let count = match &overlay.mode {
                Mode::Actions { items, .. } => items.len(),
                _ => overlay.info.matched,
            };
            for (y, entry) in overlay.layout() {
                let Entry::Row(index) = entry else { continue };
                let id = NodeId(FIRST_ROW + index as u64);
                let mut row = Node::new(Role::ListBoxOption);
                let (selected, disabled) = match &overlay.mode {
                    Mode::Actions { items, sel, .. } => {
                        let item = &items[index];
                        row.set_label(item.title.clone());
                        if let Some(detail) = &item.detail {
                            row.set_description(detail.clone());
                        }
                        (*sel == index, item.disabled)
                    }
                    _ => {
                        let item = &overlay.rows[index];
                        row.set_label(item.name.clone());
                        row.set_description(item.path.to_string_lossy().into_owned());
                        (overlay.is_selected(index), item.missing)
                    }
                };
                row.set_selected(selected);
                row.set_bounds(bounds(0.0, metrics::LIST_TOP + y, metrics::WIDTH, metrics::ROW));
                row.add_action(Action::Focus);
                if disabled {
                    row.set_disabled();
                } else {
                    row.add_action(Action::Click);
                }
                if selected && overlay.nav {
                    focus = id;
                }
                nodes.push((id, row));
                entries.push(id);
            }
            list.set_children(entries);
            list.set_size_of_set(count);
            nodes.push((LIST, list));
            children.push(LIST);
            let message = if !actions && overlay.index.error.is_some() {
                overlay.index.error.clone().unwrap_or_default()
            } else if !actions && overlay.index.loading {
                "Loading the index…".into()
            } else if !actions && overlay.index.indexing.is_some() {
                "Indexing your files…".into()
            } else if !actions && overlay.info.streaming {
                "Searching inside files…".into()
            } else {
                let noun = match (actions, count == 1) {
                    (true, true) => "action",
                    (true, false) => "actions",
                    (false, true) => "result",
                    (false, false) => "results",
                };
                if !actions && count == 0 && !overlay.input.text.trim().is_empty() {
                    format!("0 results for “{}”", overlay.input.text.trim())
                } else {
                    format!("{} {noun}", arcade_link::design::format::format_count(count as i64))
                }
            };
            let mut count = text_node(message);
            count.set_live(Live::Polite);
            nodes.push((COUNT, count));
            children.push(COUNT);
        }
    }
    if let Some(toast) = &overlay.toast {
        let mut node = text_node(toast.text.clone());
        node.set_live(Live::Polite);
        nodes.push((TOAST, node));
        children.push(TOAST);
    }
    root.set_children(children);
    root.set_bounds(bounds(0.0, 0.0, metrics::WIDTH, overlay.height()));
    nodes.push((ROOT, root));
    TreeUpdate { nodes, tree: Some(tree), focus }
}

pub fn apply(overlay: &mut Overlay, request: ActionRequest, context: &dyn Context) -> Vec<Effect> {
    if !overlay.visible {
        return vec![];
    }
    if request.target == INPUT {
        match request.action {
            Action::Focus => {
                overlay.nav = false;
            }
            Action::SetValue => {
                if let Some(ActionData::Value(text)) = request.data {
                    match &mut overlay.mode {
                        Mode::Rename { input, .. } => input.select_all(),
                        Mode::Actions { filter, .. } => filter.select_all(),
                        _ => overlay.input.select_all(),
                    }
                    return overlay.paste(&text);
                }
            }
            _ => {}
        }
    } else if request.action == Action::Click && (request.target == KEEP || request.target == TRASH) && matches!(overlay.mode, Mode::ConfirmTrash { .. }) {
        return overlay.key(&KeyEvent::new(if request.target == KEEP { Key::Escape } else { Key::Enter }), context);
    } else if request.target.0 >= FIRST_ROW && matches!(request.action, Action::Focus | Action::Click) {
        let index = (request.target.0 - FIRST_ROW) as usize;
        if request.action == Action::Focus {
            if let Mode::Actions { items, sel, .. } = &mut overlay.mode {
                if index < items.len() {
                    *sel = index;
                    overlay.nav = true;
                }
                return vec![];
            }
        }
        return overlay.click(index, request.action == Action::Click, false, context);
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{
        input::TextInput,
        model::{ActionId, ActionItem, Glyph, ResultsInfo, Row},
    };

    struct NoPreview;
    impl Context for NoPreview {
        fn can_preview(&self, _: &[Row]) -> bool {
            false
        }
    }

    fn request(action: Action, target: NodeId) -> ActionRequest {
        ActionRequest { action, target, data: None }
    }

    #[test]
    fn focusing_an_action_never_activates_it_and_input_focus_keeps_the_mode() {
        let mut overlay = Overlay::default();
        overlay.show(None, None);
        let target = Row {
            name: "a.txt".into(),
            path: "/home/a.txt".into(),
            parent: "~".into(),
            size: Some(1),
            mtime: 0,
            is_dir: false,
            is_symlink: false,
            kind: find_core::kind::Kind::Text,
            highlights: vec![],
            pinned: false,
            section: None,
            missing: false,
        };
        let items = vec![ActionItem::new(ActionId::CopyPath, "Copy path", Glyph::Copy)];
        overlay.mode = Mode::Actions { all: items.clone(), items, sel: 0, scroll: 0, filter: TextInput::default(), targets: vec![target] };
        assert!(apply(&mut overlay, request(Action::Focus, NodeId(FIRST_ROW)), &NoPreview).is_empty());
        assert!(overlay.visible && matches!(overlay.mode, Mode::Actions { .. }));
        assert!(apply(&mut overlay, request(Action::Focus, INPUT), &NoPreview).is_empty());
        assert!(matches!(overlay.mode, Mode::Actions { .. }) && !overlay.nav);
        assert!(!apply(&mut overlay, request(Action::Click, NodeId(FIRST_ROW)), &NoPreview).is_empty());
    }

    #[test]
    fn empty_action_list_announces_actions_not_previous_results() {
        let mut overlay = Overlay::default();
        overlay.show(None, None);
        overlay.info.matched = 42;
        overlay.mode = Mode::Actions { all: vec![], items: vec![], sel: 0, scroll: 0, filter: TextInput::default(), targets: vec![] };
        let update = build(&overlay, 1.0, 0.0);
        assert_eq!(update.nodes.iter().find(|(id, _)| *id == COUNT).unwrap().1.value(), Some("0 actions"));
    }

    #[test]
    fn live_count_and_full_names_follow_real_results_and_hide_removes_them() {
        let mut overlay = Overlay::default();
        overlay.show(Some("café"), None);
        let name = "café-日本-very-long-name-".repeat(8);
        let row = Row {
            name: name.clone(),
            path: "/home/user/Documents/café.txt".into(),
            parent: "~/Documents".into(),
            size: Some(1),
            mtime: 0,
            is_dir: false,
            is_symlink: false,
            kind: find_core::kind::Kind::Text,
            highlights: vec![],
            pinned: false,
            section: None,
            missing: false,
        };
        overlay.set_results(overlay.seq, vec![row], ResultsInfo { matched: 1, ..Default::default() });
        let update = build(&overlay, 1.25, metrics::SHADOW);
        let count = &update.nodes.iter().find(|(id, _)| *id == COUNT).unwrap().1;
        assert_eq!(count.value(), Some("1 result"));
        assert_eq!(count.live(), Some(Live::Polite));
        let row = &update.nodes.iter().find(|(id, _)| *id == NodeId(FIRST_ROW)).unwrap().1;
        assert_eq!(row.label(), Some(name.as_str()));
        assert!(row.supports_action(Action::Click));
        overlay.hide();
        let hidden = build(&overlay, 1.0, 0.0);
        assert_eq!(hidden.nodes.len(), 1);
        assert!(hidden.nodes[0].1.is_hidden());
        assert_eq!(hidden.focus, ROOT);
    }
}
