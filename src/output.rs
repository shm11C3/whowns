use std::io::{self, Write};

use crate::model::{
    ActionGuide, Confidence, OwnershipGraph, OwnershipNode, Resolution, ResolutionStatus,
};

const RESET: &str = "\x1b[0m";
const BOLD_CYAN: &str = "\x1b[1;36m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";

pub fn print_inspect(
    graphs: &[OwnershipGraph],
    explain: bool,
    color: bool,
    mut out: impl Write,
) -> io::Result<()> {
    for (graph_index, graph) in graphs.iter().enumerate() {
        if graph_index > 0 {
            writeln!(out)?;
        }
        writeln!(out, "{}", styled(&graph.command, BOLD_CYAN, color))?;
        if graph.resolutions.is_empty() {
            writeln!(out, "└── {}", styled("? not found in PATH", RED, color))?;
            continue;
        }
        for (index, resolution) in graph.resolutions.iter().enumerate() {
            print_resolution(
                &mut out,
                graph,
                resolution,
                explain,
                color,
                index + 1 == graph.resolutions.len(),
            )?;
        }
    }
    Ok(())
}

pub fn print_list(
    graphs: &[OwnershipGraph],
    explain: bool,
    color: bool,
    mut out: impl Write,
) -> io::Result<()> {
    let command_width = graphs
        .iter()
        .map(|graph| graph.command.len())
        .max()
        .unwrap_or(7)
        .max(7);
    let header = format!(
        "{:<command_width$}  {:<10}  {:<9}  OWNER CHAIN",
        "COMMAND", "CONFIDENCE", "SHADOWED"
    );
    writeln!(out, "{}", styled(&header, BOLD_CYAN, color))?;
    for graph in graphs {
        let Some(active) = graph.active() else {
            let command = format!("{:<command_width$}", graph.command);
            let confidence = format!("{:<10}", "unknown");
            writeln!(
                out,
                "{}  {}  {:<9}  {}",
                styled(&command, BOLD_CYAN, color),
                styled(&confidence, RED, color),
                "0",
                styled("not found", RED, color)
            )?;
            continue;
        };
        let confidence = active
            .primary_owner()
            .map(OwnershipNode::confidence)
            .unwrap_or(Confidence::Unknown);
        let command = format!("{:<command_width$}", graph.command);
        let confidence_column = format!("{:<10}", confidence.as_str());
        writeln!(
            out,
            "{}  {}  {:<9}  {}",
            styled(&command, BOLD_CYAN, color),
            styled_confidence(&confidence_column, confidence, color),
            graph.shadowed_count(),
            owner_chain(active, color)
        )?;
    }

    if explain && !graphs.is_empty() {
        writeln!(out, "\n{}\n", styled("Details", BOLD_CYAN, color))?;
        print_inspect(graphs, true, color, &mut out)?;
    }
    Ok(())
}

fn print_resolution(
    mut out: impl Write,
    graph: &OwnershipGraph,
    resolution: &Resolution,
    explain: bool,
    color: bool,
    last_resolution: bool,
) -> io::Result<()> {
    let (status_marker, status_color) = match resolution.status {
        ResolutionStatus::Active => ("●", GREEN),
        ResolutionStatus::Shadowed => ("○", YELLOW),
    };
    let status = format!("{status_marker} {}", resolution.status.as_str());
    writeln!(
        out,
        "{} {}",
        connector(last_resolution),
        styled(&status, status_color, color)
    )?;
    let prefix = child_prefix("", last_resolution);
    write_leaf(
        &mut out,
        &prefix,
        false,
        "executable",
        &resolution.path.to_string_lossy(),
    )?;
    if resolution.path != resolution.real_path {
        write_leaf(
            &mut out,
            &prefix,
            false,
            "resolves to",
            &resolution.real_path.to_string_lossy(),
        )?;
    }

    let primary = resolution.primary_owner();
    let has_actions = primary.is_some_and(|owner| !action_entries(&owner.actions).is_empty());
    let show_hint = !explain && resolution.owners.len() > 1;
    let has_tail = explain || has_actions || show_hint;
    write_leaf(
        &mut out,
        &prefix,
        !has_tail,
        "ownership",
        &format!("{} → {}", graph.command, owner_chain(resolution, color)),
    )?;

    if explain {
        print_owner_details(&mut out, &prefix, true, &resolution.owners, color)?;
    } else {
        if let Some(primary) = primary.filter(|_| has_actions) {
            print_action_group(
                &mut out,
                &prefix,
                !show_hint,
                &format!("actions ({})", primary.display_name()),
                &primary.actions,
            )?;
        }
        if show_hint {
            write_leaf(
                &mut out,
                &prefix,
                true,
                "hint",
                "use --explain to expand ownership evidence",
            )?;
        }
    }
    Ok(())
}

