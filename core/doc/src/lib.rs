//! The Chitrakar document model.
//!
//! A document is a tree of live, non-destructive nodes (see docs/PLAN.md §2).
//! All mutation goes through [`Command`]s applied via [`Document::apply`],
//! which returns the inverse command — undo/redo falls out of that in
//! [`History`].

mod node;

pub use node::{
    Adjustment, BlendMode, Filter, Node, NodeKind, RasterRef, Stroke, Transform, VectorShape,
};

use chitrakar_color::ColorMode;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NodeId(pub u64);

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum DocError {
    #[error("unknown node id {0:?}")]
    UnknownNode(NodeId),
    #[error("node {0:?} is not a group")]
    NotAGroup(NodeId),
    #[error("index {index} out of bounds for group {group:?} with {len} children")]
    IndexOutOfBounds {
        group: NodeId,
        index: usize,
        len: usize,
    },
    #[error("cannot remove the root group")]
    CannotRemoveRoot,
    #[error("cannot move {0:?} into its own subtree")]
    MoveIntoOwnSubtree(NodeId),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentMeta {
    pub width: u32,
    pub height: u32,
    pub dpi: f32,
    pub color_mode: ColorMode,
}

/// The scene graph. Nodes live in a flat arena keyed by [`NodeId`]; groups
/// hold ordered child-id lists (topmost child last, i.e. painter's order).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub meta: DocumentMeta,
    root: NodeId,
    nodes: HashMap<NodeId, Node>,
    children: HashMap<NodeId, Vec<NodeId>>,
    next_id: u64,
    /// Immutable, content-addressed pixel sources referenced by RasterRef
    /// nodes. Dimensions serialize with the manifest; the bytes live as
    /// separate files in the .chitra container and are restored on load.
    #[serde(default)]
    resources: HashMap<String, Resource>,
}

/// An immutable source image (8-bit sRGB RGBA). The original bytes a raster
/// object points at — never edited, only referenced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resource {
    pub width: u32,
    pub height: u32,
    #[serde(skip)]
    pub rgba8: Vec<u8>,
}

impl Document {
    pub fn new(width: u32, height: u32, color_mode: ColorMode) -> Self {
        let root = NodeId(0);
        let mut nodes = HashMap::new();
        nodes.insert(root, Node::group("root"));
        let mut children = HashMap::new();
        children.insert(root, Vec::new());
        Self {
            meta: DocumentMeta {
                width,
                height,
                dpi: 72.0,
                color_mode,
            },
            root,
            nodes,
            children,
            next_id: 1,
            resources: HashMap::new(),
        }
    }

    /// Add pixel bytes to the resource pool, returning their content id.
    /// Identical content shares one entry. Not a [`Command`]: the pool is an
    /// immutable store, only node references to it are document state.
    pub fn add_resource(&mut self, width: u32, height: u32, rgba8: Vec<u8>) -> String {
        let id = content_id(width, height, &rgba8);
        self.resources.entry(id.clone()).or_insert(Resource {
            width,
            height,
            rgba8,
        });
        id
    }

    pub fn resource(&self, id: &str) -> Option<&Resource> {
        self.resources.get(id)
    }

    pub fn resources(&self) -> impl Iterator<Item = (&String, &Resource)> {
        self.resources.iter()
    }

    /// Re-attach pixel bytes to a resource whose metadata came from a
    /// deserialized manifest (bytes are stored outside the manifest).
    pub fn restore_resource_bytes(&mut self, id: &str, rgba8: Vec<u8>) -> bool {
        match self.resources.get_mut(id) {
            Some(r) if rgba8.len() == (r.width * r.height * 4) as usize => {
                r.rgba8 = rgba8;
                true
            }
            _ => false,
        }
    }

    /// Whether `maybe_descendant` is inside the subtree rooted at `ancestor`
    /// (a node counts as its own descendant).
    fn is_descendant(&self, ancestor: NodeId, maybe_descendant: NodeId) -> bool {
        let mut stack = vec![ancestor];
        while let Some(n) = stack.pop() {
            if n == maybe_descendant {
                return true;
            }
            if let Some(kids) = self.children.get(&n) {
                stack.extend(kids.iter().copied());
            }
        }
        false
    }

