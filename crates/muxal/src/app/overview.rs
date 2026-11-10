//! The overview (the top bar's dashboard toggle): every agent in the
//! workspace as a tile showing its status and the bottom of its terminal, so
//! one glance answers "what is everything doing?" across projects. Clicking a
//! tile jumps to that pane.
//!
//! Previews are plain text read from each terminal's screen when the overview
//! paints; `MuxalApp::tick` repaints it once a second while it is open, and it
//! costs nothing while closed.

use super::*;

/// Rows of terminal text in a tile.
const PREVIEW_ROWS: usize = 10;
/// Characters kept per row. Tiles clip well before this; the cap only bounds
/// the text handed to layout for a very wide terminal.
const PREVIEW_COLS: usize = 160;

/// The bottom of a terminal screen as preview rows: rows right-trimmed,
/// trailing blank rows dropped (an agent's screen ends at its prompt, not at
/// the grid's last line), then the last `max_rows`, each cut to `max_cols`.
fn preview_rows(screen: &str, max_rows: usize, max_cols: usize) -> Vec<String> {
    let mut rows: Vec<&str> = screen.lines().map(str::trim_end).collect();
    while rows.last().is_some_and(|row| row.is_empty()) {
        rows.pop();
    }
    let start = rows.len().saturating_sub(max_rows);
    rows[start..]
        .iter()
        .map(|row| row.chars().take(max_cols).collect())
        .collect()
}

