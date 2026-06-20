use super::*;

pub(super) const GRAPH_END: usize = usize::MAX;

/// One edge of the inflexion graph. `group` indexes a chunk group (a shifted
/// prefix run plus its main chunk) in the owning graph's arena.
#[derive(Debug, Clone, Copy)]
pub(super) struct GraphEdge {
    group: usize,
    text_start: usize,
    main_start: usize,
    text_end: usize,
    segment_type: u8,
    group_id: (usize, usize),
    next_node: usize,
}

/// Faithful port of C++ `InflexionGraph`: indexes nodes, drops weak paths when a
/// strong one exists, and minimizes the node count so that alternative
/// segmentations of one token collapse onto the same node span — reproducing the
/// reference edge ordering and node numbering exactly.
#[derive(Debug)]
pub(super) struct InflexionGraph<'a> {
    graph: Vec<Vec<GraphEdge>>,
    node2start: Vec<usize>,
    arena: Vec<Vec<BinaryAnalyzerChunk<'a>>>,
    only_weak_paths: bool,
}

impl<'a> Default for InflexionGraph<'a> {
    fn default() -> Self {
        // `only_weak_paths` starts true so the first strong path clears any
        // weak-only graph accumulated before it (C++ constructor invariant).
        Self {
            graph: Vec::new(),
            node2start: Vec::new(),
            arena: Vec::new(),
            only_weak_paths: true,
        }
    }
}

impl<'a> InflexionGraph<'a> {
    pub(super) fn is_empty(&self) -> bool {
        self.graph.is_empty()
    }

    pub(super) fn node_len(&self) -> usize {
        self.graph.len()
    }

    pub(super) fn edges_at(&self, node: usize) -> usize {
        self.graph[node].len()
    }

