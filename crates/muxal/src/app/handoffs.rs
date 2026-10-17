//! Manual handoff UI and file polling. A completed file only changes a button;
//! the receiving pane is created exclusively by the Start button handler.
use super::*;
use muxal_core::handoff::{HandoffRequest, retained_transfer_ids, reviewed_text};
use muxal_store::handoff as documents;

pub(super) struct HandoffUi {
    dialog: Option<HandoffDialog>,
    input: Entity<InputState>,
    polling: bool,
    last_poll: Option<Instant>,
    error: Option<String>,
    focus_return: Option<Uuid>,
}

#[derive(Clone, Copy)]
enum HandoffDialog {
    Prepare(Uuid),
    Waiting(Uuid),
    Review {
        request: Uuid,
        recipient: Option<Uuid>,
    },
}

impl HandoffUi {
    pub(super) fn new(window: &mut Window, cx: &mut Context<MuxalApp>) -> Self {
        Self {
            dialog: None,
            input: cx.new(|cx| InputState::new(window, cx).multi_line(true)),
            polling: false,
            last_poll: None,
            error: None,
            focus_return: None,
        }
    }

    pub(super) fn reset(&mut self) {
        self.dialog = None;
        self.last_poll = None;
        self.error = None;
        self.focus_return = None;
    }

    pub(super) fn is_open(&self) -> bool {
        self.dialog.is_some()
    }
}

impl MuxalApp {
    fn handoff_directory(&self) -> Option<PathBuf> {
        documents::directory(self.current_workspace?)
    }

    pub(super) fn handoff_available(&self, iid: Uuid) -> bool {
        self.settings.handoffs_enabled
            && self
                .workspace
                .instance(iid)
                .is_some_and(|i| i.kind == InstanceKind::Terminal && i.program.is_some())
            && !self.popouts.contains_key(&iid)
    }

    fn handoff_request(&self, source: Uuid) -> Option<&HandoffRequest> {
        self.workspace.handoffs.iter().find(|r| r.source == source)
    }

    pub(super) fn handoff_ready(&self, source: Uuid) -> bool {
        self.handoff_request(source)
            .is_some_and(|r| r.summary.is_some())
    }

    pub(super) fn handoff_button_label(&self, source: Uuid) -> SharedString {
        if self.handoff_ready(source) {
            t("Review")
        } else if self.handoff_request(source).is_some() {
            t("Preparing…")
        } else {
            t("Handoff")
        }
    }

    fn handoff_source(&self, request: &HandoffRequest) -> Option<&Instance> {
        let instance = self.workspace.instance(request.source)?;
        let project = self.workspace.project(instance.project_id)?;
        let cwd = instance
            .worktree_path
            .as_deref()
            .unwrap_or(&project.root_path);
        request.matches_source(instance, cwd).then_some(instance)
    }

    fn handoff_can_prepare(&self, source: Uuid, cx: &App) -> bool {
        self.handoff_available(source)
            && self.terminals.get(&source).is_some_and(|v| {
                let view = v.read(cx);
                !view.exited() && matches!(view.status(), AgentStatus::Idle | AgentStatus::Done)
            })
    }

    pub(super) fn open_handoff(
        &mut self,
        source: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // InputState belongs to the main window, including for project windows.
        if !self.is_main_window(window) {
            let handle = self.main_window;
            let this = cx.entity().downgrade();
            cx.defer(move |cx| {
                if let Some(handle) = handle {
                    let _ = handle.update(cx, |_, window, cx| {
                        window.activate_window();
                        let _ = this.update(cx, |app, cx| app.open_handoff(source, window, cx));
                    });
                }
            });
            return;
        }
        if !self.handoff_available(source) {
            return;
        }
        self.handoff_ui.focus_return = self.active_instance;
        self.handoff_ui.error = None;
        if let Some(request) = self.handoff_request(source).cloned() {
            self.handoff_ui.dialog = Some(HandoffDialog::Waiting(request.id));
            window.focus(&self.dialog_focus, cx);
            if self.handoff_ready(source) {
                self.review_handoff(request.id, window, cx);
            }
        } else {
            self.handoff_ui.dialog = Some(HandoffDialog::Prepare(source));
            self.handoff_ui
                .input
                .update(cx, |s, cx| s.set_value("", window, cx));
            window.focus(&self.handoff_ui.input.read(cx).focus_handle(cx), cx);
        }
        cx.notify();
    }

    fn handoff_error(&mut self, message: &str, cx: &mut Context<Self>) {
        self.handoff_ui.error = Some(message.to_owned());
        cx.notify();
    }

