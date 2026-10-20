use super::*;

fn label(action: &str) -> SharedString {
    if let Some(number) = action.strip_prefix("JumpToTab") {
        return tf("Switch to tab {number}", &[("number", number)]).into();
    }
    if let Some(number) = action.strip_prefix("JumpToProject") {
        return tf("Switch to project {number}", &[("number", number)]).into();
    }
    if let Some(number) = action.strip_prefix("NewAgent") {
        return tf("Open agent preset {number}", &[("number", number)]).into();
    }
    match action {
        "NewPane" => t("New pane"),
        "NewTab" => t("New tab"),
        "TabNext" => t("Next tab"),
        "TabPrev" => t("Previous tab"),
        "SplitRight" => t("Split right"),
        "SplitDown" => t("Split down"),
        "ClosePane" => t("Close pane"),
        "FocusNext" => t("Focus next pane"),
        "FocusPrev" => t("Focus previous pane"),
        "FocusLeft" => t("Focus pane on left"),
        "FocusRight" => t("Focus pane on right"),
        "FocusUp" => t("Focus pane above"),
        "FocusDown" => t("Focus pane below"),
        "ZoomIn" => t("Zoom in"),
        "ZoomOut" => t("Zoom out"),
        "ToggleSidebar" => t("Toggle sidebar"),
        "ToggleDashboard" => t("Toggle dashboard"),
        "ToggleSettings" => t("Open settings"),
        "GlobalSearch" => t("Command palette"),
        "FindInProject" => t("Find in project"),
        "SearchTerminal" => t("Search terminal"),
        "ClearTerminal" => t("Clear terminal history"),
        "FocusAttention" => t("Focus agent needing attention"),
        "ShowKeys" => t("Keyboard shortcuts"),
        "ToggleBroadcast" => t("Toggle broadcast input"),
        "ToggleWorktree" => t("Toggle worktree for new agents"),
        "ToggleFullScreen" => t("Toggle full screen"),
        "ToggleDevConsole" => t("Toggle developer console"),
        "SaveFile" => t("Save file"),
        "SaveFileAs" => t("Save file as"),
        "CycleProjects" => t("Next pinned project"),
        "CycleProjectsPrev" => t("Previous pinned project"),
        _ => action.to_string().into(),
    }
}

pub(super) fn retain_component_bindings(map: &gpui::Keymap) -> Vec<gpui::KeyBinding> {
    map.bindings()
        .filter(|binding| !binding.action().name().starts_with("muxal::"))
        .cloned()
        .collect()
}

impl MuxalApp {
    pub(super) fn render_settings_keybindings(
        &self,
        content_w: Pixels,
        height: Pixels,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let form_w = (content_w - px(32.0)).max(px(280.0));
        let input_w = (form_w * 0.42).clamp(px(120.0), px(210.0));
        let modified_only = self.settings_ui.keys_modified_only;
        let mut rows = v_flex().gap_1().w(form_w);
        let mut shown = 0;
        for (name, input) in &self.settings_ui.keybinds {
            let Some((_, default, _)) = settings_view::DEFAULT_KEYBINDINGS
                .iter()
                .find(|(id, _, _)| *id == name)
            else {
                continue;
            };
            let modified = muxal_core::keybindings::modified(&input.read(cx).value(), default);
            let numbered = name.starts_with("JumpTo") || name.starts_with("NewAgent");
            if modified_only && !modified {
                continue;
            }
            if !modified_only && numbered && !self.settings_ui.keys_numbered_open {
                continue;
            }
            let reset = input.clone();
            let default = *default;
            let mut row = div()
                .flex()
                .items_center()
                .gap_2()
                .w(form_w)
                .child(div().flex_1().min_w_0().text_sm().child(label(name)))
                .child(
                    div()
                        .w(input_w)
                        .flex_none()
                        .child(Input::new(input).small()),
                );
            if modified {
                row = row.child(
                    Button::new(SharedString::from(format!("reset-{name}")))
                        .ghost()
                        .xsmall()
                        .label("↺")
                        .tooltip(t("Reset shortcut"))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            // Modified-only may remove this row; focus a live element first.
                            window.focus(&this.focus_handle, cx);
                            reset.update(cx, |input, cx| input.set_value(default, window, cx));
                            cx.notify();
                        })),
                );
            } else {
                row = row.child(div().w(px(24.0)).flex_none());
            }
            rows = rows.child(row);
            shown += 1;
        }
        if shown == 0 {
            rows = rows.child(div().py_3().text_sm().child(t("No modified shortcuts")));
        }
        let mut form = v_flex()
            .gap_2()
            .w(form_w)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                Checkbox::new("keys-modified")
                                    .checked(modified_only)
                                    .on_click(cx.listener(|this, checked: &bool, window, cx| {
                                        window.focus(&this.focus_handle, cx);
                                        this.settings_ui.keys_modified_only = *checked;
                                        cx.notify();
                                    })),
                            )
                            .child(div().text_sm().child(t("Modified only"))),
                    )
                    .child(
                        Button::new("apply-keys")
                            .primary()
                            .small()
                            .label(t("Apply keybindings"))
                            .on_click(cx.listener(|this, _, _, cx| this.apply_keybindings(cx))),
                    ),
            )
            .child(
                Button::new("keys-numbered")
                    .ghost()
                    .small()
                    .selected(self.settings_ui.keys_numbered_open)
                    .icon(if self.settings_ui.keys_numbered_open {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    })
                    .label(t("Numbered shortcuts (tabs, projects, agent presets)"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        window.focus(&this.focus_handle, cx);
                        this.settings_ui.keys_numbered_open = !this.settings_ui.keys_numbered_open;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .id("keybinding-rows")
                    .h(height)
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(rows),
            )
            .child(
                Button::new("keys-passthrough")
                    .ghost()
                    .small()
                    .selected(self.settings_ui.keys_passthrough_open)
                    .label(t("Terminal passthrough"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        window.focus(&this.focus_handle, cx);
                        this.settings_ui.keys_passthrough_open =
                            !this.settings_ui.keys_passthrough_open;
                        cx.notify();
                    })),
            );
        if self.settings_ui.keys_passthrough_open {
            form = form.child(div().w(form_w).child(Input::new(&self.settings_ui.passthrough_keys).small()))
                .child(div().text_xs().text_color(cx.theme().muted_foreground)
                    .child(t("Comma-separated extra chords passed to the focused terminal. Plain Ctrl+letter shortcuts already yield to agents by default.")));
        }
        if let Some(error) = &self.settings_ui.key_error {
            form = form.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .child(error.clone()),
            );
        }
        form.child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(t(
                    "Examples: ctrl-shift-t, cmd-w. Reset and edits take effect when applied.",
                )),
        )
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{label, retain_component_bindings};
    #[test]
    fn reinstall_retires_old_chords_but_preserves_component_keys() {
        let map = gpui::Keymap::new(vec![
            gpui::KeyBinding::new("alt-x", crate::app::NewPane, None),
            gpui::KeyBinding::new("backspace", gpui_component::input::Backspace, Some("Input")),
        ]);
        let kept = retain_component_bindings(&map);
        assert_eq!(kept.len(), 1);
        assert!(!kept[0].action().name().starts_with("muxal::"));
    }
    #[test]
    fn every_default_has_a_friendly_label() {
        for (action, _, _) in muxal_core::keybindings::DEFAULT_KEYBINDINGS {
            assert_ne!(label(action).as_ref(), *action);
        }
    }
}