impl MuxalApp {
    pub(super) fn render_overview(&self, cx: &mut Context<Self>) -> AnyElement {
        let zones = crate::theme::zones(cx);
        let now = chrono::Utc::now().timestamp_millis();
        let mono =
            muxal_terminal::resolve_mono_family(cx.text_system(), self.settings.font_family.trim());
        let row_h = rems(0.9375);

        // Terminal panes per project, in layout order (stable, so a tile stays
        // where you last saw it).
        let groups: Vec<(Uuid, Vec<Uuid>)> = self
            .workspace
            .projects
            .iter()
            .map(|project| {
                let panes = project
                    .instances()
                    .into_iter()
                    .filter(|&iid| {
                        self.workspace
                            .instance(iid)
                            .is_some_and(|i| i.kind == InstanceKind::Terminal)
                    })
                    .collect::<Vec<_>>();
                (project.id, panes)
            })
            .filter(|(_, panes)| !panes.is_empty())
            .collect();

        let count = |wanted: AgentStatus| {
            groups
                .iter()
                .flat_map(|(_, panes)| panes)
                // Loud only: a result that has aged into rest isn't news.
                .filter(|&&iid| {
                    self.pane_status(iid, cx) == Some(wanted) && self.status_loud(iid, wanted)
                })
                .count()
        };
        let summary: Vec<AnyElement> = [
            (AgentStatus::Blocked, t("input")),
            (AgentStatus::Working, t("working")),
            (AgentStatus::Done, t("done")),
        ]
        .into_iter()
        .filter_map(|(status, word)| {
            let n = count(status);
            (n > 0).then(|| status_tag(status, format!("{word} · {n}"), cx).into_any_element())
        })
        .collect();

        let mut container = div()
            .id("overview-scroll")
            .size_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_4()
            .p_4()
            .bg(zones.floor)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_lg()
                            .text_color(cx.theme().foreground)
                            .child(t("All agents")),
                    )
                    .children(summary),
            );

        if groups.is_empty() {
            return container
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(cx.theme().muted_foreground)
                        .child(t("No agents are open")),
                )
                .into_any_element();
        }

        for (pid, panes) in groups {
            let Some(project) = self.workspace.project(pid) else {
                continue;
            };
            let branch = self.project_branches.get(&pid).cloned().flatten();
            let mut tiles = div().flex().flex_wrap().gap_3();
            for iid in panes {
                tiles = tiles.child(self.overview_tile(pid, iid, now, &mono, row_h, cx));
            }
            container = container.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .font_semibold()
                                    .text_color(cx.theme().foreground)
                                    .child(project.name.clone()),
                            )
                            .children(branch.map(|b| {
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(
                                        svg()
                                            .path("icons/git-branch.svg")
                                            .size(px(12.0))
                                            .flex_none()
                                            .text_color(cx.theme().muted_foreground),
                                    )
                                    .child(b)
                            })),
                    )
                    .child(tiles),
            );
        }
        container.into_any_element()
    }

    /// One agent: a small pane — header (icon in its status color, title,
    /// status) over the bottom rows of its terminal.
    fn overview_tile(
        &self,
        pid: Uuid,
        iid: Uuid,
        now: i64,
        mono: &SharedString,
        row_h: Rems,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let zones = crate::theme::zones(cx);
        let inst = self.workspace.instance(iid);
        let program = inst.and_then(|i| i.program.clone());
        let title = self.title_in_project(iid, self.instance_title(iid, cx), cx);
        let view = self.terminals.get(&iid);
        let status = self.pane_status(iid, cx);
        let loading = self.terminal_is_loading(iid, cx);
        let exited = view.is_some_and(|v| v.read(cx).exited());
        let mark = |status: AgentStatus| {
            crate::theme::legible_on(
                status_hsla(status, cx),
                zones.idle_header,
                zones.idle_header_text,
            )
        };

        let loud = status.is_some_and(|s| self.status_loud(iid, s));
        let icon_color = match status {
            Some(s) if loud => mark(s),
            _ => zones.idle_header_text,
        };
        // Only a pane that is waiting on you outlines its whole tile.
        let border = if status == Some(AgentStatus::Blocked) {
            mark(AgentStatus::Blocked)
        } else {
            zones.idle_border
        };
        let quiet = |label: SharedString| {
            div()
                .text_size(rems(0.75))
                .whitespace_nowrap()
                .text_color(zones.idle_header_text)
                .child(label)
                .into_any_element()
        };
        let status_el: AnyElement = if exited {
            quiet(t("process exited"))
        } else if loading {
            status_tag(AgentStatus::Working, t("loading").to_string(), cx).into_any_element()
        } else {
            match (status, inst) {
                (Some(s), Some(instance)) => {
                    let label = localized_activity_label(s, &instance.activity, now);
                    if !loud {
                        quiet(label.into())
                    } else {
                        status_tag(s, label, cx).into_any_element()
                    }
                }
                _ => quiet(t("not started")),
            }
        };

        let rows = view
            .map(|v| preview_rows(&v.read(cx).visible_text(), PREVIEW_ROWS, PREVIEW_COLS))
            .unwrap_or_default();
        let mut body = div()
            .h(rems(row_h.0 * PREVIEW_ROWS as f32 + 0.75))
            .px_2()
            .py(rems(0.375))
            .overflow_hidden()
            .bg(zones.terminal_background)
            .text_color(zones.terminal_foreground)
            .font_family(mono.clone())
            .text_size(rems(0.6875));
        if rows.is_empty() {
            body = body.child(
                div()
                    .text_color(zones.terminal_muted())
                    .child(if view.is_some() {
                        t("Starting terminal…")
                    } else {
                        t("not started")
                    }),
            );
        }
        for row in rows {
            body = body.child(
                div()
                    .h(row_h)
                    .flex_none()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(row),
            );
        }

        let hover_border = cx.theme().primary;
        let path_title = title_is_path(&title);
        div()
            .id(SharedString::from(format!("overview-{}", iid.simple())))
            .w(rems(24.0))
            .flex_none()
            .flex()
            .flex_col()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(border)
            .overflow_hidden()
            .cursor_pointer()
            .hover(move |s| s.border_color(hover_border))
            .on_click(cx.listener(move |this, _ev, window, cx| {
                this.show_dashboard = false;
                this.select_project(pid, window, cx);
                this.focus_instance(iid, window, cx);
            }))
            .child(
                div()
                    .flex_none()
                    .h(rems(1.75))
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .bg(zones.idle_header)
                    .text_color(zones.idle_header_text)
                    .text_size(rems(0.8125))
                    .child(agent_icon(program.as_deref(), px(14.0), icon_color))
                    .children(
                        self.instance_worktree_color(iid)
                            .map(|c| div().size(px(8.0)).rounded_full().flex_none().bg(c)),
                    )
                    .child({
                        let title_el = div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap();
                        if path_title {
                            title_el.text_ellipsis_start()
                        } else {
                            title_el.text_ellipsis()
                        }
                        .child(title)
                    })
                    .child(div().flex_none().child(status_el)),
            )
            .child(body)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::preview_rows;

    #[test]
    fn keeps_the_bottom_rows_and_drops_the_blank_tail() {
        let screen = "one  \ntwo\n\nthree\n   \n\n";
        assert_eq!(preview_rows(screen, 3, 80), vec!["two", "", "three"]);
        assert_eq!(
            preview_rows(screen, 10, 80),
            vec!["one", "two", "", "three"]
        );
    }

    #[test]
    fn an_empty_screen_has_no_rows() {
        assert!(preview_rows("   \n\n  \n", 10, 80).is_empty());
        assert!(preview_rows("", 10, 80).is_empty());
    }

    #[test]
    fn long_rows_are_cut_by_characters_not_bytes() {
        assert_eq!(preview_rows("héllo wörld", 5, 4), vec!["héll"]);
    }
}