    fn parent_and_index(&self, id: NodeId) -> Option<(NodeId, usize)> {
        self.children
            .iter()
            .find_map(|(p, kids)| kids.iter().position(|k| *k == id).map(|i| (*p, i)))
    }

    pub fn root(&self) -> NodeId {
        self.root
    }

    pub fn node(&self, id: NodeId) -> Result<&Node, DocError> {
        self.nodes.get(&id).ok_or(DocError::UnknownNode(id))
    }

    pub fn children_of(&self, id: NodeId) -> Result<&[NodeId], DocError> {
        match self.children.get(&id) {
            Some(c) => Ok(c),
            None if self.nodes.contains_key(&id) => Ok(&[]),
            None => Err(DocError::UnknownNode(id)),
        }
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Whether any filter layer exists. Filters read pixel neighborhoods, so
    /// while one is present, incremental invalidation falls back to
    /// whole-canvas (a region render could sample stale surroundings).
    pub fn has_filter(&self) -> bool {
        self.nodes
            .values()
            .any(|n| matches!(n.kind, NodeKind::Filter(_)))
    }

    /// Apply a command, returning its inverse (for undo).
    pub fn apply(&mut self, cmd: Command) -> Result<Command, DocError> {
        match cmd {
            Command::AddNode {
                parent,
                index,
                node,
            } => {
                if !matches!(self.node(parent)?.kind, NodeKind::Group) {
                    return Err(DocError::NotAGroup(parent));
                }
                let siblings = &self.children[&parent];
                if index > siblings.len() {
                    return Err(DocError::IndexOutOfBounds {
                        group: parent,
                        index,
                        len: siblings.len(),
                    });
                }
                let id = NodeId(self.next_id);
                self.next_id += 1;
                if matches!(node.kind, NodeKind::Group) {
                    self.children.insert(id, Vec::new());
                }
                self.nodes.insert(id, *node);
                self.children.get_mut(&parent).unwrap().insert(index, id);
                Ok(Command::RemoveNode { id })
            }
            Command::RemoveNode { id } => {
                if id == self.root {
                    return Err(DocError::CannotRemoveRoot);
                }
                self.node(id)?;
                let (parent, index) = self
                    .children
                    .iter()
                    .find_map(|(p, kids)| kids.iter().position(|k| *k == id).map(|i| (*p, i)))
                    .ok_or(DocError::UnknownNode(id))?;
                self.children.get_mut(&parent).unwrap().remove(index);
                // Removing a group takes its subtree with it; restore is a
                // whole-subtree re-add.
                let subtree = self.detach_subtree(id);
                Ok(Command::RestoreSubtree {
                    parent,
                    index,
                    subtree,
                })
            }
            Command::RestoreSubtree {
                parent,
                index,
                subtree,
            } => {
                self.children
                    .get(&parent)
                    .ok_or(DocError::UnknownNode(parent))?;
                let id = subtree.root_id;
                for (nid, node) in subtree.nodes {
                    self.nodes.insert(nid, node);
                }
                for (nid, kids) in subtree.children {
                    self.children.insert(nid, kids);
                }
                self.children.get_mut(&parent).unwrap().insert(index, id);
                Ok(Command::RemoveNode { id })
            }
            Command::SetOpacity { id, opacity } => {
                let node = self.nodes.get_mut(&id).ok_or(DocError::UnknownNode(id))?;
                let prev = node.opacity;
                node.opacity = opacity.clamp(0.0, 1.0);
                Ok(Command::SetOpacity { id, opacity: prev })
            }
            Command::SetVisible { id, visible } => {
                let node = self.nodes.get_mut(&id).ok_or(DocError::UnknownNode(id))?;
                let prev = node.visible;
                node.visible = visible;
                Ok(Command::SetVisible { id, visible: prev })
            }
            Command::SetBlendMode { id, blend } => {
                let node = self.nodes.get_mut(&id).ok_or(DocError::UnknownNode(id))?;
                let prev = node.blend;
                node.blend = blend;
                Ok(Command::SetBlendMode { id, blend: prev })
            }
            Command::SetTransform { id, transform } => {
                let node = self.nodes.get_mut(&id).ok_or(DocError::UnknownNode(id))?;
                let prev = node.transform;
                node.transform = transform;
                Ok(Command::SetTransform {
                    id,
                    transform: prev,
                })
            }
            Command::SetKind { id, kind } => {
                let node = self.nodes.get_mut(&id).ok_or(DocError::UnknownNode(id))?;
                // Structural kinds (Group) can't be swapped with leaf kinds;
                // this command is for parameter edits on leaves.
                let prev = std::mem::replace(&mut node.kind, *kind);
                Ok(Command::SetKind {
                    id,
                    kind: Box::new(prev),
                })
            }
            Command::SetName { id, name } => {
                let node = self.nodes.get_mut(&id).ok_or(DocError::UnknownNode(id))?;
                let prev = std::mem::replace(&mut node.name, name);
                Ok(Command::SetName { id, name: prev })
            }
            Command::MoveNode { id, parent, index } => {
                if id == self.root {
                    return Err(DocError::CannotRemoveRoot);
                }
                if !matches!(self.node(parent)?.kind, NodeKind::Group) {
                    return Err(DocError::NotAGroup(parent));
                }
                if self.is_descendant(id, parent) {
                    return Err(DocError::MoveIntoOwnSubtree(id));
                }
                let (old_parent, old_index) =
                    self.parent_and_index(id).ok_or(DocError::UnknownNode(id))?;
                self.children
                    .get_mut(&old_parent)
                    .unwrap()
                    .remove(old_index);
                let dest = self.children.get_mut(&parent).unwrap();
                dest.insert(index.min(dest.len()), id);
                Ok(Command::MoveNode {
                    id,
                    parent: old_parent,
                    index: old_index,
                })
            }
        }
    }

    fn detach_subtree(&mut self, id: NodeId) -> Subtree {
        let mut subtree = Subtree {
            root_id: id,
            nodes: Vec::new(),
            children: Vec::new(),
        };
        let mut stack = vec![id];
        while let Some(nid) = stack.pop() {
            if let Some(node) = self.nodes.remove(&nid) {
                subtree.nodes.push((nid, node));
            }
            if let Some(kids) = self.children.remove(&nid) {
                stack.extend(kids.iter().copied());
                subtree.children.push((nid, kids));
            }
        }
        subtree
    }
}

/// Content id of a resource: FNV-1a over dimensions and bytes, hex-encoded.
/// Stable across sessions so identical placements dedupe in the pool and in
/// saved containers.
fn content_id(width: u32, height: u32, rgba8: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |b: u8| {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    };
    for b in width.to_le_bytes().into_iter().chain(height.to_le_bytes()) {
        eat(b);
    }
    for &b in rgba8 {
        eat(b);
    }
    format!("{hash:016x}")
}

/// A detached document fragment carried inside undo data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subtree {
    root_id: NodeId,
    nodes: Vec<(NodeId, Node)>,
    children: Vec<(NodeId, Vec<NodeId>)>,
}

