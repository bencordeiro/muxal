//! Pure decoding migrations for retired persisted features. Browser data is
//! recognized only at this boundary; it can never become a runnable pane/preset.

use crate::{PaneNode, Settings, Uuid, Workspace, remove};
use serde_json::Value;
use std::collections::HashSet;

/// Read an old or current workspace, removing retired web panes through the
/// normal pane close operation so surviving tabs and split weights stay valid.
/// No URLs are opened and no files are written by decoding.
pub fn workspace_from_json(text: &str) -> Result<Workspace, serde_json::Error> {
    let mut value: Value = serde_json::from_str(text)?;
    let mut retired = HashSet::new();
    if let Some(instances) = value.get_mut("instances").and_then(Value::as_array_mut) {
        for instance in instances.iter().filter(|i| i["kind"] == "Browser") {
            retired.insert(serde_json::from_value::<Uuid>(instance["id"].clone())?);
        }
        instances.retain(|i| i["kind"] != "Browser");
    }
    if !retired.is_empty()
        && let Some(projects) = value.get_mut("projects").and_then(Value::as_array_mut)
    {
        for project in projects {
            if !project.is_object() {
                // Let normal deserialization report malformed projects below.
                continue;
            }
            let mut layout: Option<PaneNode> = serde_json::from_value(project["layout"].clone())?;
            // Use layout order rather than HashSet iteration: multiple closes
            // must always redistribute spans in the same deterministic order.
            let ids = layout
                .as_ref()
                .map(PaneNode::collect_instances)
                .unwrap_or_default();
            for id in ids.into_iter().filter(|id| retired.contains(id)) {
                remove(&mut layout, id);
            }
            project["layout"] = serde_json::to_value(layout)?;
            if project["last_focused_instance"]
                .as_str()
                .and_then(|id| Uuid::parse_str(id).ok())
                .is_some_and(|id| retired.contains(&id))
            {
                project["last_focused_instance"] = Value::Null;
            }
        }
    }
    serde_json::from_value(value)
}

/// Retire built-in and custom web presets without replacing the rest of a
/// user's settings with defaults. Old terminal presets may omit `kind`.
pub fn settings_from_toml(text: &str) -> Result<Settings, toml::de::Error> {
    let mut value: toml::Value = toml::from_str(text)?;
    let mut retired = HashSet::new();
    if let Some(presets) = value.get_mut("presets").and_then(toml::Value::as_array_mut) {
        for preset in presets
            .iter()
            .filter(|p| p.get("kind").and_then(toml::Value::as_str) == Some("Browser"))
        {
            for key in ["id", "name"] {
                if let Some(id) = preset.get(key).and_then(toml::Value::as_str) {
                    retired.insert(id.to_string());
                }
            }
        }
        presets.retain(|p| p.get("kind").and_then(toml::Value::as_str) != Some("Browser"));
        // Older defaults could store a name. Preserve a legitimate terminal
        // with the same label as a retired web preset.
        for preset in presets {
            if let Some(name) = preset.get("name").and_then(toml::Value::as_str) {
                retired.remove(name);
            }
        }
    }
    if let Some(table) = value.as_table_mut() {
        table.remove("browser_enabled");
        if table
            .get("default_preset")
            .and_then(toml::Value::as_str)
            .is_some_and(|id| retired.contains(id))
        {
            table.insert("default_preset".into(), toml::Value::String(String::new()));
        }
        for key in ["runners", "loops"] {
            if let Some(items) = table.get_mut(key).and_then(toml::Value::as_array_mut) {
                for item in items {
                    if item
                        .get("preset_id")
                        .and_then(toml::Value::as_str)
                        .is_some_and(|id| retired.contains(id))
                        && let Some(item) = item.as_table_mut()
                    {
                        item.remove("preset_id");
                        // Do not silently send scheduled work to a different agent.
                        if key == "loops" {
                            item.insert("enabled".into(), toml::Value::Boolean(false));
                        }
                    }
                }
            }
        }
    }
    value.try_into()
}

#[cfg(test)]
mod tests {
    use super::{settings_from_toml, workspace_from_json};
    use crate::{
        AgentPreset, Instance, PaneNode, Project, Settings, SplitDirection, Uuid, Workspace,
        add_tab,
    };
    use serde_json::json;

