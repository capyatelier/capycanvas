use super::PortableId;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Content { Paint(PortableId), Group(PortableId), Effect(PortableId), Selection(PortableId), Paper }

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shape {
    Composition { result: PortableId },
    Stack { entries: Vec<PortableId> },
    Occurrence { content: Content, mask: Option<PortableId> },
    Paint,
    Coverage,
    Effect { definition: PortableId, inputs: Vec<PortableId> },
    Definition { dependencies: Vec<PortableId> },
    Selection,
    Guides,
    Output { composition: PortableId },
    Unknown { ancillary: bool, references: Vec<PortableId> },
}
impl Shape {
    fn reference_count(&self) -> usize {
        match self {
            Self::Composition { .. } | Self::Output { .. } => 1,
            Self::Stack { entries } => entries.len(),
            Self::Occurrence { content, mask } => usize::from(!matches!(content, Content::Paper)) + usize::from(mask.is_some()),
            Self::Effect { inputs, .. } => inputs.len().saturating_add(1),
            Self::Definition { dependencies } => dependencies.len(),
            Self::Unknown { references, .. } => references.len(),
            _ => 0,
        }
    }
    fn references(&self) -> Vec<PortableId> {
        match self {
            Self::Composition { result } => vec![*result],
            Self::Stack { entries } => entries.clone(),
            Self::Occurrence { content, mask } => match content {
                Content::Paint(id) | Content::Group(id) | Content::Effect(id) | Content::Selection(id) =>
                    std::iter::once(*id).chain(*mask).collect(),
                Content::Paper => mask.iter().copied().collect(),
            },
            Self::Effect { definition, inputs } => std::iter::once(*definition).chain(inputs.iter().copied()).collect(),
            Self::Definition { dependencies } => dependencies.clone(),
            Self::Output { composition } => vec![*composition],
            Self::Unknown { references, .. } => references.clone(),
            _ => Vec::new(),
        }
    }
    fn ancillary(&self) -> bool { matches!(self, Self::Unknown { ancillary: true, .. }) }
}

