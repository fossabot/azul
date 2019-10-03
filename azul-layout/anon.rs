//! Module that handles the construction on `AnonDom`, a `Dom` that holds
//! "anonymous" nodes (that group two following "inline" texts into one "anonymous" block).

use std::collections::BTreeMap;
use crate::{
    RectContent,
    style::{Style, Display},
};
use azul_core::{
    id_tree::{NodeDataContainer, NodeHierarchy, NodeId, NodeDepths, Node},
    traits::GetTextLayout,
};

pub(crate) type OriginalNodeId = NodeId;
pub(crate) type AnonNodeId = NodeId;

/// Same as the original DOM, but with anonymous nodes added to the original nodes.
///
/// Each box must contain only block children, or only inline children. When an DOM element
/// contains a mix of block and inline children, the layout engine inserts anonymous boxes to
/// separate the two types. (These boxes are "anonymous" because they aren't associated with
/// nodes in the DOM tree.)
#[derive(Debug, Clone)]
pub(crate) struct AnonDom {
    pub(crate) anon_node_hierarchy: NodeHierarchy,
    pub(crate) anon_node_styles: NodeDataContainer<AnonNode>,
    pub(crate) original_node_id_mapping: BTreeMap<OriginalNodeId, AnonNodeId>,
}

#[derive(Debug, Copy, Clone)]
pub(crate) enum AnonNode {
    /// Node that doesn't have a correspondent in the DOM tree,
    /// but still behaves like display: block. Is always a parent of one or
    /// more display:inline items
    AnonStyle,
    /// Non-inline DOM node (block / flex, etc.)
    BlockNode(Style),
    /// Inline node or text. Note that the style.display may still be "block",
    /// on text nodes the "display" property is ignored, since texts are always
    /// laid out as inline items.
    InlineNode(Style),
}

impl AnonNode {
    pub(crate) fn is_inline(&self) -> bool {
        use self::AnonNode::*;
        match self {
            AnonStyle | BlockNode(_) => false,
            InlineNode(_) => true,
        }
    }
}

impl AnonDom {