    fn prepare_handoff(&mut self, source: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        if !self.handoff_can_prepare(source, cx) || self.handoff_request(source).is_some() {
            self.handoff_error(
                &t("Wait until the agent is idle and its input is empty."),
                cx,
            );
            return;
        }
        let Some(dir) = self.handoff_directory() else {
            return;
        };
        let Some(instance) = self.workspace.instance(source) else {
            return;
        };
        let Some(project) = self.workspace.project(instance.project_id) else {
            return;
        };
        let cwd = instance
            .worktree_path
            .clone()
            .unwrap_or_else(|| project.root_path.clone());
        let direction = self.handoff_ui.input.read(cx).value().to_string();
        let request = HandoffRequest::new(instance, cwd, direction);
        if let Err(error) = std::fs::create_dir_all(&dir) {
            self.handoff_error(
                &tf(
                    "Could not save handoff: {error}",
                    &[("error", &error.to_string())],
                ),
                cx,
            );
            return;
        }
        let prompt = request.preparation_prompt(&documents::source_path(&dir, &request));
        self.workspace.handoffs.push(request);
        // Persist before sending: a restart must retain the pending request.
        if !self.try_persist() {
            self.workspace.handoffs.retain(|r| r.source != source);
            self.handoff_error(
                &t("Could not save the handoff request. Nothing was sent."),
                cx,
            );
            return;
        }
        // Preparing a summary should not be followed by an auto-continue turn.
        self.auto.remove(&source);
        let session = self.terminals[&source].read(cx).session().clone();
        // One line also works when a harness has bracketed paste disabled.
        session.paste(&prompt.split_whitespace().collect::<Vec<_>>().join(" "));
        session.mark_turn_submitted();
        session.write_input(b"\r");
        self.handoff_ui.dialog = None;
        self.handoff_ui.focus_return = None;
        self.return_handoff_focus(Some(source), window, cx);
        cx.notify();
    }

    fn review_handoff(&mut self, id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        let Some(request) = self.workspace.handoffs.iter().find(|r| r.id == id).cloned() else {
            return;
        };
        let Some(text) = request.summary.clone() else {
            return;
        };
        let default = self.handoff_source(&request).and_then(|i| i.preset_id);
        let eligible = |p: &&AgentPreset| {
            p.kind == muxal_core::PresetKind::Terminal
                && p.program.is_some()
                && self.agent_runnable(p)
        };
        let recipient = self
            .presets
            .iter()
            .filter(eligible)
            .find(|p| Some(p.id) == default)
            .or_else(|| self.presets.iter().find(eligible))
            .map(|p| p.id);
        self.handoff_ui.dialog = Some(HandoffDialog::Review {
            request: id,
            recipient,
        });
        self.handoff_ui.error = None;
        self.handoff_ui
            .input
            .update(cx, |s, cx| s.set_value(text, window, cx));
        window.focus(&self.handoff_ui.input.read(cx).focus_handle(cx), cx);
        cx.notify();
    }

    /// Closing saves edits and explicitly restores focus to a live element.
    pub(super) fn close_handoff(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(HandoffDialog::Review { request, .. }) = self.handoff_ui.dialog
            && self.workspace.handoffs.iter().any(|r| r.id == request)
            && self.save_handoff_edit(request, cx).is_none()
        {
            return;
        }
        self.handoff_ui.dialog = None;
        self.handoff_ui.error = None;
        let focus = self.handoff_ui.focus_return.take();
        self.return_handoff_focus(focus, window, cx);
        cx.notify();
    }

    /// Save reviewed text in workspace.json before discarding any transfer file.
    fn save_handoff_edit(&mut self, id: Uuid, cx: &mut Context<Self>) -> Option<String> {
        let Some(text) = reviewed_text(self.handoff_ui.input.read(cx).value().as_ref()) else {
            self.handoff_error(
                &t("The handoff must contain plain text and be no larger than 256 KiB."),
                cx,
            );
            return None;
        };
        let request = self.workspace.handoffs.iter_mut().find(|r| r.id == id)?;
        let previous = request.summary.replace(text.clone());
        if !self.try_persist() {
            if let Some(request) = self.workspace.handoffs.iter_mut().find(|r| r.id == id) {
                request.summary = previous;
            }
            self.handoff_error(
                &t("Could not save the reviewed handoff. Your edits are still open."),
                cx,
            );
            return None;
        }
        Some(text)
    }