#[derive(Clone, Copy, Debug)]
pub struct GraphLimits { pub objects: usize, pub edges: usize, pub depth: usize }
impl Default for GraphLimits {
    fn default() -> Self { Self { objects: 65_536, edges: 262_144, depth: 128 } }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Support { Editable, Preserved(BTreeSet<&'static str>) }

#[derive(Default)]
pub struct GraphShape {
    pub objects: BTreeMap<PortableId, Shape>,
    pub resources: BTreeSet<PortableId>,
    pub outputs: Vec<PortableId>,
    pub default_output: Option<PortableId>,
}
impl GraphShape {
    pub fn validate(&self, root: PortableId, limits: GraphLimits) -> Result<Support, &'static str> {
        if self.objects.len() > limits.objects { return Err("Authored object limit exceeded"); }
        if self.objects.keys().any(|id| self.resources.contains(id)) { return Err("Duplicate authored identity"); }
        let root_shape = self.objects.get(&root).ok_or("Missing authored root")?;
        if root_shape.ancillary() { return Err("Ancillary authored root"); }
        let mut reasons = BTreeSet::new();
        if !matches!(root_shape, Shape::Composition { .. }) { reasons.insert("Unsupported authored root"); }
        if self.outputs.is_empty() != self.default_output.is_none()
            || self.default_output.is_some_and(|id| !self.outputs.contains(&id)) {
            return Err("Invalid default output");
        }
        let mut seen_outputs = BTreeSet::new();
        for id in &self.outputs {
            if !seen_outputs.insert(*id) { return Err("Duplicate output"); }
            let shape = self.objects.get(id).ok_or("Missing output")?;
            if !matches!(shape, Shape::Output { .. } | Shape::Unknown { ancillary: false, .. }) {
                return Err("Invalid output type");
            }
        }
        let mut memberships = BTreeMap::<PortableId, usize>::new();
        let mut uses = BTreeMap::<PortableId, usize>::new();
        let mut edges = 0usize;
        let mut evaluation = BTreeMap::<PortableId, Vec<PortableId>>::new();
        let mut expansion = BTreeMap::<PortableId, Vec<PortableId>>::new();
        for (id, shape) in &self.objects {
            edges = edges.checked_add(shape.reference_count()).ok_or("Authored edge count overflow")?;
            if edges > limits.edges { return Err("Authored edge limit exceeded"); }
            let references = shape.references();
            for target in &references {
                if let Some(referenced) = self.objects.get(target) {
                    if referenced.ancillary() { return Err("Artwork or ancillary record depends on ancillary data"); }
                } else if !self.resources.contains(target) { return Err("Dangling authored reference"); }
            }
            let mut expect = |target: PortableId, predicate: fn(&Shape) -> bool| -> Result<(), &'static str> {
                match self.objects.get(&target) {
                    Some(Shape::Unknown { ancillary: false, .. }) => { reasons.insert("Unknown referenced type"); Ok(()) },
                    Some(value) if predicate(value) => Ok(()),
                    _ => Err("Invalid authored relationship"),
                }
            };
            let mut dependencies = Vec::new();
            match shape {
                Shape::Composition { result } => {
                    expect(*result, |s| matches!(s, Shape::Stack { .. }))?;
                    *uses.entry(*result).or_default() += 1;
                    dependencies.push(*result);
                }
                Shape::Stack { entries } => {
                    for entry in entries {
                        expect(*entry, |s| matches!(s, Shape::Occurrence { .. }))?;
                        if *memberships.entry(*entry).or_default() != 0 { return Err("Occurrence belongs to multiple stack slots"); }
                        *memberships.get_mut(entry).unwrap() += 1;
                    }
                    dependencies.extend(entries);
                }
                Shape::Occurrence { content, mask } => {
                    let target = match content {
                        Content::Paint(id) => { expect(*id, |s| matches!(s, Shape::Paint))?; Some(*id) },
                        Content::Group(id) => { expect(*id, |s| matches!(s, Shape::Stack { .. }))?; Some(*id) },
                        Content::Effect(id) => { expect(*id, |s| matches!(s, Shape::Effect { .. }))?; Some(*id) },
                        Content::Selection(id) => { expect(*id, |s| matches!(s, Shape::Selection))?; Some(*id) },
                        Content::Paper => None,
                    };
                    if let Some(mask) = mask {
                        expect(*mask, |s| matches!(s, Shape::Coverage))?;
                    }
                    for target in target.into_iter().chain(*mask) {
                        *uses.entry(target).or_default() += 1;
                        dependencies.push(target);
                    }
                }
                Shape::Effect { definition, inputs } => {
                    expect(*definition, |s| matches!(s, Shape::Definition { .. }))?;
                    for input in inputs {
                        expect(*input, |s| matches!(s, Shape::Paint | Shape::Coverage | Shape::Effect { .. } | Shape::Composition { .. } | Shape::Stack { .. }))?;
                        *uses.entry(*input).or_default() += 1;
                    }
                    dependencies.extend(inputs);
                    if !inputs.is_empty() { reasons.insert("Explicit effect inputs require graph editing"); }
                }
                Shape::Definition { dependencies } => {
                    for dependency in dependencies {
                        expect(*dependency, |s| matches!(s, Shape::Definition { .. }))?;
                    }
                    expansion.insert(*id, dependencies.clone());
                    if !dependencies.is_empty() { reasons.insert("Reusable definition requires graph editing"); }
                }
                Shape::Output { composition } => {
                    expect(*composition, |s| matches!(s, Shape::Composition { .. }))?;
                    if *composition != root { reasons.insert("Output has an independent composition"); }
                }
                Shape::Unknown { ancillary: false, .. } => { reasons.insert("Unknown authored type"); }
                _ => (),
            }
            evaluation.insert(*id, dependencies);
        }
        for count in uses.into_values() {
            if count > 1 {
                reasons.insert("Shared editable content");
            }
        }
        acyclic(&evaluation, limits.depth)?;
        acyclic(&expansion, limits.depth)?;
        if self.objects.values().filter(|s| matches!(s, Shape::Composition { .. })).count() != 1
            || self.outputs.len() != 1
            || self.objects.values().filter(|s| matches!(s, Shape::Output { .. })).count() != 1 {
            reasons.insert("Unsupported composition or output inventory");
        }
        Ok(if reasons.is_empty() { Support::Editable } else { Support::Preserved(reasons) })
    }
}

fn acyclic(graph: &BTreeMap<PortableId, Vec<PortableId>>, limit: usize) -> Result<(), &'static str> {
    let mut counts: BTreeMap<_, usize> = graph.keys().map(|id| (*id, 0)).collect();
    for dependencies in graph.values() {
        for dependency in dependencies { *counts.entry(*dependency).or_default() += 1; }
    }
    let mut queue: VecDeque<_> = counts.iter().filter(|(_, count)| **count == 0).map(|(id, _)| (*id, 1usize)).collect();
    let mut depths = BTreeMap::<PortableId, usize>::new();
    let mut visited = 0;
    while let Some((id, depth)) = queue.pop_front() {
        if depth > limit { return Err("Authored dependency depth exceeded"); }
        visited += 1;
        for dependency in graph.get(&id).into_iter().flatten() {
            let next_depth = depths.entry(*dependency).or_default();
            *next_depth = (*next_depth).max(depth + 1);
            let count = counts.get_mut(dependency).unwrap();
            *count -= 1;
            if *count == 0 { queue.push_back((*dependency, *next_depth)); }
        }
    }
    if visited != counts.len() { return Err("Cyclic authored dependency"); }
    Ok(())
}