    pub(crate) fn new<T: GetTextLayout>(
        node_hierarchy: &NodeHierarchy,
        node_styles: &NodeDataContainer<Style>,
        node_depths: &NodeDepths,
        rect_contents: &BTreeMap<NodeId, RectContent<T>>,
    ) -> Self {

        use self::AnonNode::*;

        // Worst case scenario is that every node needs an anonymous block.
        // Pre-allocate 2x the nodes to avoid recursion
        let mut new_nodes = vec![AnonNode::AnonStyle; node_hierarchy.len() * 2];
        let mut new_node_hierarchy = vec![Node::ROOT; node_hierarchy.len() * 2];
        let mut original_node_id_mapping = BTreeMap::new();
        original_node_id_mapping.insert(NodeId::ZERO, NodeId::ZERO);

        let mut num_anon_nodes = 0;

        // Count how many anonymous nodes need to be inserted in order
        // to correct the "next sibling" count
        let anon_nodes_count = count_all_anon_nodes(node_hierarchy, node_styles, node_depths, rect_contents);

        for (_depth, parent_id) in node_depths {

            let children_ids = parent_id.children(node_hierarchy).collect::<Vec<NodeId>>();
            let children_count = children_ids.len();

            let num_inline_children = children_ids.iter().map(|child_id| is_inline_node(&node_styles[*child_id], &rect_contents, child_id)).count();
            let num_block_children = children_count - num_inline_children;
            let all_children_are_inline = num_block_children == 0;
            let all_children_are_block = num_inline_children == 0;

            // Add the node data of the parent to the DOM
            let parent_node_style = &node_styles[*parent_id];
            let old_parent_node = node_hierarchy[*parent_id];
            let parent_is_inline_node = is_inline_node(&parent_node_style, &rect_contents, parent_id);

            original_node_id_mapping.insert(*parent_id, *parent_id + num_anon_nodes);

            new_nodes[(*parent_id + num_anon_nodes).index()] =
                if parent_is_inline_node { InlineNode(*parent_node_style) } else { BlockNode(*parent_node_style) };

            let anon_node_count_all_children = anon_nodes_count.get(parent_id).cloned().unwrap_or(0);

            new_node_hierarchy[(*parent_id + num_anon_nodes).index()] = Node {
                parent: old_parent_node.parent.as_ref().and_then(|p| original_node_id_mapping.get(p).copied()),
                previous_sibling: old_parent_node.previous_sibling.as_ref().and_then(|s| original_node_id_mapping.get(s).copied()),
                next_sibling: old_parent_node.next_sibling.map(|n| n + num_anon_nodes + anon_node_count_all_children),
                first_child: old_parent_node.first_child.map(|n| n + num_anon_nodes),
                last_child: old_parent_node.last_child.map(|n| n + num_anon_nodes + anon_node_count_all_children),
            };

            if all_children_are_inline || all_children_are_block {

                for child_id in children_ids.iter() {

                    let child_node_style = &node_styles[*child_id];
                    let old_child_node = node_hierarchy[*child_id];
                    let child_node_count_all_children = anon_nodes_count.get(child_id).copied().unwrap_or(0);

                    original_node_id_mapping.insert(*child_id, *child_id + num_anon_nodes);

                    new_nodes[(*child_id + num_anon_nodes).index()] =
                        if all_children_are_block { BlockNode(*child_node_style) } else { InlineNode(*child_node_style) };

                    new_node_hierarchy[(*child_id + num_anon_nodes).index()] = Node {
                        parent: old_child_node.parent.as_ref().and_then(|p| original_node_id_mapping.get(p).copied()),
                        previous_sibling: old_child_node.previous_sibling.as_ref().and_then(|s| original_node_id_mapping.get(s).copied()),
                        next_sibling: old_child_node.next_sibling.map(|n| n + num_anon_nodes + child_node_count_all_children),
                        first_child: old_child_node.first_child.map(|n| n + num_anon_nodes),
                        last_child: old_child_node.last_child.map(|n| n + num_anon_nodes + child_node_count_all_children),
                    };
                }

            } else {

                // Mixed inline / block content: Need to insert anonymous nodes +
                // fix their parent / child relationships

                if children_count == 0 {
                    continue;
                }

                let mut current_child_is_inline_node = {
                    let first_child_id = &children_ids[0];
                    is_inline_node(&node_styles[*first_child_id], &rect_contents, first_child_id)
                };

                macro_rules! insert_anonymous_block {($id:expr) => ({
                    let old_node = node_hierarchy[*$id];
                    let node_count_all_children = anon_nodes_count.get($id).copied().unwrap_or(0);
                    new_node_hierarchy[(*$id + num_anon_nodes).index()] = Node {
                        parent: old_node.parent.as_ref().and_then(|p| original_node_id_mapping.get(p).copied()),
                        previous_sibling: old_node.previous_sibling.as_ref().and_then(|s| original_node_id_mapping.get(s).copied()),
                        next_sibling: old_parent_node.next_sibling.map(|n| n + num_anon_nodes + node_count_all_children),
                        first_child: old_parent_node.first_child.map(|n| n + num_anon_nodes),
                        last_child: old_parent_node.last_child.map(|n| n + num_anon_nodes + node_count_all_children),
                    };
                    num_anon_nodes += 1;
                })}

                if current_child_is_inline_node {
                    insert_anonymous_block!(&children_ids[0]);
                }

                // Mixed content: How many anonymous nodes are needed?
                for child_id in children_ids.iter() {

                    let child_node_style = node_styles[*child_id];

                    let child_is_inline_node = is_inline_node(&child_node_style, rect_contents, child_id);

                    // inline content follows a block
                    if child_is_inline_node && !current_child_is_inline_node {
                        insert_anonymous_block!(child_id);
                    }

                    original_node_id_mapping.insert(*child_id, *child_id + num_anon_nodes);

                    new_nodes[(*child_id + num_anon_nodes).index()] =
                        if all_children_are_block { BlockNode(child_node_style) } else { InlineNode(child_node_style) };

                    current_child_is_inline_node = child_is_inline_node;
                }
            }
        }

        let total_nodes = node_hierarchy.len() + num_anon_nodes;
        new_nodes.truncate(total_nodes);
        new_node_hierarchy.truncate(total_nodes);

        Self {
            anon_node_hierarchy: NodeHierarchy::new(new_node_hierarchy),
            anon_node_styles: NodeDataContainer::new(new_nodes),
            original_node_id_mapping,
        }
    }
}

