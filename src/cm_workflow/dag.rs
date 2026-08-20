//! DAG 依赖校验与拓扑分层（Kahn）。

use std::collections::{HashMap, VecDeque};

use super::model::WorkflowNodeSpec;

type KahnGraph = (HashMap<String, usize>, HashMap<String, Vec<String>>);

fn kahn_graph_from_nodes(nodes: &[WorkflowNodeSpec]) -> Result<KahnGraph, String> {
    let mut indegree: HashMap<String, usize> =
        nodes.iter().map(|n| (n.id.clone(), 0usize)).collect();
    let mut adj: HashMap<String, Vec<String>> = HashMap::new();
    for n in nodes.iter() {
        for d in n.deps.iter() {
            adj.entry(d.clone()).or_default().push(n.id.clone());
            *indegree
                .get_mut(&n.id)
                .ok_or("internal error: missing indegree")? += 1;
        }
    }
    Ok((indegree, adj))
}

fn kahn_zero_indegree(indegree: &HashMap<String, usize>) -> VecDeque<String> {
    indegree
        .iter()
        .filter(|(_, v)| **v == 0)
        .map(|(k, _)| k.clone())
        .collect()
}

fn kahn_consume_successors(
    x: &str,
    adj: &HashMap<String, Vec<String>>,
    indegree: &mut HashMap<String, usize>,
    next: &mut VecDeque<String>,
) -> Result<(), String> {
    let Some(ns) = adj.get(x) else {
        return Ok(());
    };
    for y in ns.iter() {
        let entry = indegree
            .get_mut(y)
            .ok_or("internal error: missing indegree node")?;
        *entry -= 1;
        if *entry == 0 {
            next.push_back(y.clone());
        }
    }
    Ok(())
}

pub fn topo_layers(nodes: &[WorkflowNodeSpec]) -> Result<Vec<Vec<String>>, String> {
    let (mut indegree, adj) = kahn_graph_from_nodes(nodes)?;
    let mut current = kahn_zero_indegree(&indegree);
    let mut layers: Vec<Vec<String>> = Vec::new();
    let mut visited = 0usize;

    while !current.is_empty() {
        let layer_nodes: Vec<String> = current.into_iter().collect();
        let mut next: VecDeque<String> = VecDeque::new();
        for x in layer_nodes.iter() {
            visited += 1;
            kahn_consume_successors(x, &adj, &mut indegree, &mut next)?;
        }
        layers.push(layer_nodes);
        current = next;
    }

    if visited != nodes.len() {
        return Err("workflow_validate_only: 存在循环依赖（DAG 层级计算失败）".to_string());
    }
    Ok(layers)
}

fn unknown_dependency_error(nodes: &[WorkflowNodeSpec]) -> Result<(), String> {
    let node_map: HashMap<&str, &WorkflowNodeSpec> =
        nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    for n in nodes.iter() {
        for d in n.deps.iter() {
            if !node_map.contains_key(d.as_str()) {
                return Err(format!("节点 {} 依赖了未知节点 {}", n.id, d));
            }
        }
    }
    Ok(())
}

fn kahn_visit_count(nodes: &[WorkflowNodeSpec]) -> Result<usize, String> {
    let (mut indegree, adj) = kahn_graph_from_nodes(nodes)?;
    let mut q = kahn_zero_indegree(&indegree);
    let mut visited = 0usize;
    while let Some(x) = q.pop_front() {
        visited += 1;
        kahn_consume_successors(&x, &adj, &mut indegree, &mut q)?;
    }
    Ok(visited)
}

pub(crate) fn validate_dag(nodes: &[WorkflowNodeSpec]) -> Result<(), String> {
    unknown_dependency_error(nodes)?;
    let visited = kahn_visit_count(nodes)?;
    if visited != nodes.len() {
        return Err("workflow 存在循环依赖（DAG 校验失败）".to_string());
    }
    Ok(())
}
