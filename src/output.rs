use std::io::{self, Write};

use crate::model::{ActionGuide, OwnershipGraph, OwnershipNode, Resolution};

pub fn print_inspect(
    graphs: &[OwnershipGraph],
    explain: bool,
    mut out: impl Write,
) -> io::Result<()> {
    for (graph_index, graph) in graphs.iter().enumerate() {
        if graph_index > 0 {
            writeln!(out)?;
        }
        writeln!(out, "{}", graph.command)?;
        if graph.resolutions.is_empty() {
            writeln!(out, "  not found in PATH")?;
            continue;
        }
        for resolution in &graph.resolutions {
            print_resolution(&mut out, graph, resolution, explain)?;
        }
    }
    Ok(())
}

pub fn print_list(graphs: &[OwnershipGraph], explain: bool, mut out: impl Write) -> io::Result<()> {
    let command_width = graphs
        .iter()
        .map(|graph| graph.command.len())
        .max()
        .unwrap_or(7)
        .max(7);
    writeln!(
        out,
        "{:<command_width$}  {:<10}  {:<9}  OWNER CHAIN",
        "COMMAND", "CONFIDENCE", "SHADOWED"
    )?;
    for graph in graphs {
        let Some(active) = graph.active() else {
            writeln!(
                out,
                "{:<command_width$}  {:<10}  {:<9}  not found",
                graph.command, "unknown", "0"
            )?;
            continue;
        };
        let confidence = active
            .primary_owner()
            .map(|owner| owner.confidence.as_str())
            .unwrap_or("unknown");
        writeln!(
            out,
            "{:<command_width$}  {:<10}  {:<9}  {}",
            graph.command,
            confidence,
            graph.shadowed_count(),
            owner_chain(active)
        )?;
    }

    if explain && !graphs.is_empty() {
        writeln!(out, "\nDetails\n")?;
        print_inspect(graphs, true, &mut out)?;
    }
    Ok(())
}

fn print_resolution(
    mut out: impl Write,
    graph: &OwnershipGraph,
    resolution: &Resolution,
    explain: bool,
) -> io::Result<()> {
    writeln!(
        out,
        "  {}: {}",
        resolution.status.as_str(),
        resolution.path.display()
    )?;
    if resolution.path != resolution.real_path {
        writeln!(out, "    resolves to: {}", resolution.real_path.display())?;
    }
    writeln!(
        out,
        "    ownership: {} -> {}",
        graph.command,
        owner_chain(resolution)
    )?;

    if explain {
        for (index, owner) in resolution.owners.iter().enumerate() {
            writeln!(
                out,
                "    owner[{}]: {} ({}, {})",
                index + 1,
                owner.display_name(),
                owner.kind().as_str(),
                owner.confidence.as_str()
            )?;
            if let Some(package) = &owner.package {
                writeln!(out, "      package: {package}")?;
            }
            if let Some(version) = &owner.version {
                writeln!(out, "      version: {version}")?;
            }
            for evidence in &owner.evidence {
                writeln!(
                    out,
                    "      evidence[{}]: {}",
                    evidence.source, evidence.detail
                )?;
            }
            print_actions(&mut out, &owner.actions, "      ")?;
        }
    } else if let Some(primary) = resolution.primary_owner() {
        print_actions(&mut out, &primary.actions, "    ")?;
        if resolution.owners.len() > 1 {
            writeln!(
                out,
                "    hint: use --explain to see how the manager itself was installed"
            )?;
        }
    }
    Ok(())
}

fn owner_chain(resolution: &Resolution) -> String {
    if resolution.owners.is_empty() {
        return "unconfirmed owner [unknown]".into();
    }
    resolution
        .owners
        .iter()
        .map(|owner| format!("{} [{}]", owner.display_name(), owner.confidence.as_str()))
        .collect::<Vec<_>>()
        .join(" -> ")
}