// For each parent node, holds the amount of anonymous children nodes
fn count_all_anon_nodes<T: GetTextLayout>(
    node_hierarchy: &NodeHierarchy,
    node_styles: &NodeDataContainer<Style>,
    node_depths: &NodeDepths,
    rect_contents: &BTreeMap<NodeId, RectContent<T>>,
) -> BTreeMap<NodeId, usize> {

    let mut anon_nodes_by_depth = BTreeMap::new();
    let mut sum_anon_nodes = BTreeMap::new();

    let max_depth_level = match node_depths.last() {
        Some((s, _)) => *s,
        None => return anon_nodes_by_depth,
    };

    for (depth, parent_id) in node_depths.iter().rev() {

        let anon_nodes_direct_children = count_anon_nodes_direct_children(parent_id, node_hierarchy, node_styles, rect_contents);

        let current_node_all_anon_children = if *depth == max_depth_level {
            anon_nodes_direct_children
        } else {
            anon_nodes_direct_children +
                ((depth + 1)..max_depth_level)
                .map(|d| sum_anon_nodes.get(&d).copied().unwrap_or(0))
                .sum::<usize>()
        };

        anon_nodes_by_depth.insert(*parent_id, current_node_all_anon_children);
        *sum_anon_nodes.entry(depth).or_insert(0) += anon_nodes_direct_children;
    }

    anon_nodes_by_depth
}

fn count_anon_nodes_direct_children<T: GetTextLayout>(
    node_id: &NodeId,
    node_hierarchy: &NodeHierarchy,
    node_styles: &NodeDataContainer<Style>,
    rect_contents: &BTreeMap<NodeId, RectContent<T>>,
) -> usize {

    let children_ids = node_id.children(node_hierarchy).collect::<Vec<NodeId>>();
    let num_inline_children = children_ids.iter().map(|child_id| is_inline_node(&node_styles[*child_id], &rect_contents, child_id)).count();
    let num_block_children = children_ids.len() - num_inline_children;
    let all_children_are_inline = num_block_children == 0;
    let all_children_are_block = num_inline_children == 0;

    let mut anon_node_count = 0;

    if all_children_are_block || all_children_are_inline {
        // If all children are blocks or inlines, there are no anon blocks necessary
        return anon_node_count;
    }

    let first_child_id = match &node_hierarchy[*node_id].first_child {
        None => return anon_node_count,
        Some(s) => s,
    };

    let mut last_child_is_inline_node = is_inline_node(&node_styles[*first_child_id], rect_contents, first_child_id);

    if last_child_is_inline_node {
        anon_node_count += 1
    };

    for child_id in children_ids.iter() {
        let current_child_is_inline_node = is_inline_node(&node_styles[*child_id], &rect_contents, child_id);
        if !current_child_is_inline_node {
            last_child_is_inline_node = false;
        } else if current_child_is_inline_node && !last_child_is_inline_node {
            anon_node_count += 1;
        }
    }

    anon_node_count
}

