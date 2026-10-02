use std::collections::{BTreeMap, BTreeSet};
use fluent_bundle::{concurrent::FluentBundle, FluentArgs, FluentResource};
use fluent_syntax::{ast, parser};
use crate::localization::{CATALOGS, MessageId, SHIPPED_LANGUAGES};

type Pattern<'a> = ast::Pattern<&'a str>;
type Inventory<'a> = BTreeMap<String, Pattern<'a>>;

fn inventory<'a>(files: &[(&str, &'a str)]) -> Result<Inventory<'a>, String> {
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

fn requirements(key: &str, patterns: &Inventory<'_>, stack: &mut BTreeSet<String>) -> Result<BTreeSet<String>, String> {
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

fn validate<'a>(english: &Inventory<'a>, files: &[(&str, &'a str)], require_complete: bool) -> Result<Inventory<'a>, String> {
    let translated = inventory(files)?;
    for key in english.keys() {
        if require_complete && !translated.contains_key(key) { return Err(format!("missing canonical identity {key}")); }
    }
    for key in translated.keys() {
        let expected = requirements(key, english, &mut BTreeSet::new())?;
        let actual = requirements(key, &translated, &mut BTreeSet::new())?;
        if !actual.is_subset(&expected) { return Err(format!("unexpected variables for {key}: {actual:?}")); }
    }
    Ok(translated)
}

fn bundle(locale: &str, files: &[(&str, &str)]) -> FluentBundle<FluentResource> {
    let mut bundle = FluentBundle::new_concurrent(vec![locale.parse().unwrap()]);
    bundle.set_use_isolating(false);
    for (_, source) in files { bundle.add_resource(FluentResource::try_new((*source).to_owned()).unwrap()).unwrap(); }
    bundle
}

fn assert_formats(bundle: &FluentBundle<FluentResource>, key: &str, args: &FluentArgs<'_>) -> String {
    let (id, attribute) = key.split_once('.').map_or((key, None), |(id, attribute)| (id, Some(attribute)));
    let message = bundle.get_message(id).unwrap();
    let pattern = attribute.map(|attribute| message.get_attribute(attribute).unwrap().value()).or_else(|| message.value()).unwrap();
    let mut errors = Vec::new();
    let result = bundle.format_pattern(pattern, Some(args), &mut errors).into_owned();
    assert!(errors.is_empty(), "{key}: {errors:?}");
    result
}

#[test]
fn catalogs_have_canonical_identities_and_format_without_errors() {
    let english_files = CATALOGS.iter().find(|(locale, _)| *locale == "en").unwrap().1;
    let english = inventory(english_files).unwrap();
    for id in MessageId::ALL { assert!(english.contains_key(id.key())); }
    let long_name = "作品🎨{draft}\"".repeat(128);
    for (locale, files) in CATALOGS {
        let require_complete = SHIPPED_LANGUAGES.iter().any(|language| language.tag() == *locale);
        let translated = validate(&english, files, require_complete).unwrap_or_else(|error| panic!("{locale}: {error}"));
        let bundle = bundle(locale, files);
        for key in translated.keys().filter(|key| !key.starts_with('-')) {
            let variables = requirements(key, &english, &mut BTreeSet::new()).unwrap();
            for count in [0, 1, 37] {
                for literal in ["画布 한글 日本語 🎨 {draft} \"quoted\"", long_name.as_str()] {
                    let mut args = FluentArgs::new();
                    for variable in &variables { args.set(variable, literal); }
                    args.set("count", count);
                    assert_formats(&bundle, key, &args);
                }
                let mut args = FluentArgs::new();
                for variable in &variables { args.set(variable, count); }
                assert_formats(&bundle, key, &args);
            }
        }
    }
}

#[test]
fn validator_rejects_invalid_catalogs() {
    let english = inventory(&[("en", "label = Label\nmessage = Hello { $name }\n")]).unwrap();
    for source in ["label = {\n", "label = A\nlabel = B\n", "label = A\n", "label = A\nmessage = { $other }\n", "label = A\nmessage = { missing }\n", "label = A\nmessage = { -missing }\n", "label = A\nmessage = { label.missing }\n", "label = { message }\nmessage = { label }\n"] {
        assert!(validate(&english, &[("fixture", source)], true).is_err(), "accepted {source:?}");
    }
    assert!(inventory(&[("one", "label = A"), ("two", "label = B")]).is_err());
    assert!(inventory(&[("one", "-label = A"), ("two", "-label = B")]).is_err());
}

#[test]
fn translated_patterns_can_omit_variables_and_change_plural_branches() {
    let english = inventory(&[("en", "-brand = Capy\n    .short = CC\nlabel = { -brand.short ->\n    [CC] CC\n   *[other] Capy\n}\nmessage = { $count ->\n    [one] One { $name }\n   *[other] Many { $name }\n}\n")]).unwrap();
    let translated = "-brand = Capy\n    .short = CC\nlabel = { -brand.short ->\n    [CC] CC\n   *[other] Capy\n}\nmessage = { $count ->\n    [0] None\n   *[other] Several\n}\n";
    validate(&english, &[("translation", translated)], true).unwrap();
}

#[test]
fn incomplete_drafts_are_allowed_but_shipped_catalogs_must_be_complete() {
    let english = inventory(&[("en", "label = Label\nmessage = Hello { $name }\n")]).unwrap();
    let partial = [("draft", "label = Label\n")];
    assert!(validate(&english, &partial, false).is_ok());
    assert!(validate(&english, &partial, true).is_err());
    for source in ["label = { $other }\n", "label = { message }\n", "unknown = Unknown\n"] {
        assert!(validate(&english, &[("draft", source)], false).is_err());
    }
}

#[test]
fn references_and_call_arguments_contribute_transitive_requirements() {
    let english = inventory(&[("en", "-brand = { $name }\nlabel = { -brand }\nbound = { -brand(name: \"Capy\") }\nmessage = { $count }\n")]).unwrap();
    assert_eq!(requirements("label", &english, &mut BTreeSet::new()).unwrap(), BTreeSet::from(["name".to_owned()]));
    assert_eq!(requirements("message", &english, &mut BTreeSet::new()).unwrap(), BTreeSet::from(["count".to_owned()]));
    assert!(requirements("bound", &english, &mut BTreeSet::new()).unwrap().is_empty());
    let invalid = "-brand = { $other }\nlabel = { -brand }\nbound = { -brand(name: \"Capy\") }\nmessage = { $count }\n";
    assert!(validate(&english, &[("fixture", invalid)], true).is_err());
    let invalid_branch = "-brand = Name\nlabel = Name\nbound = Name\nmessage = { $count ->\n    [one] One\n   *[other] { missing.attribute }\n}\n";
    assert!(validate(&english, &[("fixture", invalid_branch)], true).is_err());
}

#[test]
fn unsupported_functions_are_rejected_in_all_patterns_and_branches() {
    let english = inventory(&[("en", "-brand = Capy\nmessage = { $count }\n")]).unwrap();
    for source in [
        "-brand = Capy\nmessage = { NUMBER($count) }\n",
        "-brand = Capy\nmessage = { $count ->\n    [913] { UNKNOWN() }\n   *[other] Files\n}\n",
        "-brand = { UNKNOWN() }\nmessage = { $count }\n",
    ] {
        assert!(validate(&english, &[("fixture", source)], true).is_err(), "accepted {source:?}");
    }
}

#[test]
fn dynamic_arguments_remain_literal_and_formatting_errors_are_visible() {
    let files = [("fixture", "message = { $count ->\n    [0] No files\n    [one] One file: { $name }\n   *[other] { $count } files: { $name }\n}\n")];
    let bundle = bundle("en", &files);
    for count in [0, 1, 37] {
        for name in ["画布 한글 日本語 🎨 \u{2066}RTL\u{2069} { $other } \"quoted\"".to_owned(), "作品🎨{draft}\"".repeat(128)] {
            let mut args = FluentArgs::new();
            args.set("count", count);
            args.set("name", name.as_str());
            let formatted = assert_formats(&bundle, "message", &args);
            if count > 0 { assert!(formatted.contains(&name)); }
        }
    }
    let mut errors = Vec::new();
    bundle.format_pattern(bundle.get_message("message").unwrap().value().unwrap(), None, &mut errors);
    assert!(!errors.is_empty());
}
