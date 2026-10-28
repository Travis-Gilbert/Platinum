//! The sidebar's tree of tabs: which tab each one hangs under, in what
//! order, which branches are folded and what the person named them.
//!
//! The tree is how the tabs are organized, not how they were found. A tab
//! opened from another hangs under it at first and remembers it as its
//! `opener`; moving it under another branch changes its parent and keeps the
//! opener, so the way a page was reached survives reorganizing.
//!
//! Tabs are named by the browser's ids. Pinned tabs are not in the tree.
//! Plain data with no GPUI in it, so the rules are tested on their own.

use std::collections::HashMap;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Node {
    pub parent: Option<u64>,
    pub children: Vec<u64>,
    /// The tab this one was opened from, wherever it hangs now.
    pub opener: Option<u64>,
    /// The branch is folded: its descendants are not drawn.
    pub collapsed: bool,
    /// What the person called the branch, shown instead of the page's title.
    pub name: Option<String>,
}

/// Where a dragged tab lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// Just above that tab, beside it.
    Before(u64),
    /// Just below that tab and its branch, beside it.
    After(u64),
    /// The last child of that tab.
    Into(u64),
    /// The last tab at the top level.
    End,
}

#[derive(Clone, Debug, Default)]
pub struct TabTree {
    nodes: HashMap<u64, Node>,
    roots: Vec<u64>,
}

impl TabTree {
    pub fn contains(&self, id: u64) -> bool {
        self.nodes.contains_key(&id)
    }

    pub fn node(&self, id: u64) -> Option<&Node> {
        self.nodes.get(&id)
    }

    pub fn roots(&self) -> &[u64] {
        &self.roots
    }

    pub fn children(&self, id: u64) -> &[u64] {
        self.nodes.get(&id).map_or(&[], |node| &node.children)
    }

    pub fn has_children(&self, id: u64) -> bool {
        !self.children(id).is_empty()
    }

    /// The top-level tab whose branch `id` is in: `id` itself at the top.
    pub fn root(&self, id: u64) -> u64 {
        let mut id = id;
        while let Some(parent) = self.node(id).and_then(|node| node.parent) {
            id = parent;
        }
        id
    }

    /// A new tab at the end of the top level.
    pub fn add_root(&mut self, id: u64) {
        self.insert(id, None, None);
    }

    /// A tab opened from `opener`: its last child. An opener that is not in
    /// the tree (a pinned tab, say) leaves the tab at the top level.
    pub fn add_child(&mut self, id: u64, opener: u64) {
        let parent = self.contains(opener).then_some(opener);
        self.insert(id, parent, Some(opener));
    }

    /// `id` as the last child of `parent`, or the last top-level tab. Tabs
    /// restored parent first, in order, come back as they were.
    pub fn insert(&mut self, id: u64, parent: Option<u64>, opener: Option<u64>) {
        if self.contains(id) {
            return;
        }
        let parent = parent.filter(|parent| self.contains(*parent));
        self.siblings_mut(parent).push(id);
        self.nodes.insert(
            id,
            Node {
                parent,
                opener,
                ..Node::default()
            },
        );
    }

    /// Take `id` out of the tree. Its children take its place, in order,
    /// under its parent.
    pub fn remove(&mut self, id: u64) {
        let Some(node) = self.nodes.remove(&id) else {
            return;
        };
        let siblings = self.siblings_mut(node.parent);
        if let Some(ix) = siblings.iter().position(|sibling| *sibling == id) {
            siblings.splice(ix..=ix, node.children.iter().copied());
        }
        for child in &node.children {
            if let Some(child) = self.nodes.get_mut(child) {
                child.parent = node.parent;
            }
        }
    }

    /// Take `id` and everything under it out of the tree. Returns them,
    /// `id` first, in the order they were drawn.
    pub fn remove_branch(&mut self, id: u64) -> Vec<u64> {
        let branch = self.branch(id);
        if branch.is_empty() {
            return branch;
        }
        self.detach(id);
        for id in &branch {
            self.nodes.remove(id);
        }
        branch
    }

    /// The person closed `id`. All its children move up a level into its
    /// place, in order, keeping their own branches; a closed tab never stays
    /// behind to hold its branch.
    pub fn close(&mut self, id: u64) {
        self.remove(id);
    }

    /// `id` and everything under it, depth first.
    pub fn branch(&self, id: u64) -> Vec<u64> {
        let mut out = Vec::new();
        if self.contains(id) {
            self.walk(id, &mut |id| out.push(id));
        }
        out
    }

