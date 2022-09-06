use std::fmt::Debug;

pub type NodeIndex = u32;

#[derive(thiserror::Error, Debug)]
pub enum TreeError {
    #[error("Parent node {0} doesn't exist in the tree.")]
    ParentDoesntExist(NodeIndex),
}

#[derive(Debug)]
pub struct Tree<V>
where
    V: Clone + Debug,
{
    nodes: Vec<Node<V>>,
}

impl<V> Tree<V>
where
    V: Clone + Debug,
{
    pub fn new_with_root(root: V) -> Self {
        Self {
            nodes: vec![Node::new_root(root)],
        }
    }

    pub fn root(&self) -> &Node<V> {
        &self.nodes[0]
    }

    pub fn root_mut(&mut self) -> &mut Node<V> {
        &mut self.nodes[0]
    }

    pub fn node(&self, index: NodeIndex) -> Option<&Node<V>> {
        self.nodes.get(index as usize)
    }

    pub fn node_mut(&mut self, index: NodeIndex) -> Option<&mut Node<V>> {
        self.nodes.get_mut(index as usize)
    }

    fn new_child_node(&mut self, parent_idx: NodeIndex, value: V) -> Result<NodeIndex, TreeError> {
        let child_index = self.nodes.len();
        let parent = self
            .node_mut(parent_idx)
            .ok_or(TreeError::ParentDoesntExist(parent_idx))?;
        let child_index = child_index as NodeIndex;
        parent.children.push(child_index);
        self.nodes.push(Node::new(value, Some(parent_idx)));
        Ok(child_index)
    }

    pub fn new_child_of(
        &mut self,
        parent_idx: NodeIndex,
        value: V,
    ) -> Result<NodeIndex, TreeError> {
        self.new_child_node(parent_idx, value)
    }

    /// Iterate all nodes in the tree, regardless of order. In practice, it
    /// will iterate through the backing Vec in insertion order.
    pub fn all_nodes_iter(&self) -> impl Iterator<Item = &Node<V>> {
        self.nodes.iter()
    }

    /// Iterate over nodes in a depth-first manner.
    pub fn depth_first_iter<'a>(&'a self) -> DepthFirstIterator<'a, V> {
        DepthFirstIterator::new(self, 0)
    }
}

#[derive(Debug, Clone)]
pub struct Node<V: Clone + Debug> {
    pub value: V,
    children: Vec<NodeIndex>,
    parent: Option<NodeIndex>,
}

impl<V> Node<V>
where
    V: Clone + Debug,
{
    fn new(value: V, parent: Option<NodeIndex>) -> Self {
        Self {
            value,
            children: vec![],
            parent,
        }
    }

    fn new_root(value: V) -> Self {
        Self::new(value, None)
    }

    pub fn is_root(&self) -> bool {
        self.parent.is_none()
    }

    pub fn is_leaf(&self) -> bool {
        self.children.is_empty()
    }
}

pub struct DepthFirstIterator<'a, V: Clone + Debug> {
    tree: &'a Tree<V>,
    stack: Vec<NodeIndex>,
}

impl<'a, V> Iterator for DepthFirstIterator<'a, V>
where
    V: Clone + Debug,
{
    type Item = (NodeIndex, &'a Node<V>);

    fn next(&mut self) -> Option<Self::Item> {
        let next_idx = self.stack.pop()?;
        let node = self.tree.node(next_idx)?;
        self.stack.extend(node.children.clone());
        Some((next_idx, node))
    }
}

impl<'a, V> DepthFirstIterator<'a, V>
where
    V: Clone + Debug,
{
    /// Create a new depth-first iterator over `tree` starting at the node with the index `starting_index`.
    pub fn new(tree: &'a Tree<V>, starting_index: NodeIndex) -> Self {
        DepthFirstIterator {
            tree,
            stack: vec![starting_index],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_children() {
        let mut tree = Tree::new_with_root(1);
        let _root = tree.root_mut();
        let child_index = tree.new_child_of(0, 2).unwrap();
        let _second_child = tree.new_child_of(child_index, 3).unwrap();

        assert_eq!(tree.nodes.len(), 3);
        assert!(tree.nodes[0].is_root());
    }

    #[test]
    fn iterates_depth_first() {
        let mut tree = Tree::new_with_root(0);
        let mut children = vec![(0, 0)];
        for _ in 0..5 {
            let mut deeper_child = 0;
            for d in 0..5 {
                deeper_child = tree.new_child_of(deeper_child, d).unwrap();
                children.push((deeper_child, d));
            }
        }

        let mut depth_first = vec![];
        for (index, node) in tree.depth_first_iter() {
            println!("{:?}", node);
            depth_first.push((index, node.value));
        }

        assert_eq!(tree.nodes.len(), 26);
        assert!(tree.nodes[0].is_root());
        println!("{:?}", depth_first);
        let mut depth_first: Vec<_> = depth_first.into_iter().rev().collect();
        for expected in [
            (0, 0),
            (21, 0),
            (22, 1),
            (23, 2),
            (24, 3),
            (25, 4),
            (16, 0),
            (17, 1),
            (18, 2),
            (19, 3),
            (20, 4),
            (11, 0),
            (12, 1),
            (13, 2),
            (14, 3),
            (15, 4),
            (6, 0),
            (7, 1),
            (8, 2),
            (9, 3),
            (10, 4),
            (1, 0),
            (2, 1),
            (3, 2),
            (4, 3),
            (5, 4),
        ]
        .iter()
        {
            assert_eq!(
                depth_first.pop(),
                Some(*expected),
                "depth first iterator produced unexpected ordering"
            );
        }
    }

    #[test]
    fn cloned() {
        let mut tree = Tree::new_with_root(0);
        for _ in 0..5 {
            let idx = tree.new_child_of(0, 5).unwrap();
            for x in 0..2 {
                let one = tree.new_child_of(idx, x).unwrap();
                let two = tree.new_child_of(one, x).unwrap();
                tree.new_child_of(two, x).unwrap();
            }
        }

        let iter_at_1 = DepthFirstIterator::new(&mut tree, 1)
            .map(|(idx, _node)| idx)
            .collect::<Vec<NodeIndex>>();

        assert!(!iter_at_1.contains(&0));

        // starting node
        assert!(iter_at_1.contains(&1));

        // Iterator should only pass over child nodes of node 1
        for i in 2..=7 {
            assert!(iter_at_1.contains(&i), "{}", i);
        }

        for i in 8..=26 {
            assert!(!iter_at_1.contains(&i), "{}", i);
        }
    }
}
