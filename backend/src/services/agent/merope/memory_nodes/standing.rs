//! How their life stands now (「现在」, `node.standing` in the core): what a
//! new fact she keeps may change of it.
//!
//! A fact is often put on a node of its own (「新工作」, 「公司食堂」) while
//! what it changes stands on a node she had (「工作」, 「饮食口味」): the two
//! are not linked, and their names do not match. The things she already
//! knew that the fact brought to mind are kept on that node, though, so a
//! fact may change how things stand on its own nodes and on theirs
//! ([`nodes_it_may_change`]). What it starts that nothing held yet stands on
//! its own node ([`where_it_starts`]).
//!
//! Pure: no clock, no database. The memory bench's trial of a kept list
//! (`memory_bench::background::state_list`) asks through these; going over a
//! stretch of talk will too, once it writes 「现在」.

/// The nodes whose 「现在」 a new fact may change, by name: the ones it is
/// kept on, then those the things it brought to mind (`known_on`, each
/// known thing's nodes) are kept on. Each once, in that order.
pub(crate) fn nodes_it_may_change(own: &[String], known_on: &[Vec<String>]) -> Vec<String> {
    let mut nodes: Vec<String> = Vec::new();
    for node in own.iter().chain(known_on.iter().flatten()) {
        if !nodes.contains(node) {
            nodes.push(node.clone());
        }
    }
    nodes
}

/// Where something a fact starts stands: the node asked for when the fact
/// is kept on it, else the first node the fact is kept on (the model's own
/// name for it, 「习惯」 for a fact kept on 「早餐」, is not a node it is on).
/// `None` when the fact is kept on no node.
pub(crate) fn where_it_starts<'a>(asked: &str, own: &'a [String]) -> Option<&'a str> {
    own.iter()
        .find(|node| node.as_str() == asked)
        .or_else(|| own.first())
        .map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn a_fact_on_a_node_of_its_own_still_reaches_the_node_it_changes() {
        // 「新工作」 is the fact's own node; what it brought to mind is on 「工作」.
        let nodes = nodes_it_may_change(
            &names(&["新工作"]),
            &[names(&["工作", "加班"]), names(&[]), names(&["工作"])],
        );
        assert_eq!(nodes, names(&["新工作", "工作", "加班"]));
        assert_eq!(
            nodes_it_may_change(&[], &[names(&["宠物"])]),
            names(&["宠物"])
        );
        assert!(nodes_it_may_change(&[], &[]).is_empty());
    }

    #[test]
    fn what_a_fact_starts_stands_on_its_own_node() {
        let own = names(&["早餐", "小区"]);
        assert_eq!(where_it_starts("小区", &own), Some("小区"));
        assert_eq!(where_it_starts("习惯", &own), Some("早餐"));
        assert_eq!(where_it_starts("习惯", &[]), None);
    }
}