/// Every mutation of a [`Document`]. Serializable — the foundation for undo,
/// history persistence, and future collaboration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
    AddNode {
        parent: NodeId,
        index: usize,
        node: Box<Node>,
    },
    RemoveNode {
        id: NodeId,
    },
    RestoreSubtree {
        parent: NodeId,
        index: usize,
        subtree: Subtree,
    },
    SetOpacity {
        id: NodeId,
        opacity: f32,
    },
    SetVisible {
        id: NodeId,
        visible: bool,
    },
    SetBlendMode {
        id: NodeId,
        blend: BlendMode,
    },
    SetTransform {
        id: NodeId,
        transform: Transform,
    },
    SetKind {
        id: NodeId,
        kind: Box<NodeKind>,
    },
    SetName {
        id: NodeId,
        name: String,
    },
    /// Reparent/reorder a node. `index` is the position in the destination
    /// group's child list (painter's order: 0 = bottom).
    MoveNode {
        id: NodeId,
        parent: NodeId,
        index: usize,
    },
}

/// Linear undo/redo stacks of inverse commands.
#[derive(Debug, Default)]
pub struct History {
    undo: Vec<Command>,
    redo: Vec<Command>,
}

impl History {
    pub fn apply(&mut self, doc: &mut Document, cmd: Command) -> Result<(), DocError> {
        let inverse = doc.apply(cmd)?;
        self.undo.push(inverse);
        self.redo.clear();
        Ok(())
    }

