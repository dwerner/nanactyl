use crate::{tree::Tree, Identity};

#[derive(Debug, Default, Clone)]
pub struct SceneNode {
    pub thing_id: Option<Identity>,
}

/// Wrapper for data held in the scene graph.
impl SceneNode {
    pub fn new() -> Self {
        Self { thing_id: None }
    }
    pub fn with_id(id: Identity) -> Self {
        Self { thing_id: Some(id) }
    }
}

#[derive(Debug)]
pub struct Scene {
    pub graph: Tree<SceneNode>,
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            graph: Tree::new_with_root(SceneNode::new()),
        }
    }
}

impl Scene {
    // TODO: size hinting at construction.
    pub fn new() -> Self {
        Scene::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_created() {
        let _ = Scene::new();
    }
}