    pub(super) fn edge(&self, node: usize, index: usize) -> (&[BinaryAnalyzerChunk<'a>], usize) {
        let edge = &self.graph[node][index];
        (&self.arena[edge.group], edge.next_node)
    }

    /// Splits a path into its non-shifted edges (each a shifted prefix run plus
    /// its main chunk) and adds them, replicating C++ `addPath`.
    pub(super) fn add_path(&mut self, path: &BinaryAnalyzerPath<'a>, weak: bool) {
        if weak && !self.is_empty() && !self.only_weak_paths {
            return;
        } else if self.only_weak_paths && !weak {
            self.graph.clear();
            self.node2start.clear();
            self.arena.clear();
            self.only_weak_paths = false;
        }

        let edges_num = analyzer_path_edge_count(&path.chunks);
        let mut position = 0usize;
        let mut index = 0;
        while index < path.chunks.len() {
            let mut shifted_end = index;
            while shifted_end + 1 < path.chunks.len() && path.chunks[shifted_end].shift_orth {
                shifted_end += 1;
            }
            let group = path.chunks[index..=shifted_end].to_vec();
            let main = &group[group.len() - 1];
            let arena_index = self.arena.len();
            let text_start = group[0].original_start;
            let make = |next_node: usize| GraphEdge {
                group: arena_index,
                text_start,
                main_start: main.original_start,
                text_end: main.original_end,
                segment_type: main.segment_type,
                group_id: main.group_id,
                next_node,
            };
            let is_front = position == 0;
            let is_back = position + 1 == edges_num;

            if is_front && is_back {
                let edge = make(GRAPH_END);
                self.arena.push(group);
                self.add_start_edge(edge);
            } else if is_front {
                let next = if self.graph.is_empty() {
                    1
                } else {
                    self.graph.len()
                };
                let edge = make(next);
                self.arena.push(group);
                self.add_start_edge(edge);
            } else if is_back {
                let start_node = self.graph.len();
                let edge = make(GRAPH_END);
                self.arena.push(group);
                self.add_middle_edge(start_node, edge);
            } else {
                let start_node = self.graph.len();
                let edge = make(start_node + 1);
                self.arena.push(group);
                self.add_middle_edge(start_node, edge);
            }
            position += 1;
            index = shifted_end + 1;
        }
    }

    fn add_start_edge(&mut self, edge: GraphEdge) {
        if self.graph.is_empty() {
            self.graph.push(Vec::new());
            self.node2start.push(edge.text_start);
        }
        self.graph[0].push(edge);
    }

    fn add_middle_edge(&mut self, start_node: usize, edge: GraphEdge) {
        if start_node == self.graph.len() {
            self.graph.push(Vec::new());
            self.node2start.push(edge.text_start);
        }
        self.graph[start_node].push(edge);
    }

    /// Runs minimization, topological renumbering and last-node repair, returning
    /// the node count (the next free node number, C++ `graph.size()`).
    pub(super) fn finish(&mut self) -> usize {
        self.minimize();
        if self.graph.len() > 2 {
            self.sort_nodes_topologically();
        }
        self.repair_last_node_numbers();
        self.graph.len()
    }

    fn minimize(&mut self) {
        if self.graph.len() > 2 {
            while self.try_to_merge_two_nodes() {}
        }
    }

    fn try_to_merge_two_nodes(&mut self) -> bool {
        for node1 in 0..self.graph.len() {
            for node2 in ((node1 + 1)..self.graph.len()).rev() {
                if self.can_merge_nodes(node1, node2) {
                    self.do_merge_nodes(node1, node2);
                    return true;
                }
            }
        }
        false
    }

    fn can_merge_nodes(&self, node1: usize, node2: usize) -> bool {
        self.node2start[node1] == self.node2start[node2]
            && self.possible_paths(node1) == self.possible_paths(node2)
    }

    fn possible_paths(&self, node: usize) -> BTreeSet<BTreeSet<(usize, u8)>> {
        if node == GRAPH_END || node + 1 == self.graph.len() {
            return BTreeSet::new();
        }
        let mut res = BTreeSet::new();
        for edge in &self.graph[node] {
            let elem = (edge.text_start, edge.segment_type);
            if edge.next_node != self.graph.len() {
                for mut path in self.possible_paths(edge.next_node) {
                    path.insert(elem);
                    res.insert(path);
                }
            }
        }
        res
    }

    fn do_merge_nodes(&mut self, node1: usize, node2: usize) {
        debug_assert!(node1 < node2);
        let incoming = self.graph[node2].clone();
        for edge in incoming {
            if !edge_in(&self.graph[node1], &edge) {
                self.graph[node1].push(edge);
            }
        }
        self.redirect_edges(node2, node1);
        self.do_remove_node(node2);
    }

    fn redirect_edges(&mut self, from_node: usize, to_node: usize) {
        for node in 0..from_node {
            let mut i = 0;
            while i < self.graph[node].len() {
                if self.graph[node][i].next_node == from_node {
                    let mut redirected = self.graph[node][i];
                    redirected.next_node = to_node;
                    if edge_in(&self.graph[node], &redirected) {
                        self.graph[node].remove(i);
                    } else {
                        self.graph[node][i].next_node = to_node;
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }
        }
    }

    fn do_remove_node(&mut self, node: usize) {
        for i in (node + 1)..self.graph.len() {
            self.redirect_edges(i, i - 1);
            self.graph[i - 1] = self.graph[i].clone();
            self.node2start[i - 1] = self.node2start[i];
        }
        self.graph.pop();
        self.node2start.pop();
    }

    fn repair_last_node_numbers(&mut self) {
        let size = self.graph.len();
        for edges in &mut self.graph {
            for edge in edges {
                if edge.next_node == GRAPH_END {
                    edge.next_node = size;
                }
            }
        }
    }

    fn sort_nodes_topologically(&mut self) {
        let n = self.graph.len();
        let mut sorted: Vec<usize> = (0..n).collect();
        sorted.sort_by(|&i, &j| self.node2start[i].cmp(&self.node2start[j]));
        let mut old_to_new = vec![0usize; n];
        for (new_node, &old_node) in sorted.iter().enumerate() {
            old_to_new[old_node] = new_node;
        }
        for edges in &mut self.graph {
            for edge in edges {
                if edge.next_node < n {
                    edge.next_node = old_to_new[edge.next_node];
                }
            }
        }
        let graph_copy = self.graph.clone();
        let node2start_copy = self.node2start.clone();
        for old_node in 0..n {
            let new_node = old_to_new[old_node];
            self.graph[new_node] = graph_copy[old_node].clone();
            self.node2start[new_node] = node2start_copy[old_node];
        }
    }
}

pub(super) fn analyzer_path_edge_count(chunks: &[BinaryAnalyzerChunk<'_>]) -> usize {
    let mut count = 0usize;
    let mut index = 0usize;
    while index < chunks.len() {
        let mut shifted_end = index;
        while shifted_end + 1 < chunks.len() && chunks[shifted_end].shift_orth {
            shifted_end += 1;
        }
        count += 1;
        index = shifted_end + 1;
    }
    count
}

/// C++ `containsEqualEdge`: edges are equal by node span, segment identity and
/// target — never by decoded text.
pub(super) fn edge_in(edges: &[GraphEdge], edge: &GraphEdge) -> bool {
    edges.iter().any(|other| {
        other.text_start == edge.text_start
            && other.main_start == edge.main_start
            && other.text_end == edge.text_end
            && other.segment_type == edge.segment_type
            && other.next_node == edge.next_node
            && other.group_id == edge.group_id
    })
}