    pub fn undo(&mut self, doc: &mut Document) -> Result<bool, DocError> {
        match self.undo.pop() {
            None => Ok(false),
            Some(inv) => {
                let redo = doc.apply(inv)?;
                self.redo.push(redo);
                Ok(true)
            }
        }
    }

    pub fn redo(&mut self, doc: &mut Document) -> Result<bool, DocError> {
        match self.redo.pop() {
            None => Ok(false),
            Some(cmd) => {
                let inv = doc.apply(cmd)?;
                self.undo.push(inv);
                Ok(true)
            }
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(name: &str) -> Box<Node> {
        Box::new(Node::vector(
            name,
            VectorShape::Rect {
                width: 10.0,
                height: 10.0,
            },
        ))
    }

    #[test]
    fn add_then_undo_then_redo() {
        let mut doc = Document::new(800, 600, ColorMode::Rgb);
        let mut history = History::default();
        let root = doc.root();

        history
            .apply(
                &mut doc,
                Command::AddNode {
                    parent: root,
                    index: 0,
                    node: rect("r1"),
                },
            )
            .unwrap();
        assert_eq!(doc.children_of(root).unwrap().len(), 1);

        assert!(history.undo(&mut doc).unwrap());
        assert_eq!(doc.children_of(root).unwrap().len(), 0);
        assert_eq!(doc.node_count(), 1); // just root

        assert!(history.redo(&mut doc).unwrap());
        assert_eq!(doc.children_of(root).unwrap().len(), 1);
        assert_eq!(
            doc.node(doc.children_of(root).unwrap()[0]).unwrap().name,
            "r1"
        );
    }

    #[test]
    fn removing_group_restores_whole_subtree_on_undo() {
        let mut doc = Document::new(100, 100, ColorMode::Cmyk);
        let mut history = History::default();
        let root = doc.root();

        history
            .apply(
                &mut doc,
                Command::AddNode {
                    parent: root,
                    index: 0,
                    node: Box::new(Node::group("g")),
                },
            )
            .unwrap();
        let group = doc.children_of(root).unwrap()[0];
        history
            .apply(
                &mut doc,
                Command::AddNode {
                    parent: group,
                    index: 0,
                    node: rect("child"),
                },
            )
            .unwrap();
        assert_eq!(doc.node_count(), 3);

        history
            .apply(&mut doc, Command::RemoveNode { id: group })
            .unwrap();
        assert_eq!(doc.node_count(), 1);

        history.undo(&mut doc).unwrap();
        assert_eq!(doc.node_count(), 3);
        let restored_group = doc.children_of(root).unwrap()[0];
        assert_eq!(restored_group, group);
        assert_eq!(doc.children_of(group).unwrap().len(), 1);
    }

    #[test]
    fn set_opacity_is_invertible_and_new_edit_clears_redo() {
        let mut doc = Document::new(100, 100, ColorMode::Rgb);
        let mut history = History::default();
        let root = doc.root();
        history
            .apply(
                &mut doc,
                Command::AddNode {
                    parent: root,
                    index: 0,
                    node: rect("r"),
                },
            )
            .unwrap();
        let id = doc.children_of(root).unwrap()[0];

        history
            .apply(&mut doc, Command::SetOpacity { id, opacity: 0.5 })
            .unwrap();
        assert_eq!(doc.node(id).unwrap().opacity, 0.5);

        history.undo(&mut doc).unwrap();
        assert_eq!(doc.node(id).unwrap().opacity, 1.0);
        assert!(history.can_redo());

        history
            .apply(&mut doc, Command::SetVisible { id, visible: false })
            .unwrap();
        assert!(!history.can_redo());
    }

    #[test]
    fn cannot_add_under_leaf_or_remove_root() {
        let mut doc = Document::new(100, 100, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: rect("r"),
        })
        .unwrap();
        let leaf = doc.children_of(root).unwrap()[0];

        let err = doc
            .apply(Command::AddNode {
                parent: leaf,
                index: 0,
                node: rect("x"),
            })
            .unwrap_err();
        assert_eq!(err, DocError::NotAGroup(leaf));
        assert_eq!(
            doc.apply(Command::RemoveNode { id: root }).unwrap_err(),
            DocError::CannotRemoveRoot
        );
    }

    #[test]
    fn move_node_reorders_and_undoes() {
        let mut doc = Document::new(100, 100, ColorMode::Rgb);
        let mut history = History::default();
        let root = doc.root();
        for name in ["a", "b", "c"] {
            let index = doc.children_of(root).unwrap().len();
            history
                .apply(
                    &mut doc,
                    Command::AddNode {
                        parent: root,
                        index,
                        node: rect(name),
                    },
                )
                .unwrap();
        }
        let ids = doc.children_of(root).unwrap().to_vec();

        // Move bottom node "a" to the top.
        history
            .apply(
                &mut doc,
                Command::MoveNode {
                    id: ids[0],
                    parent: root,
                    index: 2,
                },
            )
            .unwrap();
        assert_eq!(doc.children_of(root).unwrap(), &[ids[1], ids[2], ids[0]]);

        history.undo(&mut doc).unwrap();
        assert_eq!(doc.children_of(root).unwrap(), &[ids[0], ids[1], ids[2]]);
    }

    #[test]
    fn move_node_rejects_own_subtree_and_reparents() {
        let mut doc = Document::new(100, 100, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(Node::group("g")),
        })
        .unwrap();
        let group = doc.children_of(root).unwrap()[0];
        doc.apply(Command::AddNode {
            parent: root,
            index: 1,
            node: rect("r"),
        })
        .unwrap();
        let r = doc.children_of(root).unwrap()[1];

        // Reparent the rect into the group.
        doc.apply(Command::MoveNode {
            id: r,
            parent: group,
            index: 0,
        })
        .unwrap();
        assert_eq!(doc.children_of(group).unwrap(), &[r]);

        // A group can't move into itself.
        assert_eq!(
            doc.apply(Command::MoveNode {
                id: group,
                parent: group,
                index: 0,
            })
            .unwrap_err(),
            DocError::MoveIntoOwnSubtree(group)
        );
    }