    fn return_handoff_focus(
        &mut self,
        target: Option<Uuid>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Release the main window's input even when returning to another window.
        window.focus(&self.focus_handle, cx);
        if let Some(iid) = target.filter(|id| self.workspace.instance(*id).is_some()) {
            let pid = self.workspace.instance(iid).map(|i| i.project_id);
            let handle = self
                .secondary_windows
                .iter()
                .find(|s| Some(s.pid) == pid)
                .map(|s| s.handle);
            if let Some(handle) = handle {
                let this = cx.entity().downgrade();
                cx.defer(move |cx| {
                    let _ = handle.update(cx, |_, window, cx| {
                        window.activate_window();
                        let _ = this.update(cx, |app, cx| app.select_instance(iid, window, cx));
                    });
                });
            } else {
                self.select_instance(iid, window, cx);
            }
        } else {
            self.deselect_pane(window, cx);
        }
    }

    fn cancel_handoff(&mut self, id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.workspace.handoffs.iter().position(|r| r.id == id) {
            let request = self.workspace.handoffs.remove(index);
            if !self.try_persist() {
                self.workspace.handoffs.insert(index, request);
                self.handoff_error(&t("Could not cancel the handoff. Try again."), cx);
                return;
            }
        }
        self.handoff_ui.dialog = None;
        self.close_handoff(window, cx);
    }

    fn start_handoff(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(HandoffDialog::Review {
            request: id,
            recipient: Some(preset_id),
        }) = self.handoff_ui.dialog
        else {
            return;
        };
        if !self.settings.handoffs_enabled {
            return;
        }
        let Some(request) = self.workspace.handoffs.iter().find(|r| r.id == id).cloned() else {
            return;
        };
        if self.is_main_window(window)
            && let Some(handle) = self
                .secondary_windows
                .iter()
                .find(|s| s.pid == request.project)
                .map(|s| s.handle)
        {
            window.focus(&self.focus_handle, cx);
            let this = cx.entity().downgrade();
            cx.defer(move |cx| {
                let _ = handle.update(cx, |_, window, cx| {
                    window.activate_window();
                    let _ = this.update(cx, |app, cx| app.start_handoff(window, cx));
                });
            });
            return;
        }
        let Some(source) = self.handoff_source(&request).cloned() else {
            self.handoff_error(
                &t("The source conversation or directory changed. Prepare a new handoff."),
                cx,
            );
            return;
        };
        let Some(project) = self.workspace.project(request.project) else {
            return;
        };
        if !request.cwd.is_dir()
            || !project.instances().contains(&source.id)
            || self.popouts.contains_key(&source.id)
        {
            self.handoff_error(
                &t("The source pane or working directory is no longer available."),
                cx,
            );
            return;
        }
        let Some(preset) = self
            .presets
            .iter()
            .find(|p| p.id == preset_id && self.agent_runnable(p))
            .cloned()
        else {
            return;
        };
        let Some(summary) = self.save_handoff_edit(id, cx) else {
            return;
        };
        let instance = request.recipient(&preset, &summary);
        let iid = instance.id;
        self.handoff_ui.dialog = None;
        self.handoff_ui.focus_return = None;
        self.select_instance(source.id, window, cx);
        // Reveal the new split even when the source was maximized or the action
        // came from the dashboard's command palette.
        self.maximized = None;
        self.show_dashboard = false;
        // Consume before placement persists the workspace, so a crash cannot
        // restore both a receiving pane and an unconsumed request.
        self.workspace.handoffs.retain(|r| r.id != id);
        // Inherit explicitly even when there is no worktree. The toolbar's new-
        // worktree toggle must not put the recipient in a different checkout.
        self.place_and_spawn(
            request.project,
            instance,
            PlacementMode::Split(SplitDirection::Horizontal),
            Some(source.id),
            Some(WorktreeChoice::Inherit(source.id)),
            window,
            cx,
        );
        if self.workspace.instance(iid).is_none() {
            self.workspace.handoffs.push(request);
            self.handoff_ui.dialog = Some(HandoffDialog::Review {
                request: id,
                recipient: Some(preset_id),
            });
            self.handoff_error(&t("Could not place the receiving pane. Try again."), cx);
        }
        cx.notify();
    }