fn owner_chain(resolution: &Resolution, color: bool) -> String {
    if resolution.owners.is_empty() {
        return format!(
            "unconfirmed owner [{}]",
            styled_confidence("unknown", Confidence::Unknown, color)
        );
    }
    resolution
        .owners
        .iter()
        .map(|owner| {
            let confidence = owner.confidence();
            format!(
                "{} [{}]",
                owner.display_name(),
                styled_confidence(confidence.as_str(), confidence, color)
            )
        })
        .collect::<Vec<_>>()
        .join(" → ")
}

fn styled(value: &str, style: &str, color: bool) -> String {
    if color {
        format!("{style}{value}{RESET}")
    } else {
        value.into()
    }
}

fn styled_confidence(value: &str, confidence: Confidence, color: bool) -> String {
    let style = match confidence {
        Confidence::Confirmed => GREEN,
        Confidence::Probable => YELLOW,
        Confidence::Unknown => RED,
    };
    styled(value, style, color)
}

fn connector(last: bool) -> &'static str {
    if last { "└──" } else { "├──" }
}

fn child_prefix(prefix: &str, last: bool) -> String {
    format!("{prefix}{}", if last { "    " } else { "│   " })
}

fn write_leaf(
    out: &mut impl Write,
    prefix: &str,
    last: bool,
    label: &str,
    value: &str,
) -> io::Result<()> {
    writeln!(out, "{prefix}{} {label}: {value}", connector(last))
}

fn action_entries(actions: &ActionGuide) -> Vec<(&'static str, &str)> {
    [
        ("inspect", actions.inspect.as_deref()),
        ("update", actions.update.as_deref()),
        ("remove", actions.remove.as_deref()),
        ("note", actions.note.as_deref()),
    ]
    .into_iter()
    .filter_map(|(label, value)| value.map(|value| (label, value)))
    .collect()
}

fn print_action_group(
    out: &mut impl Write,
    prefix: &str,
    last: bool,
    label: &str,
    actions: &ActionGuide,
) -> io::Result<()> {
    writeln!(out, "{prefix}{} {label}", connector(last))?;
    let prefix = child_prefix(prefix, last);
    let entries = action_entries(actions);
    for (index, (label, value)) in entries.iter().enumerate() {
        write_leaf(out, &prefix, index + 1 == entries.len(), label, value)?;
    }
    Ok(())
}

fn print_owner_details(
    out: &mut impl Write,
    prefix: &str,
    last: bool,
    owners: &[OwnershipNode],
    color: bool,
) -> io::Result<()> {
    writeln!(out, "{prefix}{} owner details", connector(last))?;
    let prefix = child_prefix(prefix, last);
    for (index, owner) in owners.iter().enumerate() {
        print_owner(out, &prefix, index + 1 == owners.len(), owner, color)?;
    }
    Ok(())
}