fn print_actions(mut out: impl Write, actions: &ActionGuide, indent: &str) -> io::Result<()> {
    if let Some(inspect) = &actions.inspect {
        writeln!(out, "{indent}inspect: {inspect}")?;
    }
    if let Some(update) = &actions.update {
        writeln!(out, "{indent}update: {update}")?;
    }
    if let Some(remove) = &actions.remove {
        writeln!(out, "{indent}remove: {remove}")?;
    }
    if let Some(note) = &actions.note {
        writeln!(out, "{indent}note: {note}")?;
    }
    Ok(())
}

pub fn print_json(graphs: &[OwnershipGraph], mut out: impl Write) -> io::Result<()> {
    writeln!(out, "[")?;
    for (graph_index, graph) in graphs.iter().enumerate() {
        writeln!(out, "  {{")?;
        writeln!(out, "    \"command\": \"{}\",", escape(&graph.command))?;
        writeln!(out, "    \"resolutions\": [")?;
        for (resolution_index, resolution) in graph.resolutions.iter().enumerate() {
            print_resolution_json(
                &mut out,
                resolution,
                resolution_index + 1 == graph.resolutions.len(),
            )?;
        }
        writeln!(out, "    ]")?;
        let comma = if graph_index + 1 == graphs.len() {
            ""
        } else {
            ","
        };
        writeln!(out, "  }}{comma}")?;
    }
    writeln!(out, "]")
}

fn print_resolution_json(
    mut out: impl Write,
    resolution: &Resolution,
    last: bool,
) -> io::Result<()> {
    writeln!(out, "      {{")?;
    json_string_field(&mut out, 8, "status", resolution.status.as_str(), true)?;
    json_string_field(
        &mut out,
        8,
        "path",
        &resolution.path.to_string_lossy(),
        true,
    )?;
    json_string_field(
        &mut out,
        8,
        "real_path",
        &resolution.real_path.to_string_lossy(),
        true,
    )?;
    writeln!(out, "        \"ownership_chain\": [")?;
    for (owner_index, owner) in resolution.owners.iter().enumerate() {
        print_owner_json(&mut out, owner, owner_index + 1 == resolution.owners.len())?;
    }
    writeln!(out, "        ]")?;
    writeln!(out, "      }}{}", if last { "" } else { "," })
}

fn print_owner_json(mut out: impl Write, owner: &OwnershipNode, last: bool) -> io::Result<()> {
    writeln!(out, "          {{")?;
    json_string_field(&mut out, 12, "id", owner.id.as_str(), true)?;
    json_string_field(&mut out, 12, "name", owner.display_name(), true)?;
    json_string_field(&mut out, 12, "kind", owner.kind().as_str(), true)?;
    json_optional_field(&mut out, 12, "package", owner.package.as_deref(), true)?;
    json_optional_field(&mut out, 12, "version", owner.version.as_deref(), true)?;
    json_string_field(&mut out, 12, "confidence", owner.confidence.as_str(), true)?;
    writeln!(out, "            \"evidence\": [")?;
    for (index, evidence) in owner.evidence.iter().enumerate() {
        writeln!(out, "              {{")?;
        json_string_field(&mut out, 16, "source", &evidence.source, true)?;
        json_string_field(&mut out, 16, "detail", &evidence.detail, false)?;
        writeln!(
            out,
            "              }}{}",
            if index + 1 == owner.evidence.len() {
                ""
            } else {
                ","
            }
        )?;
    }
    writeln!(out, "            ],")?;
    writeln!(out, "            \"action_guide\": {{")?;
    json_optional_field(
        &mut out,
        14,
        "inspect",
        owner.actions.inspect.as_deref(),
        true,
    )?;
    json_optional_field(
        &mut out,
        14,
        "update",
        owner.actions.update.as_deref(),
        true,
    )?;
    json_optional_field(
        &mut out,
        14,
        "remove",
        owner.actions.remove.as_deref(),
        true,
    )?;
    json_optional_field(&mut out, 14, "note", owner.actions.note.as_deref(), false)?;
    writeln!(out, "            }}")?;
    writeln!(out, "          }}{}", if last { "" } else { "," })
}