    pub(super) fn poll_handoffs(&mut self, cx: &mut Context<Self>) {
        if self.handoff_ui.polling
            || self
                .handoff_ui
                .last_poll
                .is_some_and(|at| at.elapsed() < Duration::from_secs(1))
        {
            return;
        }
        let Some(dir) = self.handoff_directory() else {
            return;
        };
        let workspace = self.current_workspace;
        let requests: Vec<_> = self
            .workspace
            .handoffs
            .iter()
            .filter(|r| r.summary.is_none())
            .cloned()
            .collect();
        self.handoff_ui.polling = true;
        self.handoff_ui.last_poll = Some(Instant::now());
        cx.spawn(async move |this, cx| {
            let (summaries, files) = cx
                .background_executor()
                .spawn(async move {
                    let summaries = requests
                        .iter()
                        .filter_map(|r| documents::read(&dir, r).ok().map(|text| (r.id, text)))
                        .collect::<Vec<_>>();
                    let files = documents::scan(&dir).unwrap_or_else(|error| {
                        log::warn!("could not scan handoff transfers: {error:#}");
                        Vec::new()
                    });
                    (summaries, files)
                })
                .await;
            let cleanup = this
                .update(cx, |this, cx| {
                    if this.current_workspace != workspace {
                        return None;
                    }
                    let mut captured = Vec::new();
                    for (id, text) in summaries {
                        if let Some(request) = this
                            .workspace
                            .handoffs
                            .iter_mut()
                            .find(|r| r.id == id && r.summary.is_none())
                        {
                            request.summary = Some(text);
                            captured.push(id);
                        }
                    }
                    // Determine retention from CURRENT state, not the worker's old
                    // snapshot: a request may have been cancelled while it was reading.
                    let retained = retained_transfer_ids(&this.workspace);
                    let removable = files.iter().any(|file| !retained.contains(&file.request));
                    if (!captured.is_empty() || removable) && !this.try_persist() {
                        for id in captured {
                            if let Some(request) =
                                this.workspace.handoffs.iter_mut().find(|r| r.id == id)
                            {
                                request.summary = None;
                            }
                        }
                        // The file is the only durable copy if saving failed.
                        cx.notify();
                        return None;
                    }
                    if !captured.is_empty() {
                        cx.notify();
                    }
                    Some((files, retained))
                })
                .ok()
                .flatten();
            if let Some((files, retained)) = cleanup {
                let result = cx
                    .background_executor()
                    .spawn(async move { documents::remove_transfers(files, &retained) })
                    .await;
                if let Err(error) = result {
                    log::warn!("could not clean up handoff transfers: {error:#}");
                }
            }
            let _ = this.update(cx, |this, _| this.handoff_ui.polling = false);
        })
        .detach();
    }

    pub(super) fn render_handoff(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(dialog) = self.handoff_ui.dialog else {
            return div().into_any_element();
        };
        let mut body = div().flex().flex_col().gap_3().min_h_0();
        let mut buttons = div().flex().justify_end().gap_2();
        let source = match dialog {
            HandoffDialog::Prepare(source) => Some(source),
            HandoffDialog::Waiting(id) | HandoffDialog::Review { request: id, .. } => self
                .workspace
                .handoffs
                .iter()
                .find(|r| r.id == id)
                .map(|r| r.source),
        };
        if let Some(source) = source.and_then(|id| self.workspace.instance(id)) {
            body = body.child(
                div()
                    .font_semibold()
                    .child(source.display_name().to_owned()),
            );
            if let Some(project) = self.workspace.project(source.project_id) {
                body = body.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(
                            source
                                .worktree_path
                                .as_deref()
                                .unwrap_or(&project.root_path)
                                .display()
                                .to_string(),
                        ),
                );
            }
        }

