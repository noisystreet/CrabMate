//! DAG 依赖校验与拓扑分层（Kahn）。

use std::collections::{HashMap, VecDeque};

use super::model::WorkflowNodeSpec;

pub(crate) fn topo_layers(nodes: &[WorkflowNodeSpec]) -> Result<Vec<Vec<String>>, String> {
    // Kahn 算法逐层生成拓扑层级。
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

    let mut current: VecDeque<String> = indegree
        .iter()
        .filter_map(|(k, v)| if *v == 0 { Some(k.clone()) } else { None })
        .collect();
    let mut layers: Vec<Vec<String>> = Vec::new();
    let mut visited = 0usize;

    while !current.is_empty() {
        let layer_nodes: Vec<String> = current.into_iter().collect();
        let mut next: VecDeque<String> = VecDeque::new();

        for x in layer_nodes.iter() {
            visited += 1;
            if let Some(ns) = adj.get(x) {
                for y in ns.iter() {
                    let entry = indegree
                        .get_mut(y)
                        .ok_or("internal error: missing indegree node")?;
                    *entry -= 1;
                    if *entry == 0 {
                        next.push_back(y.clone());
                    }
                }
            }
        }

        layers.push(layer_nodes);
        current = next;
    }

    if visited != nodes.len() {
        return Err("workflow_validate_only: 存在循环依赖（DAG 层级计算失败）".to_string());
    }
    Ok(layers)
}
pub(crate) fn validate_dag(nodes: &[WorkflowNodeSpec]) -> Result<(), String> {
    let mut node_map: HashMap<&str, &WorkflowNodeSpec> = HashMap::new();
    for n in nodes.iter() {
        node_map.insert(&n.id, n);
    }
    for n in nodes.iter() {
        for d in n.deps.iter() {
            if !node_map.contains_key(d.as_str()) {
                return Err(format!("节点 {} 依赖了未知节点 {}", n.id, d));
            }
        }
    }
    // cycle detection: Kahn
    let mut indegree: HashMap<String, usize> = nodes.iter().map(|n| (n.id.clone(), 0)).collect();
    let mut adj: HashMap<String, Vec<String>> = HashMap::new();
    for n in nodes.iter() {
        for d in n.deps.iter() {
            indegree.entry(n.id.clone()).and_modify(|x| *x += 1);
            adj.entry(d.clone()).or_default().push(n.id.clone());
        }
    }

    let mut q = VecDeque::new();
    for (k, v) in indegree.iter() {
        if *v == 0 {
            q.push_back(k.clone());
        }
    }
    let mut visited = 0usize;
    while let Some(x) = q.pop_front() {
        visited += 1;
        if let Some(next) = adj.get(&x) {
            for y in next.iter() {
                if let Some(v) = indegree.get_mut(y) {
                    *v -= 1;
                    if *v == 0 {
                        q.push_back(y.clone());
                    }
                }
            }
        }
    }
    if visited != nodes.len() {
        return Err("workflow 存在循环依赖（DAG 校验失败）".to_string());
    }
    Ok(())
}