fn is_inline_node<T: GetTextLayout>(s: &Style, rect_contents: &BTreeMap<NodeId, RectContent<T>>, node_id: &NodeId) -> bool {
    s.display == Display::Inline ||
    // Is the item a text line? Texts are always laid out as display: inline, no matter what
    rect_contents.get(node_id).map(|c| c.is_text()) == Some(true)
}

#[test]
fn test_anon_dom() {

    use azul_core::{
        dom::Dom,
        ui_state::UiState,
        ui_description::UiDescription,
        ui_solver::{ResolvedTextLayoutOptions, InlineTextLayout, InlineTextLine},
        display_list::DisplayList,
    };
    use azul_css::{
        Css, Stylesheet, CssRuleBlock, CssPath, CssDeclaration,
        CssPathSelector, CssProperty, LayoutDisplay,
        LayoutRect, LayoutSize, LayoutPoint,
    };
    use crate::GetStyle;

    struct Mock;

    struct FakeTextMetricsProvider { }

    impl GetTextLayout for FakeTextMetricsProvider {
        // Fake text metrict provider that just returns a 10x10 rect for every text
        fn get_text_layout(&mut self, _: &ResolvedTextLayoutOptions) -> InlineTextLayout {
            InlineTextLayout::new(vec![
                InlineTextLine::new(LayoutRect::new(LayoutPoint::zero(), LayoutSize::new(10.0, 10.0)), 0, 0)
            ])
        }
    }

    let dom: Dom<Mock> = Dom::body()
        .with_child(Dom::label("first").with_class("inline"))
        .with_child(Dom::label("second").with_class("inline"))
        .with_child(Dom::div().with_id("third"));

    let css = Css::new(vec![Stylesheet::new(vec![
        CssRuleBlock::new(
            CssPath::new(vec![CssPathSelector::Class("inline".to_string())]),
            vec![
                CssDeclaration::new_static(CssProperty::display(LayoutDisplay::Inline)),
            ]
        )
    ])]);

    let mut ui_state = UiState::new(dom, None);
    let ui_description = UiDescription::new(&mut ui_state, &css, &None, &BTreeMap::new(), false);
    let display_list = DisplayList::new(&ui_description, &ui_state);
    let node_styles = display_list.rectangles.transform(|t, _| t.get_style());

    let mut rect_contents = BTreeMap::new();
    rect_contents.insert(NodeId::new(1), RectContent::Text(FakeTextMetricsProvider { }));
    rect_contents.insert(NodeId::new(2), RectContent::Text(FakeTextMetricsProvider { }));

    let anon_dom = AnonDom::new(
        &ui_state.get_dom().arena.node_hierarchy,
        &node_styles,
        &ui_state.get_dom().arena.node_hierarchy.get_parents_sorted_by_depth(),
        &rect_contents,
    );

    assert_eq!(anon_dom.anon_node_hierarchy, NodeHierarchy::new(
        vec![
            // Node 0: root node (body):
            Node {
                parent: None,
                previous_sibling: None,
                next_sibling: None,
                first_child: None,
                last_child: None,
            },
            //      Node 1 (anonymous node, parent of the two inline texts):
            Node {
                parent: None,
                previous_sibling: None,
                next_sibling: None,
                first_child: None,
                last_child: None,
            },
            //          Node 2 (inline text "first"):
            Node {
                parent: None,
                previous_sibling: None,
                next_sibling: None,
                first_child: None,
                last_child: None,
            },
            //          Node 3 (inline text "second"):
            Node {
                parent: None,
                previous_sibling: None,
                next_sibling: None,
                first_child: None,
                last_child: None,
            },
            //      Node 4 (div block with id "third"):
            Node {
                parent: None,
                previous_sibling: None,
                next_sibling: None,
                first_child: None,
                last_child: None,
            },
        ]
    ));

    // anon_node_hierarchy: NodeHierarchy,
    // anon_node_styles: NodeDataContainer<AnonNode>,
    // original_node_id_mapping: BTreeMap<OriginalNodeId, AnonNodeId>,
}