fn json_string_field(
    mut out: impl Write,
    indent: usize,
    key: &str,
    value: &str,
    comma: bool,
) -> io::Result<()> {
    writeln!(
        out,
        "{:indent$}\"{}\": \"{}\"{}",
        "",
        escape(key),
        escape(value),
        if comma { "," } else { "" }
    )
}

fn json_optional_field(
    mut out: impl Write,
    indent: usize,
    key: &str,
    value: Option<&str>,
    comma: bool,
) -> io::Result<()> {
    let suffix = if comma { "," } else { "" };
    match value {
        Some(value) => writeln!(
            out,
            "{:indent$}\"{}\": \"{}\"{suffix}",
            "",
            escape(key),
            escape(value)
        ),
        None => writeln!(out, "{:indent$}\"{}\": null{suffix}", "", escape(key)),
    }
}

fn escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character.is_control() => {
                escaped.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => escaped.push(character),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::model::{Confidence, OwnerId, OwnershipNode, ResolutionStatus};

    fn graph() -> OwnershipGraph {
        OwnershipGraph {
            command: "node".into(),
            resolutions: vec![Resolution {
                path: PathBuf::from("/opt/homebrew/bin/node"),
                real_path: PathBuf::from("/opt/homebrew/Cellar/node/25/bin/node"),
                status: ResolutionStatus::Active,
                owners: vec![OwnershipNode {
                    id: OwnerId::Homebrew,
                    package: Some("node".into()),
                    version: Some("25".into()),
                    confidence: Confidence::Confirmed,
                    evidence: vec![],
                    actions: ActionGuide::default(),
                }],
            }],
        }
    }

    #[test]
    fn list_and_inspect_read_the_same_graph() {
        let graph = graph();
        let mut list = Vec::new();
        let mut inspect = Vec::new();
        print_list(std::slice::from_ref(&graph), false, &mut list).unwrap();
        print_inspect(&[graph], false, &mut inspect).unwrap();
        let list = String::from_utf8(list).unwrap();
        let inspect = String::from_utf8(inspect).unwrap();
        assert!(list.contains("Homebrew [confirmed]"));
        assert!(inspect.contains("Homebrew [confirmed]"));
    }

    #[test]
    fn json_contains_common_graph_fields() {
        let mut json = Vec::new();
        print_json(&[graph()], &mut json).unwrap();
        let json = String::from_utf8(json).unwrap();
        assert!(json.contains("\"resolutions\""));
        assert!(json.contains("\"ownership_chain\""));
        assert!(json.contains("\"action_guide\""));
    }

    #[test]
    fn json_separates_the_stable_owner_id_from_the_display_name() {
        let mut json = Vec::new();
        print_json(&[graph()], &mut json).unwrap();
        let json = String::from_utf8(json).unwrap();
        assert!(json.contains("\"id\": \"homebrew\""));
        assert!(json.contains("\"name\": \"Homebrew\""));
    }

    #[test]
    fn text_output_renders_display_names_not_stable_ids() {
        let graph = OwnershipGraph {
            command: "java".into(),
            resolutions: vec![Resolution {
                path: PathBuf::from("/home/me/.sdkman/candidates/java/21/bin/java"),
                real_path: PathBuf::from("/home/me/.sdkman/candidates/java/21/bin/java"),
                status: ResolutionStatus::Active,
                owners: vec![OwnershipNode {
                    id: OwnerId::Sdkman,
                    package: Some("java".into()),
                    version: Some("21".into()),
                    confidence: Confidence::Confirmed,
                    evidence: vec![],
                    actions: ActionGuide::default(),
                }],
            }],
        };
        let mut inspect = Vec::new();
        print_inspect(&[graph], true, &mut inspect).unwrap();
        let inspect = String::from_utf8(inspect).unwrap();
        assert!(inspect.contains("SDKMAN! [confirmed]"));
        assert!(inspect.contains("owner[1]: SDKMAN! (version_manager, confirmed)"));
    }

    #[test]
    fn escapes_json_control_characters() {
        assert_eq!(escape("a\"b\\c\n"), "a\\\"b\\\\c\\n");
    }
}