        let title = match dialog {
            HandoffDialog::Prepare(source) => {
                body = body.child(div().text_sm().child(t("Ask this agent to summarize the goal, decisions, changes, tests, and next steps. You will review the summary before starting another agent.")))
                    .child(div().text_sm().text_color(cx.theme().muted_foreground).child(t("Use an empty agent prompt. This sends a summary request and turns off Auto for this pane.")))
                    .child(div().text_sm().child(t("Direction for the next agent (optional)")))
                    .child(Self::wide_input(Input::new(&self.handoff_ui.input).h(px(90.0))));
                if !self.handoff_can_prepare(source, cx) {
                    body = body.child(
                        div()
                            .text_sm()
                            .child(t("Wait until the agent is idle and its input is empty.")),
                    );
                }
                buttons = buttons.child(
                    Button::new("handoff-prepare")
                        .primary()
                        .label(t("Ask agent to prepare"))
                        .disabled(!self.handoff_can_prepare(source, cx))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.prepare_handoff(source, window, cx)
                        })),
                );
                t("Prepare handoff")
            }
            HandoffDialog::Waiting(id) => {
                body = body.child(div().text_sm().child(t("The source agent is preparing the summary using a temporary transfer file. Muxal removes it after capturing the text. You can keep working and return when the pane shows Review.")))
                    .child(div().text_sm().text_color(cx.theme().muted_foreground).child(t("Cancelling forgets this request; it does not interrupt the agent.")));
                if let Some(request) = self.workspace.handoffs.iter().find(|r| r.id == id)
                    && let Some(dir) = self.handoff_directory()
                {
                    body = body.child(
                        div()
                            .text_xs()
                            .child(documents::source_path(&dir, request).display().to_string()),
                    );
                }
                buttons = buttons.child(
                    Button::new("handoff-cancel-request")
                        .ghost()
                        .label(t("Cancel request"))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.cancel_handoff(id, window, cx)
                        })),
                );
                if self
                    .workspace
                    .handoffs
                    .iter()
                    .any(|r| r.id == id && r.summary.is_some())
                {
                    buttons = buttons.child(
                        Button::new("handoff-review")
                            .primary()
                            .label(t("Review handoff"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.review_handoff(id, window, cx)
                            })),
                    );
                }
                t("Preparing handoff")
            }
            HandoffDialog::Review { request, recipient } => {
                body = body.child(div().text_sm().child(t("Review or edit the summary. Start sends the full text directly to a fresh agent beside the source, in the same working directory and worktree.")))
                    .child(Self::wide_input(Input::new(&self.handoff_ui.input).h(px(260.0))))
                    .child(div().text_sm().child(t("Receiving agent")));
                let mut agents = div().flex().flex_wrap().gap_1();
                for preset in self.presets.iter().filter(|p| {
                    p.kind == muxal_core::PresetKind::Terminal
                        && p.program.is_some()
                        && self.agent_runnable(p)
                }) {
                    let id = preset.id;
                    agents = agents.child(
                        Button::new(SharedString::from(format!("handoff-agent-{id}")))
                            .ghost()
                            .selected(recipient == Some(id))
                            .label(preset.name.clone())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.handoff_ui.dialog = Some(HandoffDialog::Review {
                                    request,
                                    recipient: Some(id),
                                });
                                cx.notify();
                            })),
                    );
                }
                body = body.child(agents);
                let source_valid = self
                    .workspace
                    .handoffs
                    .iter()
                    .find(|r| r.id == request)
                    .is_some_and(|r| self.handoff_source(r).is_some());
                if !source_valid {
                    body = body.child(div().text_sm().child(t(
                        "The source conversation or directory changed. Prepare a new handoff.",
                    )));
                }
                if recipient.is_none() {
                    body = body.child(
                        div()
                            .text_sm()
                            .child(t("No installed agent preset is available.")),
                    );
                }
                buttons = buttons
                    .child(
                        Button::new("handoff-discard")
                            .ghost()
                            .label(t("Discard request"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.cancel_handoff(request, window, cx)
                            })),
                    )
                    .child(
                        Button::new("handoff-start")
                            .primary()
                            .label(t("Start with handoff"))
                            .disabled(recipient.is_none() || !source_valid)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.start_handoff(window, cx)),
                            ),
                    );
                t("Review handoff")
            }
        };
        if let Some(error) = &self.handoff_ui.error {
            body = body.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .child(error.clone()),
            );
        }
        buttons = buttons.child(
            Button::new("handoff-close")
                .ghost()
                .label(if matches!(dialog, HandoffDialog::Review { .. }) {
                    t("Save and close")
                } else {
                    t("Close")
                })
                .on_click(cx.listener(|this, _, window, cx| this.close_handoff(window, cx))),
        );
        modal_backdrop()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.close_handoff(window, cx)),
            )
            .child(
                div()
                    .id("handoff-card")
                    .track_focus(&self.dialog_focus)
                    .overflow_y_scroll()
                    .w(px(650.0))
                    .max_w_full()
                    .max_h_full()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .p_5()
                    .bg(cx.theme().background)
                    .border_1()
                    .border_color(cx.theme().border)
                    .rounded(cx.theme().radius_lg)
                    .shadow_lg()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                        if event.keystroke.key == "escape" {
                            cx.stop_propagation();
                            this.close_handoff(window, cx);
                        }
                    }))
                    .child(div().text_lg().font_semibold().child(title))
                    .child(body)
                    .child(buttons),
            )
            .into_any_element()
    }
}