    #[test]
    fn retired_panes_preserve_other_content_tabs_weights_and_project_state() {
        let mut p = Project::new("repo", "/repo");
        let terminal = Instance::from_preset(p.id, &AgentPreset::shell());
        let editor = Instance::editor(p.id, Some("/repo/page.html".into()));
        let spider = Instance::spider(p.id, Default::default());
        let retired = Uuid::new_v4();
        let retired_tab = Uuid::new_v4();
        let mut tabs = Some(PaneNode::leaf(terminal.id));
        add_tab(&mut tabs, terminal.id, retired_tab);
        p.layout = Some(PaneNode::Split {
            direction: SplitDirection::Horizontal,
            sizes: vec![5.0, 2.0, 1.0, 4.0],
            children: vec![
                tabs.unwrap(),
                PaneNode::leaf(editor.id),
                PaneNode::leaf(retired),
                PaneNode::leaf(spider.id),
            ],
        });
        if let Some(PaneNode::Split { children, .. }) = &mut p.layout
            && let PaneNode::Leaf(leaf) = &mut children[0]
        {
            leaf.active = 1;
        }
        p.last_focused_instance = Some(retired);
        let mut w = Workspace::default();
        w.add_project(p.clone());
        w.pin_project(p.id);
        w.instances = vec![terminal.clone(), editor.clone(), spider.clone()];
        let mut value = serde_json::to_value(w).unwrap();
        for id in [retired, retired_tab] {
            value["instances"].as_array_mut().unwrap().push(json!({"id":id,"project_id":p.id,"kind":"Browser","browser_url":"https://example.com"}));
        }
        let restored = workspace_from_json(&value.to_string()).unwrap();
        assert_eq!(restored.instances.len(), 3);
        assert_eq!(restored.active_project, Some(p.id));
        assert_eq!(restored.pin_slot(p.id), Some(0));
        assert_eq!(
            restored.instance(editor.id).unwrap().editor_path,
            editor.editor_path
        );
        let project = restored.project(p.id).unwrap();
        assert_eq!(project.last_focused_instance, None);
        assert_eq!(project.preferred_instance(), Some(terminal.id));
        let Some(PaneNode::Split {
            sizes, children, ..
        }) = &project.layout
        else {
            panic!("split lost")
        };
        assert_eq!(sizes, &[5.0, 3.0, 4.0]);
        let PaneNode::Leaf(leaf) = &children[0] else {
            panic!("tabs lost")
        };
        assert_eq!(leaf.tabs, vec![terminal.id]);
        assert_eq!(leaf.active, 0);
        let once = serde_json::to_string(&restored).unwrap();
        assert!(!once.contains("browser_url"));
        assert_eq!(
            serde_json::to_value(workspace_from_json(&once).unwrap()).unwrap(),
            serde_json::to_value(restored).unwrap()
        );
    }

    #[test]
    fn browser_only_project_becomes_empty_and_unknown_kinds_still_fail() {
        let id = Uuid::new_v4();
        let mut p = Project::new("repo", "/repo");
        p.layout = Some(PaneNode::leaf(id));
        p.last_focused_instance = Some(id);
        let mut value =
            json!({"projects":[p],"active_project":p.id,"instances":[{"id":id,"kind":"Browser"}]});
        let restored = workspace_from_json(&value.to_string()).unwrap();
        assert!(restored.projects[0].layout.is_none());
        assert!(restored.instances.is_empty());
        value["instances"][0]["kind"] = json!("FuturePane");
        assert!(workspace_from_json(&value.to_string()).is_err());
        value["instances"][0]["kind"] = json!("Browser");
        value["projects"] = json!(["invalid project"]);
        assert!(workspace_from_json(&value.to_string()).is_err());
    }

    #[test]
    fn retired_presets_settings_and_scheduled_bindings_do_not_reset_other_settings() {
        // A terminal legitimately named Browser is not a web preset.
        let mut settings = Settings {
            presets: vec![AgentPreset::shell()],
            theme: "Clay Dark".into(),
            font_size: 19.0,
            ..Settings::default()
        };
        settings.presets[0].name = "Browser".into();
        let mut value = toml::Value::try_from(settings).unwrap();
        let id = Uuid::new_v4().to_string();
        value["presets"].as_array_mut().unwrap().push(
            toml::Value::try_from(
                json!({"id":id,"name":"Old web","kind":"Browser","url":"https://example.com"}),
            )
            .unwrap(),
        );
        value
            .as_table_mut()
            .unwrap()
            .insert("browser_enabled".into(), toml::Value::Boolean(true));
        value["default_preset"] = toml::Value::String(id.clone());
        value["runners"] =
            toml::Value::try_from(json!([{"name":"Task","preset_id":id,"prompt":"keep this"}]))
                .unwrap();
        value["loops"] = toml::Value::try_from(json!([{"name":"Timer","project_id":Uuid::new_v4(),"preset_id":id,"enabled":true,"prompt":"keep this too"}])).unwrap();
        let restored = settings_from_toml(&toml::to_string(&value).unwrap()).unwrap();
        assert_eq!(restored.theme, "Clay Dark");
        assert_eq!(restored.font_size, 19.0);
        assert_eq!(restored.presets.len(), 1);
        assert_eq!(restored.presets[0].name, "Browser");
        assert!(restored.default_preset.is_empty());
        assert_eq!(restored.runners[0].preset_id, None);
        assert_eq!(restored.runners[0].prompt, "keep this");
        assert!(!restored.loops[0].enabled);
        assert_eq!(restored.loops[0].preset_id, None);
        assert_eq!(restored.loops[0].prompt, "keep this too");
        value["default_preset"] = toml::Value::String("Old web".into());
        assert!(
            settings_from_toml(&toml::to_string(&value).unwrap())
                .unwrap()
                .default_preset
                .is_empty()
        );
        value["presets"][1]["name"] = toml::Value::String("Browser".into());
        value["default_preset"] = toml::Value::String("Browser".into());
        assert_eq!(
            settings_from_toml(&toml::to_string(&value).unwrap())
                .unwrap()
                .default_preset,
            "Browser"
        );
        let current = toml::to_string(&restored).unwrap();
        assert!(!current.contains("browser_enabled"));
        assert_eq!(
            settings_from_toml(&current).unwrap().presets,
            restored.presets
        );
    }
}