    /// Move `id`, with its branch, to `place`. A tab cannot go inside its
    /// own branch; returns whether it moved.
    pub fn move_to(&mut self, id: u64, place: Place) -> bool {
        if !self.contains(id) {
            return false;
        }
        let target = match place {
            Place::Before(target) | Place::After(target) | Place::Into(target) => Some(target),
            Place::End => None,
        };
        if let Some(target) = target
            && (!self.contains(target) || self.is_within(target, id))
        {
            return false;
        }
        self.detach(id);
        let parent = match place {
            Place::Before(target) | Place::After(target) => self.nodes[&target].parent,
            Place::Into(target) => {
                // Unfold it, so the tab is still in sight where it went.
                if let Some(node) = self.nodes.get_mut(&target) {
                    node.collapsed = false;
                }
                Some(target)
            }
            Place::End => None,
        };
        let siblings = self.siblings_mut(parent);
        let ix = match place {
            Place::Before(target) => siblings.iter().position(|s| *s == target),
            Place::After(target) => siblings.iter().position(|s| *s == target).map(|ix| ix + 1),
            Place::Into(_) | Place::End => None,
        };
        match ix {
            Some(ix) => siblings.insert(ix, id),
            None => siblings.push(id),
        }
        if let Some(node) = self.nodes.get_mut(&id) {
            node.parent = parent;
        }
        true
    }

