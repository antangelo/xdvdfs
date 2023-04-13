use core::cmp::Ordering;

use alloc::{boxed::Box, vec::Vec};

#[repr(i8)]
#[derive(Copy, Clone)]
enum AvlDirection {
    Left = -1,
    Right = 1,
    Leaf = 0,
}

#[derive(Debug, Clone)]
pub struct AvlNode<T: Ord + Clone> {
    left_node: Option<usize>,
    right_node: Option<usize>,
    parent: Option<usize>,
    height: i32,
    data: Box<T>,
}

impl<T: Ord + Clone> AvlNode<T> {
    fn new(data: T, parent: Option<usize>) -> AvlNode<T> {
        AvlNode {
            left_node: None,
            right_node: None,
            parent,
            height: 1,
            data: Box::from(data),
        }
    }

    fn child_from_direction(&self, dir: AvlDirection) -> Option<usize> {
        match dir {
            AvlDirection::Left => self.left_node,
            AvlDirection::Right => self.right_node,
            AvlDirection::Leaf => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AvlTree<T: Ord + Clone> {
    root: Option<usize>,
    tree: Vec<AvlNode<T>>,
}

impl<T: Ord + Clone> AvlTree<T> {
    pub fn new() -> Self {
        Self {
            root: None,
            tree: Vec::new(),
        }
    }

    #[cfg(test)]
    fn height_slow(&self, node: usize, memo: &mut [Option<i32>]) -> i32 {
        if let Some(height) = memo[node] {
            return height;
        }

        let left_height = match self.tree[node].left_node {
            Some(idx) => self.height_slow(idx, memo),
            None => 0,
        };

        let right_height = match self.tree[node].right_node {
            Some(idx) => self.height_slow(idx, memo),
            None => 0,
        };

        let height = 1 + core::cmp::max(left_height, right_height);
        memo[node] = Some(height);
        return height;
    }

    #[cfg(test)]
    fn validate_tree(&self) {
        struct NodeEntry {
            node: usize,
            prev: Option<usize>,
        }

        let mut heights: Vec<Option<i32>> = Vec::new();
        heights.resize(self.tree.len(), None);

        let mut stack: Vec<NodeEntry> = Vec::new();
        stack.push(NodeEntry {
            node: self.root.unwrap(),
            prev: None,
        });

        while let Some(entry) = stack.pop() {
            let node = &self.tree[entry.node];
            assert_eq!(node.parent, entry.prev);
            assert_eq!(node.height, self.height_slow(entry.node, &mut heights));

            let bf = self.balance_factor(entry.node);
            assert!(bf <= 1);
            assert!(bf >= -1);

            if let Some(left) = node.left_node {
                assert_eq!(Ord::cmp(&self.tree[left].data, &node.data), Ordering::Less);
                stack.push(NodeEntry { node: left, prev: Some(entry.node) });
            }

            if let Some(right) = node.right_node {
                assert_eq!(Ord::cmp(&self.tree[right].data, &node.data), Ordering::Greater);
                stack.push(NodeEntry { node: right, prev: Some(entry.node) });
            }
        }
    }
    
    fn allocate(&mut self, data: T, parent: Option<usize>) -> usize {
        self.tree.push(AvlNode::new(data, parent));
        self.tree.len() - 1
    }

    fn parent_direction(&self, node_idx: usize) -> Option<(usize, AvlDirection)> {
        let node = &self.tree[node_idx];
        let parent = node.parent?;
        let parent_node = &self.tree[parent];

        if let Some(idx) = parent_node.left_node {
            if idx == node_idx {
                return Some((parent, AvlDirection::Left));
            }
        }

        if let Some(idx) = parent_node.right_node {
            if idx == node_idx {
                return Some((parent, AvlDirection::Right));
            }
        }

        unreachable!("AVL node-parent invariant violated");
    }

    fn parent_node_ref(&mut self, idx: usize) -> &mut Option<usize> {
        let parent_dir = self.parent_direction(idx);
        let (parent_idx, dir) = match parent_dir {
            Some(val) => val,
            None => return &mut self.root,
        };

        let parent = &mut self.tree[parent_idx];
        let node = match dir {
            AvlDirection::Left => &mut parent.left_node,
            AvlDirection::Right => &mut parent.right_node,
            AvlDirection::Leaf => unreachable!(),
        };

        node
    }

    fn update_node_height(&mut self, node_idx: usize) {
        let node = &self.tree[node_idx];
        let left_height = match node.left_node {
            Some(idx) => self.tree[idx].height,
            None => 0,
        };

        let right_height = match node.right_node {
            Some(idx) => self.tree[idx].height,
            None => 0,
        };

        self.tree[node_idx].height = 1 + core::cmp::max(left_height, right_height);
    }

    fn balance_factor(&self, node_idx: usize) -> i32 {
        let node = &self.tree[node_idx];
        let left_height = match node.left_node {
            Some(idx) => self.tree[idx].height,
            None => 0,
        };

        let right_height = match node.right_node {
            Some(idx) => self.tree[idx].height,
            None => 0,
        };

        left_height - right_height
    }

    fn set_left_child(&mut self, node: usize, to: Option<usize>) {
        self.tree[node].left_node = to;
        if let Some(idx) = to {
            self.tree[idx].parent = Some(node);
        }
    }

    fn set_right_child(&mut self, node: usize, to: Option<usize>) {
        self.tree[node].right_node = to;
        if let Some(idx) = to {
            self.tree[idx].parent = Some(node);
        }
    }

    fn l_rotate(&mut self, a: usize, b: usize, c: usize) {
        assert_eq!(self.tree[a].right_node, Some(b));
        assert_eq!(self.tree[b].right_node, Some(c));

        let a_parent = self.tree[a].parent;
        *self.parent_node_ref(a) = Some(b);
        self.tree[b].parent = a_parent;

        let b_left = self.tree[b].left_node;
        self.set_right_child(a, b_left);
        self.set_left_child(b, Some(a));

        self.update_node_height(a);
        self.update_node_height(c);
        self.update_node_height(b);
    }

    fn r_rotate(&mut self, a: usize, b: usize, c: usize) {
        assert_eq!(self.tree[a].left_node, Some(b));
        assert_eq!(self.tree[b].left_node, Some(c));

        let a_parent = self.tree[a].parent;
        *self.parent_node_ref(a) = Some(b);
        self.tree[b].parent = a_parent;

        let b_right = self.tree[b].right_node;
        self.set_left_child(a, b_right);
        self.set_right_child(b, Some(a));

        self.update_node_height(a);
        self.update_node_height(c);
        self.update_node_height(b);
    }

    fn rl_rotate(&mut self, a: usize, b: usize, c: usize) {
        assert_eq!(self.tree[a].right_node, Some(b));
        assert_eq!(self.tree[b].left_node, Some(c));

        let a_parent = self.tree[a].parent;
        *self.parent_node_ref(a) = Some(c);
        self.tree[c].parent = a_parent;

        let c_left = self.tree[c].left_node;
        self.set_right_child(a, c_left);

        let c_right = self.tree[c].right_node;
        self.set_left_child(b, c_right);

        self.set_right_child(c, Some(b));
        self.set_left_child(c, Some(a));

        self.update_node_height(a);
        self.update_node_height(b);
        self.update_node_height(c);
    }

    fn lr_rotate(&mut self, a: usize, b: usize, c: usize) {
        assert_eq!(self.tree[a].left_node, Some(b));
        assert_eq!(self.tree[b].right_node, Some(c));

        let a_parent = self.tree[a].parent;
        *self.parent_node_ref(a) = Some(c);
        self.tree[c].parent = a_parent;

        let c_left = self.tree[c].left_node;
        self.set_right_child(b, c_left);

        let c_right = self.tree[c].right_node;
        self.set_left_child(a, c_right);

        self.set_right_child(c, Some(a));
        self.set_left_child(c, Some(b));

        self.update_node_height(a);
        self.update_node_height(b);
        self.update_node_height(c);
    }

    fn perform_rotation(
        &mut self,
        a: usize,
        b: usize,
        c: usize,
        ab: AvlDirection,
        bc: AvlDirection,
    ) {
        match ab {
            AvlDirection::Left => match bc {
                AvlDirection::Left => self.r_rotate(a, b, c),
                AvlDirection::Right => self.lr_rotate(a, b, c),
                AvlDirection::Leaf => unreachable!("AVL Rotate involves leaf child"),
            },
            AvlDirection::Right => match bc {
                AvlDirection::Left => self.rl_rotate(a, b, c),
                AvlDirection::Right => self.l_rotate(a, b, c),
                AvlDirection::Leaf => unreachable!("AVL Rotate involves leaf child"),
            },
            AvlDirection::Leaf => unreachable!("AVL Rotate involves leaf child"),
        }
    }

    fn rebalance(&mut self, leaf: usize) {
        assert!(self
            .tree
            .get(leaf)
            .as_ref()
            .filter(|node| node.left_node.is_none() && node.right_node.is_none())
            .is_some());

        let mut current_idx = Some((leaf, AvlDirection::Leaf));
        let mut prev_idx: Option<(usize, AvlDirection)> = None;
        while let Some((idx, dir)) = current_idx {
            self.update_node_height(idx);
            let balance_factor = self.balance_factor(idx);

            if balance_factor > 1 || balance_factor < -1 {
                let (node_b, prev_dir) = prev_idx.unwrap();
                let node_c = self.tree[node_b].child_from_direction(prev_dir).unwrap();
                self.perform_rotation(idx, node_b, node_c, dir, prev_dir);
            }

            prev_idx = current_idx;
            current_idx = self.parent_direction(idx);
        }
    }

    pub fn insert(&mut self, data: &T) {
        let new_element_index = self.allocate(data.clone(), None);

        let mut current_node = match self.root {
            Some(idx) => idx,
            None => {
                self.root = Some(new_element_index);

                return;
            }
        };

        let mut prev_node;
        loop {
            prev_node = Some(current_node);
            let node = &mut self.tree[current_node];
            let cmp = Ord::cmp(data, &node.data);
            let next_node = match cmp {
                Ordering::Less => &mut node.left_node,
                Ordering::Greater => &mut node.right_node,
                Ordering::Equal => panic!("AVL duplicate node inserted"),
            };

            match next_node {
                Some(idx) => current_node = *idx,
                None => {
                    *next_node = Some(new_element_index);
                    break;
                }
            }
        }

        self.tree[new_element_index].parent = prev_node;
        self.rebalance(new_element_index);
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use rand::{rngs, SeedableRng, prelude::*};

    #[test]
    fn test_insert_invariants() {
        let mut rng = rngs::StdRng::seed_from_u64(0x5842_4f58_5842_4f58);
        let mut test_set: Vec<i32> = Vec::new();
        test_set.resize_with(1000, || rng.gen());

        let mut tree = AvlTree::new();

        for i in test_set {
            tree.insert(&i);
            tree.validate_tree();
        }
    }
}