fn print_owner(
    out: &mut impl Write,
    prefix: &str,
    last: bool,
    owner: &OwnershipNode,
    color: bool,
) -> io::Result<()> {
    let confidence = owner.confidence();
    writeln!(
        out,
        "{prefix}{} {} [{}]",
        connector(last),
        owner.display_name(),
        styled_confidence(confidence.as_str(), confidence, color)
    )?;
    let prefix = child_prefix(prefix, last);
    let actions = action_entries(&owner.actions);
    let mut remaining = 1
        + usize::from(owner.package.is_some())
        + usize::from(owner.version.is_some())
        + usize::from(!owner.evidence.is_empty())
        + usize::from(!actions.is_empty());

    remaining -= 1;
    write_leaf(out, &prefix, remaining == 0, "kind", owner.kind().as_str())?;
    if let Some(package) = &owner.package {
        remaining -= 1;
        write_leaf(out, &prefix, remaining == 0, "package", package)?;
    }
    if let Some(version) = &owner.version {
        remaining -= 1;
        write_leaf(out, &prefix, remaining == 0, "version", version)?;
    }
    if !owner.evidence.is_empty() {
        remaining -= 1;
        let last = remaining == 0;
        writeln!(out, "{prefix}{} evidence", connector(last))?;
        let evidence_prefix = child_prefix(&prefix, last);
        for (index, evidence) in owner.evidence.iter().enumerate() {
            write_leaf(
                out,
                &evidence_prefix,
                index + 1 == owner.evidence.len(),
                evidence.source(),
                &evidence.detail,
            )?;
        }
    }
    if !actions.is_empty() {
        print_action_group(out, &prefix, true, "actions", &owner.actions)?;
    }
    Ok(())
}

const JSON_SCHEMA_VERSION: u8 = 1;

pub fn print_json(graphs: &[OwnershipGraph], mut out: impl Write) -> io::Result<()> {
    writeln!(out, "{{")?;
    writeln!(out, "  \"schema_version\": {JSON_SCHEMA_VERSION},")?;
    writeln!(out, "  \"graphs\": [")?;
    for (graph_index, graph) in graphs.iter().enumerate() {
        writeln!(out, "    {{")?;
        writeln!(out, "      \"command\": \"{}\",", escape(&graph.command))?;
        writeln!(out, "      \"resolutions\": [")?;
        for (resolution_index, resolution) in graph.resolutions.iter().enumerate() {
            print_resolution_json(
                &mut out,
                resolution,
                resolution_index + 1 == graph.resolutions.len(),
            )?;
        }
        writeln!(out, "      ]")?;
        let comma = if graph_index + 1 == graphs.len() {
            ""
        } else {
            ","
        };
        writeln!(out, "    }}{comma}")?;
    }
    writeln!(out, "  ]")?;
    writeln!(out, "}}")
}

fn print_resolution_json(
    mut out: impl Write,
    resolution: &Resolution,
    last: bool,
) -> io::Result<()> {
    writeln!(out, "        {{")?;
    json_string_field(&mut out, 10, "status", resolution.status.as_str(), true)?;
    json_string_field(
        &mut out,
        10,
        "path",
        &resolution.path.to_string_lossy(),
        true,
    )?;
    json_string_field(
        &mut out,
        10,
        "real_path",
        &resolution.real_path.to_string_lossy(),
        true,
    )?;
    writeln!(out, "          \"ownership_chain\": [")?;
    for (owner_index, owner) in resolution.owners.iter().enumerate() {
        print_owner_json(&mut out, owner, owner_index + 1 == resolution.owners.len())?;
    }
    writeln!(out, "          ]")?;
    writeln!(out, "        }}{}", if last { "" } else { "," })
}