    pub fn set_collapsed(&mut self, id: u64, collapsed: bool) {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.collapsed = collapsed;
        }
    }

    pub fn toggle(&mut self, id: u64) {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.collapsed = !node.collapsed;
        }
    }

    /// Name the branch, or, with nothing but spaces, go back to the title.
    pub fn rename(&mut self, id: u64, name: Option<String>) {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.name = name
                .map(|name| name.trim().to_owned())
                .filter(|name| !name.is_empty());
        }
    }

    /// Unfold every branch above `id`, so its row is drawn. Returns whether
    /// anything unfolded.
    pub fn reveal(&mut self, id: u64) -> bool {
        let mut unfolded = false;
        let mut at = self.nodes.get(&id).and_then(|node| node.parent);
        while let Some(parent) = at {
            let node = self
                .nodes
                .get_mut(&parent)
                .expect("parents are in the tree");
            unfolded |= node.collapsed;
            node.collapsed = false;
            at = node.parent;
        }
        unfolded
    }

    /// Every tab, depth first: the order the rows run in when all is open.
    pub fn order(&self) -> Vec<u64> {
        let mut out = Vec::new();
        for root in &self.roots {
            self.walk(*root, &mut |id| out.push(id));
        }
        out
    }

    /// The rows drawn, every tab not inside a folded branch, with how deep
    /// each sits.
    pub fn rows(&self) -> Vec<(u64, usize)> {
        fn visit(tree: &TabTree, id: u64, depth: usize, out: &mut Vec<(u64, usize)>) {
            let node = &tree.nodes[&id];
            out.push((id, depth));
            if !node.collapsed {
                for child in &node.children {
                    visit(tree, *child, depth + 1, out);
                }
            }
        }
        let mut out = Vec::new();
        for root in &self.roots {
            visit(self, *root, 0, &mut out);
        }
        out
    }

    fn walk(&self, id: u64, f: &mut impl FnMut(u64)) {
        f(id);
        for child in &self.nodes[&id].children {
            self.walk(*child, f);
        }
    }

    /// Whether `id` is `ancestor` or somewhere in its branch.
    fn is_within(&self, id: u64, ancestor: u64) -> bool {
        let mut at = Some(id);
        while let Some(current) = at {
            if current == ancestor {
                return true;
            }
            at = self.nodes.get(&current).and_then(|node| node.parent);
        }
        false
    }

    /// Take `id` out of its parent's children, or the top level.
    fn detach(&mut self, id: u64) {
        let parent = self.nodes.get(&id).and_then(|node| node.parent);
        self.siblings_mut(parent).retain(|sibling| *sibling != id);
    }

    fn siblings_mut(&mut self, parent: Option<u64>) -> &mut Vec<u64> {
        match parent.and_then(|parent| self.nodes.get_mut(&parent)) {
            Some(node) => &mut node.children,
            None => &mut self.roots,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 1
    /// ├ 2
    /// │ └ 4
    /// └ 3
    /// 5
    fn sample() -> TabTree {
        let mut tree = TabTree::default();
        tree.add_root(1);
        tree.add_child(2, 1);
        tree.add_child(3, 1);
        tree.add_child(4, 2);
        tree.add_root(5);
        tree
    }

    #[test]
    fn tabs_hang_under_the_tab_they_were_opened_from() {
        let tree = sample();
        assert_eq!(tree.roots(), [1, 5]);
        assert_eq!(tree.children(1), [2, 3]);
        assert_eq!(tree.children(2), [4]);
        assert_eq!(tree.order(), [1, 2, 4, 3, 5]);
        assert_eq!(tree.node(4).unwrap().opener, Some(2));
        assert_eq!((tree.root(4), tree.root(1), tree.root(5)), (1, 1, 5));
    }

    #[test]
    fn an_opener_outside_the_tree_leaves_the_tab_at_the_top() {
        let mut tree = sample();
        tree.add_child(9, 77);
        assert_eq!(tree.roots(), [1, 5, 9]);
        assert_eq!(tree.node(9).unwrap().opener, Some(77));
    }

    #[test]
    fn removing_a_tab_moves_its_children_into_its_place() {
        let mut tree = sample();
        tree.remove(2);
        assert_eq!(tree.children(1), [4, 3]);
        assert_eq!(tree.node(4).unwrap().parent, Some(1));
        tree.remove(1);
        assert_eq!(tree.roots(), [4, 3, 5]);
        assert_eq!(tree.node(3).unwrap().parent, None);
    }

    #[test]
    fn removing_a_branch_takes_everything_under_it() {
        let mut tree = sample();
        assert_eq!(tree.remove_branch(1), [1, 2, 4, 3]);
        assert_eq!(tree.order(), [5]);
        assert!(!tree.contains(4));
    }

    #[test]
    fn folded_branches_hide_their_rows() {
        let mut tree = sample();
        tree.toggle(2);
        assert_eq!(tree.rows(), [(1, 0), (2, 1), (3, 1), (5, 0)]);
        assert!(tree.node(2).unwrap().collapsed && tree.has_children(2));

        tree.toggle(1);
        assert!(tree.reveal(4));
        assert_eq!(tree.rows().len(), 5);
        assert!(!tree.reveal(4), "nothing left to unfold");
    }

    #[test]
    fn moving_keeps_the_branch_and_the_opener() {
        let mut tree = sample();
        assert!(tree.move_to(2, Place::After(5)));
        assert_eq!(tree.roots(), [1, 5, 2]);
        assert_eq!(tree.children(2), [4], "the branch came along");
        let moved = tree.node(2).unwrap();
        assert_eq!((moved.parent, moved.opener), (None, Some(1)));

        assert!(tree.move_to(3, Place::Before(1)));
        assert_eq!(tree.roots(), [3, 1, 5, 2]);

        tree.toggle(5);
        assert!(tree.move_to(1, Place::Into(5)));
        assert_eq!(tree.children(5), [1]);
        assert!(!tree.node(5).unwrap().collapsed, "unfolded to show it");

        assert!(tree.move_to(4, Place::End));
        assert_eq!(tree.roots(), [3, 5, 2, 4]);
    }

    #[test]
    fn a_tab_cannot_go_inside_its_own_branch() {
        let mut tree = sample();
        assert!(!tree.move_to(1, Place::Into(4)));
        assert!(!tree.move_to(1, Place::After(2)));
        assert!(!tree.move_to(2, Place::Into(2)));
        assert_eq!(tree.order(), [1, 2, 4, 3, 5], "nothing moved");
    }

    #[test]
    fn names_are_trimmed_and_blank_ones_clear() {
        let mut tree = sample();
        tree.rename(1, Some("  Browser research ".into()));
        assert_eq!(
            tree.node(1).unwrap().name.as_deref(),
            Some("Browser research")
        );
        tree.rename(1, Some("   ".into()));
        assert_eq!(tree.node(1).unwrap().name, None);
    }

    #[test]
    fn restoring_parent_first_rebuilds_the_tree() {
        let original = sample();
        let mut restored = TabTree::default();
        for id in original.order() {
            let node = original.node(id).unwrap();
            restored.insert(id, node.parent, node.opener);
        }
        assert_eq!(restored.order(), original.order());
        assert_eq!(restored.children(1), original.children(1));
    }

    #[test]
    fn closing_a_tab_without_children_removes_it() {
        let mut tree = sample();
        tree.close(4);
        assert!(!tree.contains(4));
        assert_eq!(tree.children(2), [] as [u64; 0]);
        tree.close(4);
        assert_eq!(
            tree.order(),
            [1, 2, 3, 5],
            "closing it again changes nothing"
        );
    }

    #[test]
    fn closing_a_tab_moves_all_its_children_up() {
        let mut tree = sample();
        tree.toggle(2);
        tree.close(1);
        assert_eq!(tree.roots(), [2, 3, 5]);
        assert_eq!(tree.children(2), [4], "a child keeps its own branch");
        assert!(tree.node(2).unwrap().collapsed, "and its fold");
        let promoted = tree.node(3).unwrap();
        assert_eq!(
            (promoted.parent, promoted.opener),
            (None, Some(1)),
            "it hangs at the top now but still came from the closed tab"
        );
    }
}
