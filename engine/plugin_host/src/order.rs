use std::collections::BTreeSet;

use namespaced_id::NamespacedId;

/// One thing to put in order.
pub(crate) struct Node {
    /// Ties are broken by this: the plugin's id, then the thing's own.
    pub key: (NamespacedId, NamespacedId),
    /// The nodes (by position) it must come after.
    pub after: Vec<usize>,
}

/// A topological order of the nodes in which, among those that may go next, the one with the least key
/// does. Never depends on how the nodes were listed. A cycle gives the names of what is in it.
pub(crate) fn order(nodes: &[Node]) -> Result<Vec<usize>, Vec<NamespacedId>> {
    let mut waiting: Vec<usize> = nodes.iter().map(|node| node.after.len()).collect();
    let mut later: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    for (index, node) in nodes.iter().enumerate() {
        for &before in &node.after {
            later[before].push(index);
        }
    }
    let mut ready: BTreeSet<(&(NamespacedId, NamespacedId), usize)> = nodes
        .iter()
        .enumerate()
        .filter(|(index, _)| waiting[*index] == 0)
        .map(|(index, node)| (&node.key, index))
        .collect();
    let mut result = Vec::with_capacity(nodes.len());
    while let Some(&first) = ready.iter().next() {
        ready.remove(&first);
        let index = first.1;
        result.push(index);
        for &next in &later[index] {
            waiting[next] -= 1;
            if waiting[next] == 0 {
                ready.insert((&nodes[next].key, next));
            }
        }
    }
    if result.len() == nodes.len() {
        Ok(result)
    } else {
        Err(nodes
            .iter()
            .enumerate()
            .filter(|(index, _)| waiting[*index] > 0)
            .map(|(_, node)| node.key.1.clone())
            .collect())
    }
}

/// For each node, which nodes come (directly or not) after it.
pub(crate) fn reachable(nodes: &[Node]) -> Vec<Vec<bool>> {
    let count = nodes.len();
    let mut later: Vec<Vec<usize>> = vec![Vec::new(); count];
    for (index, node) in nodes.iter().enumerate() {
        for &before in &node.after {
            later[before].push(index);
        }
    }
    let mut reach = vec![vec![false; count]; count];
    for start in 0..count {
        let mut stack = later[start].clone();
        while let Some(next) = stack.pop() {
            if !reach[start][next] {
                reach[start][next] = true;
                stack.extend(later[next].iter().copied());
            }
        }
    }
    reach
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(plugin: &str, name: &str) -> (NamespacedId, NamespacedId) {
        (
            format!("test:{plugin}").parse().unwrap(),
            format!("test:{name}").parse().unwrap(),
        )
    }

    fn node(plugin: &str, name: &str, after: Vec<usize>) -> Node {
        Node {
            key: key(plugin, name),
            after,
        }
    }

    #[test]
    fn nodes_with_no_edges_go_in_the_order_of_their_keys_however_they_are_listed() {
        let listed = |names: &[&str]| -> Vec<String> {
            let nodes: Vec<Node> = names
                .iter()
                .map(|name| node("plugin", name, Vec::new()))
                .collect();
            order(&nodes)
                .unwrap()
                .into_iter()
                .map(|index| names[index].to_owned())
                .collect()
        };
        assert_eq!(listed(&["c", "a", "b"]), ["a", "b", "c"]);
        assert_eq!(listed(&["b", "c", "a"]), ["a", "b", "c"]);
        assert_eq!(listed(&["a", "b", "c"]), ["a", "b", "c"]);
    }

    #[test]
    fn an_edge_beats_the_keys_and_the_least_key_still_goes_first_among_the_rest() {
        // b must come after c; a is free.
        let nodes = vec![
            node("p", "a", vec![]),
            node("p", "b", vec![2]),
            node("p", "c", vec![]),
        ];
        assert_eq!(order(&nodes), Ok(vec![0, 2, 1]));
    }

    #[test]
    fn the_plugin_comes_before_the_name_in_the_key() {
        let nodes = vec![node("z", "a", vec![]), node("a", "z", vec![])];
        assert_eq!(order(&nodes), Ok(vec![1, 0]));
    }

    #[test]
    fn a_cycle_is_reported_with_what_is_in_it() {
        let nodes = vec![
            node("p", "a", vec![1]),
            node("p", "b", vec![0]),
            node("p", "free", vec![]),
        ];
        let cycle = order(&nodes).unwrap_err();
        assert_eq!(cycle.len(), 2);
    }

    #[test]
    fn reachability_follows_chains() {
        let nodes = vec![
            node("p", "a", vec![]),
            node("p", "b", vec![0]),
            node("p", "c", vec![1]),
            node("p", "d", vec![]),
        ];
        let reach = reachable(&nodes);
        assert!(reach[0][1] && reach[0][2] && reach[1][2]);
        assert!(!reach[2][0] && !reach[0][3] && !reach[3][0]);
    }
}