    #[test]
    fn set_name_is_invertible() {
        let mut doc = Document::new(10, 10, ColorMode::Rgb);
        let mut history = History::default();
        let root = doc.root();
        history
            .apply(
                &mut doc,
                Command::AddNode {
                    parent: root,
                    index: 0,
                    node: rect("old"),
                },
            )
            .unwrap();
        let id = doc.children_of(root).unwrap()[0];
        history
            .apply(
                &mut doc,
                Command::SetName {
                    id,
                    name: "new".into(),
                },
            )
            .unwrap();
        assert_eq!(doc.node(id).unwrap().name, "new");
        history.undo(&mut doc).unwrap();
        assert_eq!(doc.node(id).unwrap().name, "old");
    }

    #[test]
    fn resources_are_content_addressed_and_survive_manifest_roundtrip() {
        let mut doc = Document::new(10, 10, ColorMode::Rgb);
        let bytes = vec![7u8; 2 * 2 * 4];
        let id1 = doc.add_resource(2, 2, bytes.clone());
        let id2 = doc.add_resource(2, 2, bytes.clone());
        assert_eq!(id1, id2, "identical content shares one entry");
        assert_eq!(doc.resources().count(), 1);

        // Manifest serialization keeps dimensions but not bytes …
        let json = serde_json::to_string(&doc).unwrap();
        let mut restored: Document = serde_json::from_str(&json).unwrap();
        let res = restored.resource(&id1).unwrap();
        assert_eq!((res.width, res.height), (2, 2));
        assert!(res.rgba8.is_empty());

        // … and the container layer restores them, validating the length.
        assert!(!restored.restore_resource_bytes(&id1, vec![1u8; 3]));
        assert!(restored.restore_resource_bytes(&id1, bytes.clone()));
        assert_eq!(restored.resource(&id1).unwrap().rgba8, bytes);
    }

    #[test]
    fn document_roundtrips_through_json() {
        let mut doc = Document::new(640, 480, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: rect("r"),
        })
        .unwrap();
        let json = serde_json::to_string(&doc).unwrap();
        let restored: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.node_count(), doc.node_count());
        assert_eq!(restored.meta.width, 640);
    }
}
