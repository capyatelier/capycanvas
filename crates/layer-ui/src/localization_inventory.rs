use std::collections::{BTreeMap, BTreeSet};
use fluent_syntax::{ast, parser};

type Pattern<'a> = ast::Pattern<&'a str>;
pub(crate) type Inventory<'a> = BTreeMap<String, Pattern<'a>>;

pub(crate) fn inventory<'a>(files: &[(&str, &'a str)]) -> Result<Inventory<'a>, String> {
    let mut patterns = BTreeMap::new();
    let mut identities = BTreeSet::new();
    for (file, source) in files {
        let resource = parser::parse(*source).map_err(|(_, errors)| format!("{file}: {errors:?}"))?;
        for entry in resource.body {
            let (id, value, attributes) = match entry {
                ast::Entry::Message(message) => (message.id.name.to_owned(), message.value, message.attributes),
                ast::Entry::Term(term) => (format!("-{}", term.id.name), Some(term.value), term.attributes),
                _ => continue,
            };
            if !identities.insert(id.clone()) { return Err(format!("duplicate {id}")); }
            if let Some(pattern) = value { patterns.insert(id.clone(), pattern); }
            for attribute in attributes {
                let key = format!("{id}.{}", attribute.id.name);
                if patterns.insert(key.clone(), attribute.value).is_some() { return Err(format!("duplicate {key}")); }
            }
        }
    }
    Ok(patterns)
}

pub(crate) fn requirements(key: &str, patterns: &Inventory<'_>, stack: &mut BTreeSet<String>) -> Result<BTreeSet<String>, String> {
    if !stack.insert(key.to_owned()) { return Err(format!("cyclic reference {key}")); }
    let pattern = patterns.get(key).ok_or_else(|| format!("missing reference {key}"))?;
    let result = pattern_requirements(pattern, patterns, stack);
    stack.remove(key);
    result
}

fn pattern_requirements(pattern: &Pattern<'_>, patterns: &Inventory<'_>, stack: &mut BTreeSet<String>) -> Result<BTreeSet<String>, String> {
    let mut variables = BTreeSet::new();
    for element in &pattern.elements {
        if let ast::PatternElement::Placeable { expression } = element {
            variables.extend(expression_requirements(expression, patterns, stack)?);
        }
    }
    Ok(variables)
}

fn expression_requirements(expression: &ast::Expression<&str>, patterns: &Inventory<'_>, stack: &mut BTreeSet<String>) -> Result<BTreeSet<String>, String> {
    match expression {
        ast::Expression::Inline(inline) => inline_requirements(inline, patterns, stack),
        ast::Expression::Select { selector, variants } => {
            let mut variables = inline_requirements(selector, patterns, stack)?;
            for variant in variants { variables.extend(pattern_requirements(&variant.value, patterns, stack)?); }
            Ok(variables)
        }
    }
}

fn argument_requirements(arguments: &ast::CallArguments<&str>, patterns: &Inventory<'_>, stack: &mut BTreeSet<String>) -> Result<BTreeSet<String>, String> {
    let mut variables = BTreeSet::new();
    for value in arguments.positional.iter().chain(arguments.named.iter().map(|argument| &argument.value)) {
        variables.extend(inline_requirements(value, patterns, stack)?);
    }
    Ok(variables)
}

fn inline_requirements(expression: &ast::InlineExpression<&str>, patterns: &Inventory<'_>, stack: &mut BTreeSet<String>) -> Result<BTreeSet<String>, String> {
    use ast::InlineExpression::*;
    match expression {
        VariableReference { id } => Ok(BTreeSet::from([id.name.to_owned()])),
        MessageReference { id, attribute } => requirements(&reference_key(id.name, attribute.as_ref(), false), patterns, stack),
        TermReference { id, attribute, arguments } => {
            let mut variables = requirements(&reference_key(id.name, attribute.as_ref(), true), patterns, stack)?;
            if let Some(arguments) = arguments {
                for argument in &arguments.named { variables.remove(argument.name.name); }
                variables.extend(argument_requirements(arguments, patterns, stack)?);
            }
            Ok(variables)
        }
        FunctionReference { id, .. } => Err(format!("unsupported function {}", id.name)),
        Placeable { expression } => expression_requirements(expression, patterns, stack),
        StringLiteral { .. } | NumberLiteral { .. } => Ok(BTreeSet::new()),
    }
}

fn reference_key(id: &str, attribute: Option<&ast::Identifier<&str>>, term: bool) -> String {
    format!("{}{id}{}", if term { "-" } else { "" }, attribute.map(|attribute| format!(".{}", attribute.name)).unwrap_or_default())
}