fn print_owner_json(mut out: impl Write, owner: &OwnershipNode, last: bool) -> io::Result<()> {
    writeln!(out, "            {{")?;
    json_string_field(&mut out, 14, "id", owner.id.as_str(), true)?;
    json_string_field(&mut out, 14, "name", owner.display_name(), true)?;
    json_string_field(&mut out, 14, "kind", owner.kind().as_str(), true)?;
    json_optional_field(&mut out, 14, "package", owner.package.as_deref(), true)?;
    json_optional_field(&mut out, 14, "version", owner.version.as_deref(), true)?;
    json_string_field(
        &mut out,
        14,
        "confidence",
        owner.confidence().as_str(),
        true,
    )?;
    writeln!(out, "              \"evidence\": [")?;
    for (index, evidence) in owner.evidence.iter().enumerate() {
        writeln!(out, "                {{")?;
        json_string_field(&mut out, 18, "source", evidence.source(), true)?;
        json_string_field(&mut out, 18, "detail", &evidence.detail, false)?;
        writeln!(
            out,
            "                }}{}",
            if index + 1 == owner.evidence.len() {
                ""
            } else {
                ","
            }
        )?;
    }
    writeln!(out, "              ],")?;
    writeln!(out, "              \"action_guide\": {{")?;
    json_optional_field(
        &mut out,
        16,
        "inspect",
        owner.actions.inspect.as_deref(),
        true,
    )?;
    json_optional_field(
        &mut out,
        16,
        "update",
        owner.actions.update.as_deref(),
        true,
    )?;
    json_optional_field(
        &mut out,
        16,
        "remove",
        owner.actions.remove.as_deref(),
        true,
    )?;
    json_optional_field(&mut out, 16, "note", owner.actions.note.as_deref(), false)?;
    writeln!(out, "              }}")?;
    writeln!(out, "            }}{}", if last { "" } else { "," })
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
    use std::process::{Command, Stdio};

    use std::path::PathBuf;

    use super::*;
    use crate::model::{Evidence, EvidenceKind, OwnerId, OwnershipNode, ResolutionStatus};

    fn graph() -> OwnershipGraph {
        OwnershipGraph {
            command: "node".into(),
            resolutions: vec![Resolution {
                path: PathBuf::from("/opt/homebrew/bin/node"),
                real_path: PathBuf::from("/opt/homebrew/Cellar/node/25/bin/node"),
                status: ResolutionStatus::Active,
                owners: vec![
                    OwnershipNode::new(
                        OwnerId::Nvm,
                        Some("node".into()),
                        Some("22.3.0".into()),
                        vec![Evidence::new(
                            EvidenceKind::ManagerQueryMatch,
                            "nvm query matches the selected runtime",
                        )],
                        ActionGuide {
                            inspect: Some("nvm current".into()),
                            update: Some("nvm install <new-version>".into()),
                            ..ActionGuide::default()
                        },
                    ),
                    OwnershipNode::new(
                        OwnerId::Homebrew,
                        Some("nvm".into()),
                        Some("0.40.3".into()),
                        vec![Evidence::new(
                            EvidenceKind::PackageDatabaseOwnership,
                            "Homebrew registry owns the nvm root",
                        )],
                        ActionGuide {
                            inspect: Some("brew info nvm".into()),
                            ..ActionGuide::default()
                        },
                    ),
                ],
            }],
        }
    }

    fn assert_representative_json_document(json: &[u8]) {
        let mut child = Command::new("python3")
            .args([
                "-c",
                r#"
import json
import sys

document = json.load(sys.stdin)
assert type(document) is dict
assert type(document.get("schema_version")) is int
assert document["schema_version"] == 1
assert type(document.get("graphs")) is list
assert len(document["graphs"]) == 1

graph = document["graphs"][0]
assert graph["command"] == "node"
assert type(graph.get("resolutions")) is list
assert len(graph["resolutions"]) == 1

resolution = graph["resolutions"][0]
assert resolution["status"] == "active"
assert type(resolution.get("ownership_chain")) is list
assert len(resolution["ownership_chain"]) == 2

nvm, homebrew = resolution["ownership_chain"]
assert nvm["id"] == "nvm"
assert nvm["name"] == "nvm"
assert homebrew["id"] == "homebrew"
assert homebrew["name"] == "Homebrew"
"#,
            ])
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("python3 is required to validate JSON in tests");
        child.stdin.as_mut().unwrap().write_all(json).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "invalid JSON: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn list_and_inspect_read_the_same_graph() {
        let graph = graph();
        let mut list = Vec::new();
        let mut inspect = Vec::new();
        print_list(std::slice::from_ref(&graph), false, false, &mut list).unwrap();
        print_inspect(&[graph], false, false, &mut inspect).unwrap();
        let list = String::from_utf8(list).unwrap();
        let inspect = String::from_utf8(inspect).unwrap();
        assert!(list.contains("Homebrew [confirmed]"));
        assert!(inspect.contains("Homebrew [confirmed]"));
    }

    #[test]
    fn inspect_renders_a_compact_ownership_tree() {
        let mut output = Vec::new();
        print_inspect(&[graph()], false, false, &mut output).unwrap();

        assert_eq!(
            String::from_utf8(output).unwrap(),
            concat!(
                "node\n",
                "└── ● active\n",
                "    ├── executable: /opt/homebrew/bin/node\n",
                "    ├── resolves to: /opt/homebrew/Cellar/node/25/bin/node\n",
                "    ├── ownership: node → nvm [confirmed] → Homebrew [confirmed]\n",
                "    ├── actions (nvm)\n",
                "    │   ├── inspect: nvm current\n",
                "    │   └── update: nvm install <new-version>\n",
                "    └── hint: use --explain to expand ownership evidence\n",
            )
        );
    }

    #[test]
    fn explain_expands_owner_evidence_as_tree_branches() {
        let mut output = Vec::new();
        print_inspect(&[graph()], true, false, &mut output).unwrap();
        let output = String::from_utf8(output).unwrap();

        assert!(output.contains(concat!(
            "    └── owner details\n",
            "        ├── nvm [confirmed]\n",
            "        │   ├── kind: version_manager\n",
            "        │   ├── package: node\n",
            "        │   ├── version: 22.3.0\n",
            "        │   ├── evidence\n",
            "        │   │   └── manager query: nvm query matches the selected runtime\n",
            "        │   └── actions\n",
            "        │       ├── inspect: nvm current\n",
            "        │       └── update: nvm install <new-version>\n",
            "        └── Homebrew [confirmed]",
        )));
    }

    #[test]
    fn json_document_is_versioned_and_has_stable_owner_identities() {
        let mut json = Vec::new();
        print_json(&[graph()], &mut json).unwrap();
        assert_representative_json_document(&json);
    }

    #[test]
    fn text_output_renders_display_names_not_stable_ids() {
        let graph = OwnershipGraph {
            command: "java".into(),
            resolutions: vec![Resolution {
                path: PathBuf::from("/home/me/.sdkman/candidates/java/21/bin/java"),
                real_path: PathBuf::from("/home/me/.sdkman/candidates/java/21/bin/java"),
                status: ResolutionStatus::Active,
                owners: vec![OwnershipNode::new(
                    OwnerId::Sdkman,
                    Some("java".into()),
                    Some("21".into()),
                    vec![Evidence::new(
                        EvidenceKind::ManagedPathLayout,
                        "path matches the SDKMAN! layout",
                    )],
                    ActionGuide::default(),
                )],
            }],
        };
        let mut inspect = Vec::new();
        print_inspect(&[graph], true, false, &mut inspect).unwrap();
        let inspect = String::from_utf8(inspect).unwrap();
        assert!(inspect.contains("SDKMAN! [probable]"));
        assert!(inspect.contains("kind: version_manager"));
    }

    #[test]
    fn colored_output_highlights_status_and_confidence() {
        let mut output = Vec::new();
        print_inspect(&[graph()], false, true, &mut output).unwrap();
        let output = String::from_utf8(output).unwrap();

        assert!(output.contains("\x1b[1;36mnode\x1b[0m"));
        assert!(output.contains("\x1b[32m● active\x1b[0m"));
        assert!(output.contains("[\x1b[32mconfirmed\x1b[0m]"));
    }

    #[test]
    fn json_control_characters_round_trip_through_a_parser() {
        let command = "a\"b\\c\n\u{0001}";
        let graph = OwnershipGraph {
            command: command.into(),
            resolutions: vec![],
        };
        let mut json = Vec::new();
        print_json(&[graph], &mut json).unwrap();

        let mut child = Command::new("python3")
            .args([
                "-c",
                concat!(
                    "import json, sys; ",
                    "document = json.load(sys.stdin); ",
                    "assert document['graphs'][0]['command'] == sys.argv[1]",
                ),
                command,
            ])
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("python3 is required to validate JSON in tests");
        child.stdin.as_mut().unwrap().write_all(&json).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "JSON value did not round trip: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
