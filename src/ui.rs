//! Main TUI rendering engine.
//!
//! Uses Ratatui to render all application views including the package
//! list, detail panels, diagnostics, help screen, and status bar.
//! This is the largest module and the primary UI entry point.

#![allow(dead_code)]
#![allow(unused_imports)]
use crate::app::{App, FilterOption, InputMode};
use crate::ui_utils::{
    centered_rect, create_highlighted_line, render_scrollbar, truncate_with_ellipsis,
    visible_height,
};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Clear, Gauge, List, ListItem, ListState, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Tabs,
    },
    Frame,
};
use std::cmp::min;

const CYAN: Color = Color::Rgb(0, 255, 255);

pub fn render(app: &mut App, f: &mut Frame) {
    let theme = &app.theme;
    let area = f.size();

    // Check terminal size constraints
    let sidebar_allowed = area.width >= 100;
    let search_bar_height = if area.height >= 20 { 3 } else { 2 };
    let tabs_height = 3;

    // Build layout
    let main_chunks = if app.show_sidebar && sidebar_allowed && app.get_selected_package().is_some()
    {
        // Split-pane mode
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(tabs_height),
                Constraint::Length(search_bar_height),
                Constraint::Min(1),
                Constraint::Length(4),
            ])
            .split(area);

        let sub_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
            .split(chunks[2]);

        // Return: tabs, search, results, sidebar, status
        RenderChunks {
            tabs: chunks[0],
            search: chunks[1],
            results: sub_chunks[0],
            sidebar: Some(sub_chunks[1]),
            status: chunks[3],
        }
    } else {
        // Normal mode
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(tabs_height),
                Constraint::Length(search_bar_height),
                Constraint::Min(1),
                Constraint::Length(4),
            ])
            .split(area);

        RenderChunks {
            tabs: chunks[0],
            search: chunks[1],
            results: chunks[2],
            sidebar: None,
            status: chunks[3],
        }
    };

    // Render components
    render_tabs(app, f, main_chunks.tabs, theme);
    render_search_bar(app, f, main_chunks.search, theme);
    render_results_list(app, f, main_chunks.results, theme);
    render_status_bar(app, f, main_chunks.status, theme);

    // Render sidebar if active
    if let (true, Some(sidebar_area)) = (app.show_sidebar, main_chunks.sidebar) {
        render_details_sidebar(app, f, sidebar_area, theme);
    }

    // Render overlays (in priority order)
    if app.show_rollback_confirm {
        render_rollback_dialog(app, f, theme);
    } else if app.show_simulation {
        render_simulation_modal(app, f, theme);
    } else if app.show_help {
        render_help_overlay(f, area, theme);
    } else if app.show_updates_view {
        render_updates_view(app, f, area, theme);
    } else if app.show_diagnostics {
        render_diagnostics_overlay(app, f, area, theme);
    } else if app.show_system_info {
        render_system_info_overlay(app, f, area, theme);
    } else if app.show_orphans {
        render_orphans_overlay(app, f, area, theme);
    } else if app.show_package_sizes {
        render_package_sizes_overlay(app, f, area, theme);
    } else if app.show_cache {
        render_cache_overlay(app, f, area, theme);
    } else if app.show_foreign {
        render_foreign_overlay(app, f, area, theme);
    } else if app.show_groups {
        render_groups_overlay(app, f, area, theme);
    } else if app.show_pacnew_pacsave {
        render_pacnew_pacsave_overlay(app, f, area, theme);
    } else if app.show_pacman_log {
        render_pacman_log_overlay(app, f, area, theme);
    } else if app.show_downgrade_modal {
        render_downgrade_modal(app, f, area, theme);
    } else if app.show_health_dashboard {
        render_health_dashboard_overlay(app, f, area, theme);
    } else if app.show_history {
        render_history_overlay(app, f, area, theme);
    } else if app.show_package_details && !app.show_sidebar {
        render_package_details(app, f, theme);
    } else if app.show_dependency_visualization {
        render_dependency_visualization(app, f, theme);
    } else if app.show_console {
        render_console(app, f, theme);
    } else if app.show_password_prompt {
        render_password_prompt(app, f, theme);
    } else if app.show_confirm_prompt {
        render_confirmation(app, f, theme);
    }

    // Render toasts (always on top if no overlay)
    if !app.show_help
        && !app.show_diagnostics
        && !app.show_system_info
        && !app.show_orphans
        && !app.show_package_sizes
        && !app.show_cache
        && !app.show_foreign
        && !app.show_groups
        && !app.show_pacnew_pacsave
        && !app.show_pacman_log
        && !app.show_health_dashboard
        && !app.show_history
        && !app.show_package_details
        && !app.show_dependency_visualization
        && !app.show_console
        && !app.show_password_prompt
        && !app.show_confirm_prompt
        && !app.show_simulation
        && !app.show_rollback_confirm
    {
        render_toasts(app, f, area, theme);
    }
}

struct RenderChunks {
    tabs: Rect,
    search: Rect,
    results: Rect,
    sidebar: Option<Rect>,
    status: Rect,
}

fn render_toasts(app: &App, f: &mut Frame, area: Rect, theme: &crate::theme::Theme) {
    if app.toasts.is_empty() {
        return;
    }

    let toast_width = 60.min(area.width.saturating_sub(4));
    let toast_height = app.toasts.len() as u16 + 2;

    let toast_area = Rect {
        x: area.x + (area.width - toast_width) / 2,
        y: area.y + 1,
        width: toast_width,
        height: toast_height,
    };

    let lines: Vec<Line> = app
        .toasts
        .iter()
        .map(|toast| {
            let (_border_color, text_style) = toast.get_render_style(theme);
            Line::from(vec![Span::styled(&toast.message, text_style)])
        })
        .collect();

    let toast_widget = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Thick)
                .title("Notification")
                .border_style(Style::default().fg(theme.info())),
        )
        .alignment(Alignment::Center);

    f.render_widget(Clear, toast_area);
    f.render_widget(toast_widget, toast_area);
}

fn render_tabs(app: &App, f: &mut Frame, area: Rect, theme: &crate::theme::Theme) {
    let titles = vec![" System ", " Ecosystem "];
    let index = match app.view_mode {
        crate::app::ViewMode::System => 0,
        crate::app::ViewMode::Ecosystem => 1,
    };

    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded),
        )
        .select(index)
        .style(Style::default().fg(theme.muted()))
        .highlight_style(
            Style::default()
                .fg(theme.primary())
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::UNDERLINED),
        );

    f.render_widget(tabs, area);
}

fn render_search_bar(app: &App, f: &mut Frame, area: Rect, theme: &crate::theme::Theme) {
    let input_style = match app.input_mode {
        InputMode::Normal => Style::default().fg(theme.foreground()),
        InputMode::Editing => Style::default().fg(theme.primary()),
    };

    let border_style = match app.input_mode {
        InputMode::Normal => Style::default().fg(theme.border()),
        InputMode::Editing => Style::default().fg(theme.primary()),
    };

    let title = if app.is_loading {
        format!(
            "🔍 {} ({})",
            app.localizer.t("search_placeholder"),
            app.animation_state.spinner_char()
        )
    } else if app.input_mode == InputMode::Editing && app.history_index.is_some() {
        let history_pos = app.history_index.map_or(1, |idx| idx + 1);
        format!(
            "🔍 {} (history {})",
            app.localizer.t("search_placeholder"),
            history_pos
        )
    } else {
        format!("🔍 {}", app.localizer.t("search_placeholder"))
    };

    let input = Paragraph::new(app.search_input.as_str())
        .style(input_style)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title(title),
        );
    f.render_widget(input, area);
}

fn render_results_list(app: &App, f: &mut Frame, area: Rect, theme: &crate::theme::Theme) {
    let page_items = app.get_paginated_results();
    let vis_height = visible_height(area, true, 1);

    let items: Vec<ListItem> = page_items
        .iter()
        .enumerate()
        .map(|(idx, pkg)| {
            let color = if pkg.is_installed {
                theme.success()
            } else {
                match pkg.source {
                    crate::models::PackageSource::Pacman => theme.repo_color(),
                    crate::models::PackageSource::Aur => theme.aur_color(),
                    crate::models::PackageSource::Npm => theme.warning(),
                    crate::models::PackageSource::Cargo => theme.secondary(),
                    crate::models::PackageSource::Pip => theme.info(),
                    crate::models::PackageSource::Apt 
                    | crate::models::PackageSource::Brew 
                    | crate::models::PackageSource::Scoop
                    | crate::models::PackageSource::Dnf
                    | crate::models::PackageSource::Zypper
                    | crate::models::PackageSource::Apk => theme.primary(),
                }
            };

            let status_mark = if app.selected_packages.contains_key(&pkg.name) {
                "☑".to_string()
            } else if pkg.is_installed {
                format!(
                    "✓ [{}]",
                    app.localizer
                        .t("installed_label")
                        .chars()
                        .next()
                        .unwrap_or('I')
                )
            } else {
                "  ○".to_string()
            };

            let source_indicator = match pkg.source {
                crate::models::PackageSource::Pacman => "📦   ",
                crate::models::PackageSource::Aur => "↑aur ",
                crate::models::PackageSource::Npm => "npm  ",
                crate::models::PackageSource::Cargo => "crgo ",
                crate::models::PackageSource::Pip => "pip  ",
                crate::models::PackageSource::Apt => "apt  ",
                crate::models::PackageSource::Brew => "brew ",
                crate::models::PackageSource::Scoop => "scop ",
                crate::models::PackageSource::Dnf => "dnf  ",
                crate::models::PackageSource::Zypper => "zyp  ",
                crate::models::PackageSource::Apk => "apk  ",
            };

            let size_str = pkg.format_download_size();
            let max_name_width = (area.width as usize).saturating_sub(35).max(10);
            let truncated_name = truncate_with_ellipsis(&pkg.name, max_name_width);

            let is_selected = Some(idx) == app.selected_index;
            let base_style = if is_selected {
                Style::default().fg(theme.highlight_fg())
            } else {
                Style::default().fg(color)
            };

            let highlight_style = if is_selected {
                Style::default()
                    .fg(theme.warning())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(theme.primary())
                    .add_modifier(Modifier::BOLD)
            };

            let mut spans = Vec::new();
            spans.push(Span::styled(
                format!("{} {} ", status_mark, source_indicator),
                base_style,
            ));

            if let Some(indices) = app.fuzzy_indices.get(&pkg.name) {
                let highlighted_line =
                    create_highlighted_line(&truncated_name, indices, base_style, highlight_style);
                spans.extend(highlighted_line.spans);

                let name_len = truncated_name.chars().count();
                if name_len < max_name_width {
                    spans.push(Span::styled(
                        " ".repeat(max_name_width - name_len),
                        base_style,
                    ));
                }
            } else {
                spans.push(Span::styled(
                    format!("{:<width$}", truncated_name, width = max_name_width),
                    base_style,
                ));
            }

            spans.push(Span::styled(
                format!(" {:>6} {}", size_str, pkg.version),
                base_style,
            ));

            ListItem::new(Line::from(spans))
        })
        .collect();

    let title = if app.is_loading {
        format!(
            "{} ({})",
            app.localizer.t("packages_label"),
            app.localizer.t("loading_label")
        )
    } else {
        let ecosystem_indicator = if matches!(app.view_mode, crate::app::ViewMode::Ecosystem) {
            format!(" [{:?}] ", app.active_ecosystem).to_lowercase()
        } else {
            "".to_string()
        };

        let filter_info = match app.current_filter {
            FilterOption::All => "".to_string(),
            FilterOption::Installed => " [Installed]".to_string(),
            FilterOption::NotInstalled => " [Not Installed]".to_string(),
            FilterOption::RepoOnly => " [Repo]".to_string(),
            FilterOption::AurOnly => " [AUR]".to_string(),
            FilterOption::Updates => " [Updates]".to_string(),
            FilterOption::AUR => " [AUR]".to_string(),
            FilterOption::Group(ref g) => format!(" [{}]", g),
        };

        let page_info = if app.total_pages() > 1 {
            format!(" Page {}/{}", app.current_page + 1, app.total_pages())
        } else {
            "".to_string()
        };

        let sort_indicator = match app.current_sort {
            crate::app::SortOption::NameAsc => " ↓Name",
            crate::app::SortOption::NameDesc => " ↑Name",
            crate::app::SortOption::Source => " ↓Source",
            crate::app::SortOption::SizeAsc => " ↓Size",
            crate::app::SortOption::SizeDesc => " ↑Size",
            crate::app::SortOption::Group => " ↓Group",
        };

        let results_len = if matches!(app.view_mode, crate::app::ViewMode::Ecosystem) {
            app.ecosystem_results.len()
        } else {
            app.results.len()
        };
        let count_info = format!(" ({})", results_len);

        format!(
            "{}{}{}{}{}{}",
            app.localizer.t("packages_label"),
            ecosystem_indicator,
            count_info,
            filter_info,
            page_info,
            sort_indicator
        )
    };

    let border_type = BorderType::Rounded;

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(border_type)
                .title(title.clone())
                .border_style(Style::default().fg(theme.border())),
        )
        .highlight_style(
            Style::default()
                .fg(theme.highlight_fg())
                .bg(theme.highlight_bg())
                .add_modifier(Modifier::BOLD),
        );

    let mut state = ratatui::widgets::ListState::default();
    state.select(app.selected_index);

    f.render_stateful_widget(list, area, &mut state);

    if page_items.len() >= vis_height {
        let scrollbar_area = Rect {
            x: area.x + area.width - 1,
            y: area.y + 1,
            width: 1,
            height: area.height.saturating_sub(2),
        };

        let mut scroll_state = app.results_scroll_state;
        if let Some(selected) = app.selected_index {
            scroll_state = scroll_state.position(selected);
        }

        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("↑"))
            .end_symbol(Some("↓"))
            .track_symbol(Some("│"))
            .thumb_symbol("█")
            .style(
                Style::default()
                    .fg(theme.muted())
                    .add_modifier(Modifier::BOLD),
            );

        f.render_stateful_widget(scrollbar, scrollbar_area, &mut scroll_state);
    }

    if !app.is_loading && page_items.is_empty() {
        let empty_msg = if app.search_input.is_empty() {
            app.localizer.t("no_results_found")
        } else {
            format!(
                "{}: '{}'",
                app.localizer.t("no_results_found"),
                app.search_input
            )
        };
        let empty_block = Paragraph::new(empty_msg)
            .style(Style::default().fg(theme.secondary()))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(border_type)
                    .title(title.clone())
                    .border_style(Style::default().fg(theme.border())),
            );
        f.render_widget(empty_block, area);
        return;
    }

    if !app.is_loading && !page_items.is_empty() {
        let info_text = match app.input_mode {
            InputMode::Editing => "Esc Exit  Enter Search  ↑↓ History".to_string(),
            InputMode::Normal => {
                let help_key = app.config.keyboard.quit.as_str() == "q";
                let selected_count = if app.selected_packages.is_empty() {
                    String::new()
                } else {
                    format!(" | Selected: {}", app.selected_packages.len())
                };
                format!(
                    "{} for help | ? Filter: {:?} | Sort: {:?}{}",
                    if help_key { "?" } else { "h" },
                    app.current_filter,
                    app.current_sort,
                    selected_count
                )
            }
        };

        let info = Paragraph::new(info_text)
            .style(
                Style::default()
                    .fg(theme.muted())
                    .add_modifier(Modifier::ITALIC),
            )
            .alignment(Alignment::Right);

        let info_area = Rect {
            x: area.x + area.width.saturating_sub(40),
            y: area.y + 1,
            width: min(38, area.width.saturating_sub(2)),
            height: 1,
        };
        f.render_widget(info, info_area);
    }
}

fn render_status_bar(app: &App, f: &mut Frame, area: Rect, theme: &crate::theme::Theme) {
    if let Some(err) = &app.error_message {
        let footer = Paragraph::new(format!("{}: {}", app.localizer.t("error_occurred"), err))
            .style(Style::default().fg(theme.error()).bg(Color::Rgb(50, 0, 0)))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Thick)
                    .title(app.localizer.t("error_occurred"))
                    .title_alignment(Alignment::Center)
                    .border_style(Style::default().fg(theme.error())),
            );
        f.render_widget(footer, area);
    } else if app.is_operation_running && app.install_total > 0 {
        let progress_pct = app.get_progress_percentage() as u16;
        let progress_label = format!(
            " Installing: {}/{} - {} ",
            app.install_current + 1,
            app.install_total,
            app.install_current_package
        );
        let progress_gauge = Gauge::default()
            .gauge_style(Style::default().fg(theme.primary()))
            .label(progress_label)
            .percent(progress_pct);

        f.render_widget(progress_gauge, area);
    } else {
        let update_status = match app.available_updates {
            Some(0) => app.localizer.t("up_to_date"),
            Some(n) => format!("{}: {}", app.localizer.t("updates_available"), n),
            None => app.localizer.t("loading_label"),
        };

        let update_color = match app.available_updates {
            Some(n) if n > 0 => theme.warning(),
            Some(0) => theme.success(),
            _ => theme.muted(),
        };

        let selection_info = if !app.selected_packages.is_empty() {
            format!(" [{} selected]", app.selected_packages.len())
        } else {
            "".to_string()
        };
        let install_key = app.config.keyboard.install.as_str();

        let footer_lines = vec![
            Line::from(vec![
                Span::styled(
                    " ? ",
                    Style::default().fg(theme.foreground()).bg(theme.muted()),
                ),
                Span::raw(" Help "),
                Span::styled(
                    " / ",
                    Style::default().fg(theme.foreground()).bg(theme.muted()),
                ),
                Span::raw(" Search "),
                Span::styled(
                    " Tab ",
                    Style::default().fg(theme.foreground()).bg(theme.muted()),
                ),
                Span::raw(" Switch View "),
                Span::styled(
                    " [ ] ",
                    Style::default().fg(theme.foreground()).bg(theme.muted()),
                ),
                Span::raw(" Prov "),
                Span::styled(
                    format!(" {} ", install_key),
                    Style::default().fg(theme.foreground()).bg(theme.muted()),
                ),
                Span::raw(" Install/Remove "),
                Span::styled(
                    " Space ",
                    Style::default().fg(theme.foreground()).bg(theme.muted()),
                ),
                Span::raw(" Select "),
            ]),
            Line::from(vec![
                Span::styled(
                    " q ",
                    Style::default().fg(theme.foreground()).bg(theme.muted()),
                ),
                Span::raw(" Quit "),
                Span::styled(
                    " f ",
                    Style::default().fg(theme.foreground()).bg(theme.muted()),
                ),
                Span::raw(" Filter "),
                Span::styled(
                    " s ",
                    Style::default().fg(theme.foreground()).bg(theme.muted()),
                ),
                Span::raw(" Sort "),
                Span::styled(
                    " n/p ",
                    Style::default().fg(theme.foreground()).bg(theme.muted()),
                ),
                Span::raw(" Page "),
                Span::styled(
                    " U ",
                    Style::default().fg(theme.foreground()).bg(theme.muted()),
                ),
                Span::raw(" Update "),
                Span::styled(update_status, Style::default().fg(update_color)),
                Span::styled(
                    selection_info,
                    Style::default()
                        .fg(theme.primary())
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
        ];

        let footer = Paragraph::new(footer_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(app.localizer.t("status"))
                .title_alignment(Alignment::Center)
                .border_style(Style::default().fg(theme.border())),
        );
        f.render_widget(footer, area);
    }
}

fn render_details_sidebar(app: &App, f: &mut Frame, area: Rect, theme: &crate::theme::Theme) {
    if let Some(pkg) = app.get_selected_package() {
        let sidebar_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(format!("📦 {}", pkg.name))
            .border_style(Style::default().fg(theme.primary()));

        f.render_widget(sidebar_block, area);

        let inner = Rect {
            x: area.x + 2,
            y: area.y + 2,
            width: area.width.saturating_sub(4),
            height: area.height.saturating_sub(4),
        };

        let mut lines = vec![
            Line::from(vec![
                Span::styled("Version: ", Style::default().fg(theme.muted())),
                Span::styled(&pkg.version, Style::default().fg(theme.foreground())),
            ]),
            Line::from(""),
        ];

        if !pkg.description.is_empty() {
            let desc_text = &pkg.description;
            let desc_lines = desc_text
                .chars()
                .take(inner.width as usize * 3)
                .collect::<String>();
            lines.push(Line::from(vec![Span::styled(
                "Description: ",
                Style::default().fg(theme.muted()),
            )]));
            for word in desc_lines.split_whitespace() {
                lines.push(Line::from(Span::styled(
                    word.to_string(),
                    Style::default().fg(theme.foreground()),
                )));
            }
            lines.push(Line::from(""));
        }

        if !pkg.licenses.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("License: ", Style::default().fg(theme.muted())),
                Span::styled(
                    pkg.licenses.join(", "),
                    Style::default().fg(theme.secondary()),
                ),
            ]));
        }

        if pkg.is_installed {
            lines.push(Line::from(vec![
                Span::styled("Status: ", Style::default().fg(theme.muted())),
                Span::styled(
                    "Installed",
                    Style::default()
                        .fg(theme.success())
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        }

        if pkg.is_outdated {
            lines.push(Line::from(vec![
                Span::styled("Status: ", Style::default().fg(theme.muted())),
                Span::styled(
                    "Outdated",
                    Style::default()
                        .fg(theme.warning())
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        }

        if matches!(pkg.source, crate::models::PackageSource::Aur) {
            if pkg.num_votes.unwrap_or(0) > 0 || pkg.votes.unwrap_or(0) > 0 {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled("Votes: ", Style::default().fg(theme.muted())),
                    Span::styled(pkg.format_votes(), Style::default().fg(theme.primary())),
                ]));
            }
            if pkg.popularity.is_some() {
                lines.push(Line::from(vec![
                    Span::styled("Popularity: ", Style::default().fg(theme.muted())),
                    Span::styled(
                        pkg.format_popularity(),
                        Style::default().fg(theme.secondary()),
                    ),
                ]));
            }
            if pkg.last_updated.is_some() {
                lines.push(Line::from(vec![
                    Span::styled("Updated: ", Style::default().fg(theme.muted())),
                    Span::styled(
                        pkg.format_last_updated(),
                        Style::default().fg(theme.foreground()),
                    ),
                ]));
            }
        }

        if !pkg.depends_on.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![Span::styled(
                "Dependencies: ",
                Style::default().fg(theme.muted()),
            )]));
            for dep in pkg.depends_on.iter().take(5) {
                lines.push(Line::from(vec![
                    Span::styled("  • ", Style::default().fg(theme.muted())),
                    Span::styled(dep, Style::default().fg(theme.foreground())),
                ]));
            }
            if pkg.depends_on.len() > 5 {
                lines.push(Line::from(vec![Span::styled(
                    format!("  ...and {} more", pkg.depends_on.len() - 5),
                    Style::default().fg(theme.muted()),
                )]));
            }
        }

        let sidebar_content = Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: true });

        f.render_widget(sidebar_content, inner);
    }
}

fn render_password_prompt(app: &App, f: &mut Frame, theme: &crate::theme::Theme) {
    let block = Block::default()
        .title(app.localizer.t("sudo_password_required"))
        .borders(Borders::ALL)
        .border_type(BorderType::Thick)
        .style(
            Style::default()
                .bg(theme.background())
                .fg(theme.foreground()),
        )
        .border_style(Style::default().fg(theme.warning()));

    let area = centered_rect(60, 30, f.size());

    f.render_widget(Clear, area);
    f.render_widget(block.clone(), area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Length(3)].as_ref())
        .margin(2)
        .split(area);

    let msg = Paragraph::new(app.localizer.t("enter_sudo_password"))
        .style(Style::default().fg(theme.foreground()));
    f.render_widget(msg, layout[0]);

    let masked = app.password_input.masked();
    let input = Paragraph::new(masked)
        .style(Style::default().fg(theme.warning()))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(app.localizer.t("password_label")),
        );
    f.render_widget(input, layout[1]);
}

fn render_confirmation(app: &App, f: &mut Frame, theme: &crate::theme::Theme) {
    if app.packages_pending_confirmation.is_empty() {
        return;
    }

    let pkg_count = app.packages_pending_confirmation.len();
    let is_multi = pkg_count > 1;

    let first = &app.packages_pending_confirmation[0];

    let action_str = if first.is_installed {
        app.localizer.t("remove_button")
    } else {
        app.localizer.t("install_button")
    };

    let title = if first.is_installed {
        app.localizer.t("confirm_remove")
    } else {
        app.localizer.t("confirm_install")
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Thick)
        .style(
            Style::default()
                .bg(theme.background())
                .fg(theme.foreground()),
        )
        .border_style(Style::default().fg(theme.primary()));

    let area = centered_rect(75, 60, f.size());

    f.render_widget(Clear, area);
    f.render_widget(block.clone(), area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(2),
                Constraint::Length(1),
            ]
            .as_ref(),
        )
        .margin(2)
        .split(area);

    let text = if is_multi {
        let total_size: u64 = app
            .packages_pending_confirmation
            .iter()
            .map(|p| p.get_size())
            .sum();
        let size_str = crate::models::Package::format_size(total_size);
        format!(
            "{} {}\n{} {}\n{}: {}",
            app.localizer.t("confirm_multiple"),
            action_str.to_lowercase(),
            pkg_count,
            if pkg_count == 1 {
                "package"
            } else {
                "packages"
            },
            app.localizer.t("total_size"),
            size_str
        )
    } else {
        let size_str = crate::models::Package::format_size(first.get_size());
        let repo = if first.source == crate::models::PackageSource::Aur {
            app.localizer.t("aur_label")
        } else {
            app.localizer.t("repo_label")
        };
        format!(
            "{} {} {}\n{}: {} | {}: {}",
            app.localizer.t("confirm_single"),
            action_str.to_lowercase(),
            first.name,
            app.localizer.t("size_label"),
            size_str,
            "Source",
            repo
        )
    };

    let p_text = Paragraph::new(text)
        .style(
            Style::default()
                .fg(theme.foreground())
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Center)
        .wrap(ratatui::widgets::Wrap { trim: true });
    f.render_widget(p_text, layout[0]);

    let hint_text = Paragraph::new(app.localizer.t("confirmation_instructions"))
        .style(
            Style::default()
                .fg(theme.muted())
                .add_modifier(Modifier::ITALIC),
        )
        .alignment(Alignment::Center);
    f.render_widget(hint_text, layout[3]);
}

fn render_updates_view(app: &App, f: &mut Frame, area: Rect, theme: &crate::theme::Theme) {
    use ratatui::layout::{Constraint, Direction, Layout, Rect};
    use ratatui::widgets::{
        BorderType, Borders, List, ListItem, ListState, Scrollbar, ScrollbarState,
    };

    if app.outdated_packages.is_empty() {
        let block = Block::default()
            .title("⚠️  No Updates Available")
            .borders(Borders::ALL)
            .border_type(BorderType::Thick)
            .border_style(Style::default().fg(theme.primary()));

        let center_area = centered_rect(50, 30, f.size());
        f.render_widget(Clear, center_area);
        f.render_widget(block, center_area);

        let msg = Paragraph::new("Your system is up to date!")
            .style(Style::default().fg(theme.success()))
            .alignment(Alignment::Center);
        f.render_widget(msg, center_area);
        return;
    }

    let main_block = Block::default()
        .title(format!(
            "⚠️  Available Updates ({})",
            app.outdated_packages.len()
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Thick)
        .style(
            Style::default()
                .bg(theme.background())
                .fg(theme.foreground()),
        )
        .border_style(Style::default().fg(theme.warning()));

    f.render_widget(Clear, area);
    f.render_widget(main_block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(3),
        ])
        .margin(1)
        .split(area);

    let mut items = Vec::new();
    let filtered = app.get_filtered_outdated_packages();

    if app.get_security_updates_count() > 0 {
        items.push(
            ListItem::new(Span::styled(
                "🚨 SECURITY CRITICAL UPDATES",
                Style::default()
                    .fg(theme.error())
                    .add_modifier(Modifier::BOLD),
            ))
            .style(Style::default().bg(Color::Rgb(50, 0, 0))),
        );
    }

    for (_idx, pkg) in filtered.iter().enumerate() {
        let checkbox = if pkg.is_selected { "☑" } else { "☐" };
        let security_flag = if pkg.is_security_update {
            format!(" 🔴 {}", pkg.cve_info.as_deref().unwrap_or("CVE"))
        } else {
            String::new()
        };
        let aur_flag = if pkg.is_aur { " [AUR]" } else { "" };
        let repo = format!(" [{}]", pkg.repository);
        let dep_info = if !pkg.new_dependencies.is_empty() {
            format!(" → requires {} deps", pkg.new_dependencies.len())
        } else {
            String::new()
        };
        let rebuild_flag = if pkg.needs_rebuild {
            " ⚠️ rebuild needed"
        } else {
            ""
        };

        let has_security_header = app.get_security_updates_count() > 0;
        let item_idx = if has_security_header { _idx + 1 } else { _idx };
        let is_selected = Some(item_idx) == app.updates_cursor;

        let base_style = if is_selected {
            Style::default().fg(theme.highlight_fg())
        } else if pkg.is_security_update {
            Style::default().fg(theme.error())
        } else if pkg.is_selected {
            Style::default().fg(theme.success())
        } else {
            Style::default().fg(theme.foreground())
        };

        let mut spans = Vec::new();
        spans.push(Span::styled(format!("{} ", checkbox), base_style));

        let highlight_style = if is_selected {
            Style::default()
                .fg(theme.warning())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(theme.primary())
                .add_modifier(Modifier::BOLD)
        };

        if let Some(indices) = app.fuzzy_indices.get(&pkg.name) {
            let highlighted_line =
                create_highlighted_line(&pkg.name, indices, base_style, highlight_style);
            spans.extend(highlighted_line.spans);

            let name_len = pkg.name.chars().count();
            if name_len < 18 {
                spans.push(Span::styled(" ".repeat(18 - name_len), base_style));
            }
        } else {
            spans.push(Span::styled(format!("{:18}", pkg.name), base_style));
        }

        spans.push(Span::styled(
            format!(
                " {:12} {:>8}{}{}{}{}{}",
                pkg.version_change(),
                pkg.formatted_size(),
                repo,
                aur_flag,
                dep_info,
                rebuild_flag,
                security_flag
            ),
            base_style,
        ));

        let line = Line::from(spans);
        items.push(ListItem::new(line));
    }

    let list = List::new(items).block(Block::default()).highlight_style(
        Style::default()
            .fg(theme.highlight_fg())
            .bg(theme.highlight_bg()),
    );

    let mut state = ListState::default();
    if let Some(cursor) = app.updates_cursor {
        state.select(Some(cursor));
    }

    f.render_widget(list, chunks[1]);

    let total_size = app.get_total_update_size();
    let selected_count = app.selected_updates.len();
    let selected_size = app.get_selected_update_size();

    let mut status_parts = Vec::new();
    status_parts.push(format!(
        "Total: {} ({})",
        filtered.len(),
        crate::models::Package::format_size(total_size)
    ));

    if selected_count > 0 {
        status_parts.push(format!(
            "Selected: {} ({})",
            selected_count,
            crate::models::Package::format_size(selected_size)
        ));
    }

    if app.has_aur_needing_rebuild() {
        status_parts.push("⚠️ AUR rebuild needed".to_string());
    }

    let warn_line = if !app.partial_update_warning_shown
        && selected_count > 0
        && selected_count < filtered.len()
    {
        "⚠️ Partial updates not recommended. Consider updating all."
    } else {
        ""
    };

    let status_text = if warn_line.is_empty() {
        status_parts.join(" | ")
    } else {
        format!("{} | {}", status_parts.join(" | "), warn_line)
    };

    let status = Paragraph::new(status_text)
        .style(
            Style::default()
                .fg(theme.muted())
                .add_modifier(Modifier::ITALIC),
        )
        .alignment(Alignment::Left);

    f.render_widget(status, chunks[2]);
}

fn render_history_overlay(app: &App, f: &mut Frame, area: Rect, theme: &crate::theme::Theme) {
    let mut items = Vec::new();

    if app.transaction_history.is_empty() {
        items.push(ListItem::new(Line::from("No transactions recorded yet.")));
    } else {
        // Show up to 30 items in the history list (if available)
        for tx in app.transaction_history.iter().take(30) {
            let status_color = match tx.status {
                crate::transaction_history::TransactionStatus::Success => theme.success(),
                crate::transaction_history::TransactionStatus::Failed => theme.error(),
                crate::transaction_history::TransactionStatus::Cancelled => theme.warning(),
                crate::transaction_history::TransactionStatus::Pending => theme.info(),
            };
            items.push(ListItem::new(Line::from(vec![
                Span::styled(
                    format!(
                        "{} | +{} -{} | {}",
                        tx.created_at,
                        tx.installed_packages.len(),
                        tx.removed_packages.len(),
                        tx.id
                    ),
                    Style::default().fg(theme.foreground()),
                ),
                Span::raw(" "),
                Span::styled(
                    format!("{:?}", tx.status),
                    Style::default()
                        .fg(status_color)
                        .add_modifier(Modifier::BOLD),
                ),
            ])));
        }
    }

    items.push(ListItem::new(Line::from("")));
    items.push(ListItem::new(Line::from(Span::styled(
        "Press 'R' to rollback latest successful transaction",
        Style::default().fg(theme.primary()),
    ))));
    items.push(ListItem::new(Line::from(Span::styled(
        "Press 'Esc' to close",
        Style::default().fg(theme.muted()),
    ))));

    let list = List::new(items.clone())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Thick)
                .title("Transaction History")
                .border_style(Style::default().fg(theme.primary())),
        )
        .highlight_style(
            Style::default()
                .fg(theme.highlight_fg())
                .bg(theme.highlight_bg())
                .add_modifier(Modifier::BOLD),
        );

    let popup = centered_rect(80, 70, area);
    f.render_widget(Clear, popup);

    let mut state = ListState::default();
    state.select(app.history_cursor);
    f.render_stateful_widget(list, popup, &mut state);

    if let Some(mut scroll_state) = app.history_scroll_state {
        let scrollbar_area = Rect {
            x: popup.x + popup.width - 1,
            y: popup.y + 1,
            width: 1,
            height: popup.height.saturating_sub(2),
        };

        let items_len = items.len();
        scroll_state = scroll_state.content_length(items_len);
        if let Some(cursor) = app.history_cursor {
            scroll_state = scroll_state.position(cursor);
        }

        render_scrollbar(f, scrollbar_area, &mut scroll_state, theme.primary());
    }
}

fn render_diagnostics_overlay(app: &App, f: &mut Frame, area: Rect, theme: &crate::theme::Theme) {
    let mut items = vec![ListItem::new(Line::from(vec![Span::styled(
        "System Diagnostics",
        Style::default()
            .fg(theme.primary())
            .add_modifier(Modifier::BOLD),
    )]))];
    items.push(ListItem::new(Line::from("")));

    if app.diagnostics.is_empty() {
        items.push(ListItem::new(Line::from("No diagnostics available.")));
    } else {
        for item in &app.diagnostics {
            items.push(ListItem::new(Line::from(format!(
                "{}: {}",
                item.label, item.status
            ))));
        }
    }

    items.push(ListItem::new(Line::from("")));
    items.push(ListItem::new(Line::from(Span::styled(
        "Press 'Esc' to close",
        Style::default().fg(theme.muted()),
    ))));

    let list = List::new(items.clone())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Thick)
                .title("Diagnostics")
                .border_style(Style::default().fg(theme.primary())),
        )
        .highlight_style(
            Style::default()
                .fg(theme.highlight_fg())
                .bg(theme.highlight_bg())
                .add_modifier(Modifier::BOLD),
        );

    let popup = centered_rect(70, 50, area);
    f.render_widget(Clear, popup);

    let mut state = ListState::default();
    state.select(app.diagnostics_cursor);
    f.render_stateful_widget(list, popup, &mut state);

    if let Some(mut scroll_state) = app.diagnostics_scroll_state {
        let scrollbar_area = Rect {
            x: popup.x + popup.width - 1,
            y: popup.y + 1,
            width: 1,
            height: popup.height.saturating_sub(2),
        };

        let items_len = items.len();
        scroll_state = scroll_state.content_length(items_len);
        if let Some(cursor) = app.diagnostics_cursor {
            scroll_state = scroll_state.position(cursor);
        }

        render_scrollbar(f, scrollbar_area, &mut scroll_state, theme.primary());
    }
}

fn render_system_info_overlay(app: &App, f: &mut Frame, area: Rect, theme: &crate::theme::Theme) {
    let mut lines = vec![Line::from(vec![Span::styled(
        "System Information",
        Style::default()
            .fg(theme.primary())
            .add_modifier(Modifier::BOLD),
    )])];
    lines.push(Line::from(""));
    if app.system_info.is_empty() {
        lines.push(Line::from("No system info available."));
    } else {
        for item in &app.system_info {
            lines.push(Line::from(format!("{}: {}", item.label, item.status)));
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::from("Press 'Esc' to close"));

    let para = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Thick)
                .title("System Info")
                .border_style(Style::default().fg(theme.aur_color())),
        )
        .wrap(ratatui::widgets::Wrap { trim: true });

    let popup = centered_rect(60, 60, area);
    f.render_widget(Clear, popup);
    f.render_widget(para, popup);
}

fn render_orphans_overlay(app: &App, f: &mut Frame, area: Rect, theme: &crate::theme::Theme) {
    let mut lines = vec![Line::from(vec![Span::styled(
        "Orphan Packages",
        Style::default()
            .fg(theme.warning())
            .add_modifier(Modifier::BOLD),
    )])];
    lines.push(Line::from(""));
    if app.orphan_packages.is_empty() {
        lines.push(Line::from(
            "No orphan packages found. All packages are required by something.",
        ));
    } else {
        lines.push(Line::from(format!(
            "Found {} orphan package(s):",
            app.orphan_packages.len()
        )));
        lines.push(Line::from(""));
        for pkg in &app.orphan_packages {
            lines.push(Line::from(vec![
                Span::raw("  • "),
                Span::styled(
                    &pkg.name,
                    Style::default()
                        .fg(theme.warning())
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(
            "These packages are explicitly installed but not required by any other package.",
        ));
        lines.push(Line::from(
            "You can remove them with: sudo pacman -Rcs <package_name>",
        ));
    }
    lines.push(Line::from(""));
    lines.push(Line::from("Press 'Esc' to close"));

    let para = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Thick)
                .title("Orphan Packages")
                .border_style(Style::default().fg(theme.warning())),
        )
        .wrap(ratatui::widgets::Wrap { trim: true });

    let popup = centered_rect(70, 50, area);
    f.render_widget(Clear, popup);
    f.render_widget(para, popup);
}

fn render_package_sizes_overlay(app: &App, f: &mut Frame, area: Rect, theme: &crate::theme::Theme) {
    let mut lines = vec![Line::from(vec![Span::styled(
        "Package Size Analysis",
        Style::default()
            .fg(theme.info())
            .add_modifier(Modifier::BOLD),
    )])];
    lines.push(Line::from(""));
    if app.package_sizes.is_empty() {
        lines.push(Line::from("No package size data available."));
    } else {
        // Calculate total size
        let total_bytes: u64 = app.package_sizes.iter().map(|p| p.size_kb * 1024).sum();
        let total_size = crate::models::Package::format_size_bytes(total_bytes);
        lines.push(Line::from(format!(
            "Top 30 largest packages (Total: {}):",
            total_size
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Package", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw("                    "),
            Span::styled("Size", Style::default().add_modifier(Modifier::BOLD)),
        ]));
        lines.push(Line::from("-".repeat(50)));

        for pkg in &app.package_sizes {
            let name = if pkg.name.len() > 25 {
                format!("{}...", &pkg.name[..22])
            } else {
                pkg.name.clone()
            };
            lines.push(Line::from(format!("{:<28} {}", name, pkg.size_formatted)));
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::from("Press 'Esc' to close"));

    let para = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Thick)
                .title("Package Sizes")
                .border_style(Style::default().fg(theme.info())),
        )
        .wrap(ratatui::widgets::Wrap { trim: true });

    let popup = centered_rect(65, 70, area);
    f.render_widget(Clear, popup);
    f.render_widget(para, popup);
}

fn render_cache_overlay(app: &App, f: &mut Frame, area: Rect, theme: &crate::theme::Theme) {
    let mut lines = vec![Line::from(vec![Span::styled(
        "Package Cache Information",
        Style::default()
            .fg(theme.success())
            .add_modifier(Modifier::BOLD),
    )])];
    lines.push(Line::from(""));

    let total_size: u64 = app.cache_info.iter().map(|c| c.size_bytes).sum();

    if app.cache_info.is_empty() {
        lines.push(Line::from("No cache directories found."));
    } else {
        lines.push(Line::from(format!(
            "Total cache size: {}",
            format_cache_size(total_size)
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(
                "Cache Location",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw("                    "),
            Span::styled("Size", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw("  "),
            Span::styled("Files", Style::default().add_modifier(Modifier::BOLD)),
        ]));
        lines.push(Line::from("-".repeat(55)));

        for cache in &app.cache_info {
            let path = if cache.path.len() > 25 {
                format!("...{}", &cache.path[cache.path.len() - 22..])
            } else {
                cache.path.clone()
            };
            lines.push(Line::from(format!(
                "{:<28} {:>10} {:>8} files",
                path, cache.size_formatted, cache.file_count
            )));
        }

        lines.push(Line::from(""));
        lines.push(Line::from("To clean cache, run:"));
        lines.push(Line::from(
            "  sudo pacman -Scc          # Clean all pacman cache",
        ));
        lines.push(Line::from(
            "  rm -rf ~/.cache/paru     # Clean AUR helper cache",
        ));
    }
    lines.push(Line::from(""));
    lines.push(Line::from("Press 'Esc' to close"));

    let para = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Thick)
                .title("Cache Info")
                .border_style(Style::default().fg(theme.success())),
        )
        .wrap(ratatui::widgets::Wrap { trim: true });

    let popup = centered_rect(70, 50, area);
    f.render_widget(Clear, popup);
    f.render_widget(para, popup);
}

fn format_cache_size(bytes: u64) -> String {
    crate::models::Package::format_size_bytes(bytes)
}

fn render_foreign_overlay(app: &App, f: &mut Frame, area: Rect, theme: &crate::theme::Theme) {
    let repo_count = crate::diagnostics::get_repository_packages_count();
    let foreign_count = app.foreign_packages.len();
    let total_count = repo_count + foreign_count;

    let mut lines = vec![Line::from(vec![Span::styled(
        "Foreign Packages (AUR/Other)",
        Style::default()
            .fg(theme.aur_color())
            .add_modifier(Modifier::BOLD),
    )])];
    lines.push(Line::from(""));

    lines.push(Line::from(format!(
        "Repository: {} packages | Foreign: {} packages | Total: {}",
        repo_count, foreign_count, total_count
    )));
    lines.push(Line::from(""));

    if app.foreign_packages.is_empty() {
        lines.push(Line::from(
            "No foreign packages found. All packages are from official repositories.",
        ));
    } else {
        lines.push(Line::from(format!("{} foreign package(s):", foreign_count)));
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Package", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw("            "),
            Span::styled("Version", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw("  "),
            Span::styled("Source", Style::default().add_modifier(Modifier::BOLD)),
        ]));
        lines.push(Line::from("-".repeat(50)));

        for pkg in &app.foreign_packages {
            let name = if pkg.name.len() > 20 {
                format!("{}...", &pkg.name[..17])
            } else {
                pkg.name.clone()
            };
            lines.push(Line::from(format!(
                "{:<22} {:<15} {}",
                name, pkg.version, pkg.source
            )));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from("Press 'Esc' to close"));

    let para = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Thick)
                .title("Foreign Packages")
                .border_style(Style::default().fg(theme.aur_color())),
        )
        .wrap(ratatui::widgets::Wrap { trim: true });

    let popup = centered_rect(70, 60, area);
    f.render_widget(Clear, popup);
    f.render_widget(para, popup);
}

fn render_groups_overlay(app: &App, f: &mut Frame, area: Rect, theme: &crate::theme::Theme) {
    let mut lines = vec![Line::from(vec![Span::styled(
        "Package Groups",
        Style::default()
            .fg(theme.info())
            .add_modifier(Modifier::BOLD),
    )])];
    lines.push(Line::from(""));

    if app.package_groups.is_empty() {
        lines.push(Line::from("No package groups available."));
    } else {
        lines.push(Line::from(format!(
            "Available groups: {}",
            app.package_groups.len()
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Group", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw("               "),
            Span::styled("Members", Style::default().add_modifier(Modifier::BOLD)),
        ]));
        lines.push(Line::from("-".repeat(40)));

        for group in &app.package_groups {
            lines.push(Line::from(format!(
                "{:<20} {:>3} packages",
                group.name, group.member_count
            )));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from("Press 'Esc' to close"));

    let para = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Thick)
                .title("Package Groups")
                .border_style(Style::default().fg(theme.info())),
        )
        .wrap(ratatui::widgets::Wrap { trim: true });

    let popup = centered_rect(60, 60, area);
    f.render_widget(Clear, popup);
    f.render_widget(para, popup);
}

fn render_pacnew_pacsave_overlay(
    app: &App,
    f: &mut Frame,
    area: Rect,
    theme: &crate::theme::Theme,
) {
    let mut lines = vec![Line::from(vec![Span::styled(
        "Config Files (.pacnew / .pacsave)",
        Style::default()
            .fg(theme.info())
            .add_modifier(Modifier::BOLD),
    )])];
    lines.push(Line::from(""));
    if app.pacnew_pacsave_files.is_empty() {
        lines.push(Line::from("No .pacnew or .pacsave files found in /etc."));
    } else {
        lines.push(Line::from(format!(
            "Found {} file(s):",
            app.pacnew_pacsave_files.len()
        )));
        lines.push(Line::from(""));
        for (idx, file) in app.pacnew_pacsave_files.iter().enumerate() {
            let indicator = if idx == app.pacnew_cursor.unwrap_or(0) {
                "▶"
            } else {
                " "
            };
            let file_type_str = match file.file_type {
                crate::services::PacnewType::New => ".pacnew",
                crate::services::PacnewType::Save => ".pacsave",
            };
            lines.push(Line::from(vec![
                Span::raw(indicator),
                Span::raw(" "),
                Span::styled(&file.original_name, Style::default().fg(CYAN)),
                Span::raw(file_type_str),
            ]));
            if idx == app.pacnew_cursor.unwrap_or(0) {
                lines.push(Line::from(vec![
                    Span::raw("   "),
                    Span::styled(
                        &file.path,
                        Style::default()
                            .fg(theme.muted())
                            .add_modifier(Modifier::DIM),
                    ),
                ]));
            }
        }
        lines.push(Line::from(""));
        lines.push(Line::from(
            "Keys: [↑/↓ or j/k] Navigate  [d] Delete  [Esc] Close",
        ));
    }
    lines.push(Line::from(""));
    lines.push(Line::from("Press 'N' to toggle this view"));

    let para = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Thick)
                .title("Config Files")
                .border_style(Style::default().fg(theme.info())),
        )
        .wrap(ratatui::widgets::Wrap { trim: true });

    let popup = centered_rect(70, 60, area);
    f.render_widget(Clear, popup);
    f.render_widget(para, popup);
}

fn render_pacman_log_overlay(app: &App, f: &mut Frame, area: Rect, theme: &crate::theme::Theme) {
    let mut lines = vec![Line::from(vec![Span::styled(
        "Pacman Log",
        Style::default()
            .fg(theme.info())
            .add_modifier(Modifier::BOLD),
    )])];
    lines.push(Line::from(""));
    lines.push(Line::from(
        "[1] All  [2] Installed  [3] Removed  [4] Upgraded",
    ));

    let filtered: Vec<_> = if let Some(filter) = &app.pacman_log_filter {
        app.pacman_log_entries
            .iter()
            .filter(|e| &e.operation == filter)
            .collect()
    } else {
        app.pacman_log_entries.iter().collect()
    };

    lines.push(Line::from(""));
    if filtered.is_empty() {
        lines.push(Line::from("No log entries found."));
    } else {
        lines.push(Line::from(format!("Showing {} entries:", filtered.len())));
        lines.push(Line::from(""));
        for entry in filtered.iter().take(30) {
            let op_str = match &entry.operation {
                crate::services::LogOperation::Installed => "[INSTALLED]",
                crate::services::LogOperation::Removed => "[REMOVED]",
                crate::services::LogOperation::Upgraded => "[UPGRADED]",
                crate::services::LogOperation::Downgraded => "[DOWNGRADED]",
            };
            let color = match &entry.operation {
                crate::services::LogOperation::Installed => theme.success(),
                crate::services::LogOperation::Removed => theme.error(),
                crate::services::LogOperation::Upgraded => theme.info(),
                crate::services::LogOperation::Downgraded => theme.warning(),
            };
            lines.push(Line::from(vec![
                Span::styled(op_str, Style::default().fg(color)),
                Span::raw(" "),
                Span::styled(&entry.package, Style::default().fg(CYAN)),
                Span::raw(" "),
                Span::raw(&entry.timestamp),
            ]));
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::from(
        "Keys: [↑/↓ or j/k] Scroll  [1-4] Filter  [Esc/L] Close",
    ));

    let para = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Thick)
                .title("Pacman Log")
                .border_style(Style::default().fg(theme.info())),
        )
        .wrap(ratatui::widgets::Wrap { trim: true });

    let popup = centered_rect(75, 70, area);
    f.render_widget(Clear, popup);
    f.render_widget(para, popup);
}

fn render_downgrade_modal(app: &App, f: &mut Frame, area: Rect, theme: &crate::theme::Theme) {
    let pkg_name = app.downgrade_package.as_deref().unwrap_or("Unknown");

    let mut lines = vec![Line::from(vec![Span::styled(
        format!("Downgrade: {}", pkg_name),
        Style::default()
            .fg(theme.warning())
            .add_modifier(Modifier::BOLD),
    )])];
    lines.push(Line::from(""));
    lines.push(Line::from("Select a version to downgrade:"));
    lines.push(Line::from(""));

    if app.available_versions.is_empty() {
        lines.push(Line::from("No older versions available."));
    } else {
        for (idx, version) in app.available_versions.iter().enumerate() {
            let indicator = if idx == app.downgrade_cursor.unwrap_or(0) {
                "▶"
            } else {
                " "
            };
            lines.push(Line::from(vec![
                Span::raw(indicator),
                Span::raw(" "),
                Span::styled(&version.version, Style::default().fg(CYAN)),
                Span::raw(" ("),
                Span::raw(&version.date),
                Span::raw(")"),
            ]));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(
        "Keys: [↑/↓ or j/k] Navigate  [Enter] Select  [Esc] Cancel",
    ));

    let para = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Thick)
                .title("Package Downgrade")
                .border_style(Style::default().fg(theme.warning())),
        )
        .wrap(ratatui::widgets::Wrap { trim: true });

    let popup = centered_rect(60, 60, area);
    f.render_widget(Clear, popup);
    f.render_widget(para, popup);
}

fn render_health_dashboard_overlay(
    app: &App,
    f: &mut Frame,
    area: Rect,
    theme: &crate::theme::Theme,
) {
    let mut lines = vec![Line::from(vec![Span::styled(
        "System Health Dashboard",
        Style::default()
            .fg(theme.success())
            .add_modifier(Modifier::BOLD),
    )])];
    lines.push(Line::from(""));

    // Disk Space
    lines.push(Line::from(vec![Span::styled(
        "Disk Space:",
        Style::default().add_modifier(Modifier::BOLD),
    )]));
    if app.health_disk_info.is_empty() {
        lines.push(Line::from("  No disk data available"));
    } else {
        for disk in &app.health_disk_info {
            let used_mb = disk.used_bytes as f64 / 1024.0 / 1024.0;
            let total_mb = disk.total_bytes as f64 / 1024.0 / 1024.0;
            let color = if disk.usage_percent > 90.0 {
                theme.error()
            } else if disk.usage_percent > 75.0 {
                theme.warning()
            } else {
                theme.success()
            };
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(&disk.mount_point, Style::default().fg(CYAN)),
                Span::raw(": "),
                Span::styled(
                    format!(
                        "{:.1} / {:.1} MB ({:.1}%)",
                        used_mb, total_mb, disk.usage_percent
                    ),
                    Style::default().fg(color),
                ),
            ]));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        "Pacman Status:",
        Style::default().add_modifier(Modifier::BOLD),
    )]));
    if let Some(status) = &app.health_pacman_status {
        let db_color = if status.db_locked {
            theme.error()
        } else {
            theme.success()
        };
        let gpg_color = if status.gpg_valid {
            theme.success()
        } else {
            theme.error()
        };
        lines.push(Line::from(vec![
            Span::raw("  Database lock: "),
            Span::styled(
                if status.db_locked {
                    "LOCKED"
                } else {
                    "Available"
                },
                Style::default().fg(db_color),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::raw("  GPG keys: "),
            Span::styled(
                if status.gpg_valid {
                    "Valid"
                } else {
                    "Invalid/Expired"
                },
                Style::default().fg(gpg_color),
            ),
        ]));
    } else {
        lines.push(Line::from("  Status unknown"));
    }

    lines.push(Line::from(""));
    lines.push(Line::from("Press 'H' to toggle this view"));

    let para = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Thick)
                .title("Health Dashboard")
                .border_style(Style::default().fg(theme.success())),
        )
        .wrap(ratatui::widgets::Wrap { trim: true });

    let popup = centered_rect(65, 50, area);
    f.render_widget(Clear, popup);
    f.render_widget(para, popup);
}

fn render_console(app: &App, f: &mut Frame, theme: &crate::theme::Theme) {
    let console_title = if app.command_stdin_tx.is_some() {
        "Console Output (interactive)"
    } else {
        "Console Output"
    };

    let console_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Thick)
        .title(console_title)
        .style(Style::default().bg(Color::Black).fg(theme.foreground()))
        .border_style(Style::default().fg(theme.info()));

    // Show the full buffer with scrolling support
    let logs: Vec<ListItem> = app
        .console_buffer
        .iter()
        .map(|l| {
            let style = if l.contains("[stderr]")
                || l.contains("error")
                || l.contains("Error")
                || l.contains("failed")
            {
                Style::default().fg(theme.error())
            } else if l.contains("warning") || l.contains("Warning") {
                Style::default().fg(theme.warning())
            } else if l.contains("success") || l.contains("completed") || l.contains("Finished") {
                Style::default().fg(theme.success())
            } else {
                Style::default().fg(theme.foreground())
            };
            ListItem::new(Line::from(vec![Span::styled(l.clone(), style)]))
        })
        .collect();

    let list = List::new(logs).block(console_block).highlight_style(
        Style::default()
            .fg(theme.highlight_fg())
            .bg(theme.highlight_bg()),
    );

    let chunk = centered_rect(85, 85, f.size());
    f.render_widget(Clear, chunk);

    let mut state = ListState::default();
    state.select(app.console_cursor);
    f.render_stateful_widget(list, chunk, &mut state);

    if let Some(mut scroll_state) = app.console_scroll_state {
        let scrollbar_area = Rect {
            x: chunk.x + chunk.width - 1,
            y: chunk.y + 1,
            width: 1,
            height: chunk.height.saturating_sub(2),
        };

        let items_len = app.console_buffer.len();
        scroll_state = scroll_state.content_length(items_len);
        if let Some(cursor) = app.console_cursor {
            scroll_state = scroll_state.position(cursor);
        }

        render_scrollbar(f, scrollbar_area, &mut scroll_state, theme.info());
    }

    if app.command_stdin_tx.is_some() {
        let input_area = Rect {
            x: chunk.x + 2,
            y: chunk.y + chunk.height.saturating_sub(2),
            width: chunk.width.saturating_sub(4),
            height: 1,
        };
        let input_prompt = if app.console_input.is_empty() {
            "Input: ".to_string()
        } else {
            format!("Input: {}", app.console_input)
        };
        let input = Paragraph::new(input_prompt).style(Style::default().fg(theme.primary()));
        f.render_widget(input, input_area);
    }

    // Render progress bar if available
    if let Some(progress) = &app.command_progress {
        let progress_y = if app.command_stdin_tx.is_some() {
            chunk.y + chunk.height.saturating_sub(5)
        } else {
            chunk.y + chunk.height.saturating_sub(4)
        };

        // Package progress bar
        let progress_area = Rect {
            x: chunk.x + 2,
            y: progress_y,
            width: chunk.width - 4,
            height: 1,
        };

        let ratio = progress.current as f64 / progress.total as f64;

        // Build label with download info if available
        let label = if let Some(speed) = &progress.download_speed {
            if let (Some(downloaded), Some(total)) =
                (progress.downloaded_bytes, progress.total_bytes)
            {
                let downloaded_mb = downloaded as f64 / 1024.0 / 1024.0;
                let total_mb = total as f64 / 1024.0 / 1024.0;
                format!(
                    "{} ({}/{}) {} - {:.1}/{:.1} MB",
                    progress.current_package,
                    progress.current,
                    progress.total,
                    speed,
                    downloaded_mb,
                    total_mb
                )
            } else {
                format!(
                    "{} ({}/{}) {}",
                    progress.current_package, progress.current, progress.total, speed
                )
            }
        } else {
            format!(
                " {} {}/{} ",
                progress.current_package, progress.current, progress.total
            )
        };

        let gauge = Gauge::default()
            .ratio(ratio.min(1.0))
            .label(&label)
            .style(Style::default().fg(theme.success()).bg(theme.muted()));

        f.render_widget(gauge, progress_area);
    }
}

fn render_package_details(app: &App, f: &mut Frame, theme: &crate::theme::Theme) {
    if let Some(pkg) = app.get_selected_package() {
        let details_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Thick)
            .title(format!(
                "📦 Package Details: {} ({})",
                pkg.name, pkg.version
            ))
            .style(
                Style::default()
                    .bg(theme.background())
                    .fg(theme.foreground()),
            )
            .border_style(Style::default().fg(theme.info()));

        let area = centered_rect(80, 80, f.size());
        f.render_widget(Clear, area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(10),
                Constraint::Min(1),
                Constraint::Length(12),
                Constraint::Length(3),
            ])
            .margin(2)
            .split(area);

        // Title
        let title = Paragraph::new(Line::from(vec![
            Span::styled(
                "📦 Package: ",
                Style::default()
                    .fg(theme.info())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(&pkg.name, Style::default().fg(theme.primary())),
            Span::raw(" | "),
            Span::styled(
                "Version: ",
                Style::default()
                    .fg(theme.warning())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(&pkg.version, Style::default().fg(theme.warning())),
        ]))
        .alignment(Alignment::Center);
        f.render_widget(title, chunks[0]);

        // Basic info
        let mut info_spans = vec![
            Span::styled(
                "📋 Description: ",
                Style::default()
                    .fg(theme.info())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(&pkg.description),
            Span::raw("\n\n"),
        ];

        if let Some(url) = &pkg.url {
            info_spans.push(Span::styled(
                "🔗 URL: ",
                Style::default()
                    .fg(theme.info())
                    .add_modifier(Modifier::BOLD),
            ));
            info_spans.push(Span::styled(url, Style::default().fg(theme.primary())));
            info_spans.push(Span::raw("\n"));
        }

        if !pkg.licenses.is_empty() {
            info_spans.push(Span::styled(
                "⚖️  License(s): ",
                Style::default()
                    .fg(theme.success())
                    .add_modifier(Modifier::BOLD),
            ));
            info_spans.push(Span::raw(pkg.licenses.join(", ")));
            info_spans.push(Span::raw("\n"));
        }

        if !pkg.groups.is_empty() {
            info_spans.push(Span::styled(
                "📁 Groups: ",
                Style::default()
                    .fg(theme.secondary())
                    .add_modifier(Modifier::BOLD),
            ));
            info_spans.push(Span::raw(pkg.groups.join(", ")));
            info_spans.push(Span::raw("\n"));
        }

        if !pkg.maintainers.is_empty() {
            info_spans.push(Span::styled(
                "👤 Maintainer(s): ",
                Style::default()
                    .fg(theme.info())
                    .add_modifier(Modifier::BOLD),
            ));
            info_spans.push(Span::raw(pkg.maintainers.join(", ")));
            info_spans.push(Span::raw("\n"));
        }

        if !pkg.keywords.is_empty() {
            info_spans.push(Span::styled(
                "🏷️  Keywords: ",
                Style::default()
                    .fg(theme.secondary())
                    .add_modifier(Modifier::BOLD),
            ));
            info_spans.push(Span::raw(pkg.keywords.join(", ")));
            info_spans.push(Span::raw("\n"));
        }

        if let Some(size) = pkg.installed_size {
            info_spans.push(Span::styled(
                "💾 Installed Size: ",
                Style::default()
                    .fg(theme.warning())
                    .add_modifier(Modifier::BOLD),
            ));
            info_spans.push(Span::raw(format!("{} KB", size)));
            info_spans.push(Span::raw("\n"));
        }

        if let Some(size) = pkg.download_size {
            info_spans.push(Span::styled(
                "⬇️  Download Size: ",
                Style::default()
                    .fg(theme.warning())
                    .add_modifier(Modifier::BOLD),
            ));
            info_spans.push(Span::raw(format!("{} KB", size)));
            info_spans.push(Span::raw("\n"));
        }

        let info_para =
            Paragraph::new(Line::from(info_spans)).wrap(ratatui::widgets::Wrap { trim: true });
        f.render_widget(info_para, chunks[1]);

        // Description
        let desc_para = Paragraph::new(pkg.description.clone())
            .wrap(ratatui::widgets::Wrap { trim: true })
            .style(Style::default().fg(theme.foreground()));
        f.render_widget(desc_para, chunks[2]);

        // Dependencies
        let deps_title = Paragraph::new(Line::from(vec![Span::styled(
            "📦 Dependencies: ",
            Style::default()
                .fg(theme.info())
                .add_modifier(Modifier::BOLD),
        )]));
        f.render_widget(deps_title, chunks[3]);

        let deps_list_items: Vec<ListItem> = pkg
            .depends_on
            .iter()
            .map(|dep| ListItem::new(format!("  • {}", dep)))
            .collect();

        let extra_dep_lines = [
            ("Optional", &pkg.opt_depends),
            ("Required By", &pkg.required_by),
            ("Conflicts", &pkg.conflicts),
            ("Provides", &pkg.provides),
            ("Replaces", &pkg.replaces),
        ];
        let mut deps_list_items = deps_list_items;
        for (label, items) in extra_dep_lines {
            if !items.is_empty() {
                deps_list_items.push(ListItem::new(format!(
                    "  • {}: {}",
                    label,
                    items.join(", ")
                )));
            }
        }

        let deps_list = List::new(deps_list_items)
            .block(Block::default().borders(Borders::NONE))
            .style(Style::default().fg(theme.success()));

        f.render_widget(deps_list, chunks[3]);

        // Footer
        let footer = Paragraph::new("Press 'Esc' to return | 'v' for dependencies")
            .style(Style::default().fg(theme.muted()))
            .alignment(Alignment::Center);
        f.render_widget(footer, chunks[4]);

        f.render_widget(details_block, area);
    }
}

fn render_dependency_visualization(app: &App, f: &mut Frame, theme: &crate::theme::Theme) {
    if let Some(pkg) = app.get_selected_package() {
        let viz_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Thick)
            .title(format!("🌳 Dependency Tree: {}", pkg.name))
            .style(
                Style::default()
                    .bg(theme.background())
                    .fg(theme.foreground()),
            )
            .border_style(Style::default().fg(theme.secondary()));

        let area = centered_rect(85, 85, f.size());
        f.render_widget(Clear, area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(1),
                Constraint::Length(3),
            ])
            .margin(2)
            .split(area);

        // Title
        let title = Paragraph::new(Line::from(vec![
            Span::styled(
                "🌳 Package: ",
                Style::default()
                    .fg(theme.info())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(&pkg.name, Style::default().fg(theme.primary())),
        ]))
        .alignment(Alignment::Center);
        f.render_widget(title, chunks[0]);

        if let Some(tree) = &app.interactive_dependency_tree {
            let flattened = crate::dependency_visualization::DependencyVisualizationService::flatten_interactive_tree(tree);
            let items: Vec<ListItem> = flattened
                .iter()
                .map(|item| {
                    let style = if item.is_orphan {
                        Style::default().fg(theme.warning())
                    } else if item.is_installed {
                        Style::default().fg(theme.success())
                    } else {
                        Style::default().fg(theme.foreground())
                    };
                    ListItem::new(item.full_display_name.clone()).style(style)
                })
                .collect();

            let mut state = ratatui::widgets::ListState::default();
            state.select(app.dependency_tree_cursor);

            let list = List::new(items)
                .block(Block::default().borders(Borders::NONE))
                .highlight_style(
                    Style::default()
                        .fg(theme.highlight_fg())
                        .bg(theme.highlight_bg())
                        .add_modifier(Modifier::BOLD),
                );

            f.render_stateful_widget(list, chunks[1], &mut state);
        } else {
            let empty = Paragraph::new("No dependency information available.")
                .alignment(Alignment::Center)
                .style(Style::default().fg(theme.muted()));
            f.render_widget(empty, chunks[1]);
        }

        // Footer
        let footer = Paragraph::new("Press 'Esc' to return")
            .style(Style::default().fg(theme.muted()))
            .alignment(Alignment::Center);
        f.render_widget(footer, chunks[2]);

        f.render_widget(viz_block, area);
    }
}

fn render_help_overlay(f: &mut Frame, area: Rect, theme: &crate::theme::Theme) {
    let help_text = vec![
        Line::from(vec![Span::styled(
            "metapak - Universal Package Manager TUI",
            Style::default()
                .fg(theme.primary())
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from("Now with support for Ecosystem Packages (npm, cargo, pip)"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Keyboard Shortcuts",
            Style::default()
                .fg(theme.primary())
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::UNDERLINED),
        )]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Navigation",
            Style::default()
                .fg(theme.info())
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from("  ↑/k       Move up"),
        Line::from("  ↓/j       Move down"),
        Line::from("  n         Next page"),
        Line::from("  p         Previous page"),
        Line::from("  Home      First page"),
        Line::from("  End       Last page"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Search & Filter",
            Style::default()
                .fg(theme.info())
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from("  / or i    Enter search mode"),
        Line::from("  ↑/↓       Navigate search history (in search mode)"),
        Line::from("  f         Cycle filter (All → Installed → Not Installed → Repo → AUR)"),
        Line::from("  s         Cycle sort (Name ↑ → Name ↓ → Source)"),
        Line::from("  r         Clear results and selection"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Actions",
            Style::default()
                .fg(theme.info())
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from("  Tab       Switch View (System/Ecosystem)"),
        Line::from("  [ / ]     Cycle Ecosystem Provider (npm/cargo/pip)"),
        Line::from("  Space     Toggle package selection"),
        Line::from("  u         Undo last selection"),
        Line::from("  Enter     Install/Remove selected packages"),
        Line::from("  y/n       Confirm/Cancel operation"),
        Line::from("  U         View/manage updates"),
        Line::from("  shift+U   Update system"),
        Line::from("  d         Show package details"),
        Line::from("  v         Show dependency tree"),
        Line::from("  o         Open package in browser"),
        Line::from("  \\         Toggle sidebar (details view)"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "General",
            Style::default()
                .fg(theme.info())
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from("  U         Updates view (a=select all, n=none, space=toggle)"),
        Line::from("  h         System diagnostics"),
        Line::from("  I         System information"),
        Line::from("  O         Orphan packages"),
        Line::from("  P         Package sizes (top 30)"),
        Line::from("  C         Cache information"),
        Line::from("  F         Foreign packages (AUR)"),
        Line::from("  G         Package groups"),
        Line::from("  ?         Toggle this help"),
        Line::from("  q         Quit application"),
        Line::from("  Esc       Cancel/Go back"),
    ];

    let help_para = Paragraph::new(help_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Thick)
                .title("Help")
                .border_style(Style::default().fg(theme.primary())),
        )
        .style(Style::default().fg(theme.foreground()));

    let help_area = centered_rect(70, 80, area);
    f.render_widget(Clear, help_area);
    f.render_widget(help_para, help_area);
}

fn render_simulation_modal(app: &App, f: &mut Frame, theme: &crate::theme::Theme) {
    let area = centered_rect(60, 60, f.size());
    f.render_widget(Clear, area);

    let block = Block::default()
        .title("🔍 Operation Simulation")
        .borders(Borders::ALL)
        .border_type(BorderType::Thick)
        .border_style(Style::default().fg(theme.primary()));

    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .margin(2)
        .split(area);

    if let Some(result) = &app.simulation_result {
        // Summary
        let download_size = crate::models::Package::format_size_bytes(result.total_download_bytes);
        let disk_change = if result.disk_change_bytes >= 0 {
            format!(
                "+{}",
                crate::models::Package::format_size_bytes(result.disk_change_bytes as u64)
            )
        } else {
            format!(
                "-{}",
                crate::models::Package::format_size_bytes(result.disk_change_bytes.unsigned_abs())
            )
        };

        let summary = Paragraph::new(vec![
            Line::from(vec![
                Span::styled("Download Size: ", Style::default().fg(theme.muted())),
                Span::styled(download_size, Style::default().fg(theme.foreground())),
            ]),
            Line::from(vec![
                Span::styled("Disk Change:   ", Style::default().fg(theme.muted())),
                Span::styled(disk_change, Style::default().fg(theme.foreground())),
            ]),
        ]);
        f.render_widget(summary, chunks[0]);

        // Conflicts and Config Changes
        let mut lines = Vec::new();
        if !result.conflicts.is_empty() {
            lines.push(Line::from(vec![Span::styled(
                "Conflicts Found:",
                Style::default()
                    .fg(theme.error())
                    .add_modifier(Modifier::BOLD),
            )]));
            for conflict in &result.conflicts {
                lines.push(Line::from(format!("  • {}", conflict)));
            }
            lines.push(Line::from(""));
        }

        if !result.config_changes.is_empty() {
            lines.push(Line::from(vec![Span::styled(
                "Configuration Changes:",
                Style::default()
                    .fg(theme.warning())
                    .add_modifier(Modifier::BOLD),
            )]));
            for config in &result.config_changes {
                lines.push(Line::from(format!("  • {}", config)));
            }
        }

        if result.conflicts.is_empty() && result.config_changes.is_empty() {
            lines.push(Line::from("No conflicts or major config changes detected."));
        }

        let details = Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: true });
        f.render_widget(details, chunks[1]);
    } else {
        let loading = Paragraph::new("Simulating operation...")
            .alignment(Alignment::Center)
            .style(Style::default().fg(theme.muted()));
        f.render_widget(loading, chunks[1]);
    }

    let footer = Paragraph::new("Press 'Enter' to Proceed | 'Esc' to Cancel")
        .alignment(Alignment::Center)
        .style(Style::default().fg(theme.muted()));
    f.render_widget(footer, chunks[2]);
}

fn render_rollback_dialog(app: &App, f: &mut Frame, theme: &crate::theme::Theme) {
    let area = centered_rect(50, 30, f.size());
    f.render_widget(Clear, area);

    let block = Block::default()
        .title("🛡️  Rollback Recommended")
        .borders(Borders::ALL)
        .border_type(BorderType::Thick)
        .border_style(Style::default().fg(theme.error()));

    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(3)])
        .margin(2)
        .split(area);

    let msg = Paragraph::new(vec![
        Line::from("The operation failed. Would you like to rollback to the snapshot created before this operation?"),
        Line::from(""),
        Line::from(vec![
            Span::styled("Snapshot ID: ", Style::default().fg(theme.muted())),
            Span::styled(
                app.pending_rollback_id.as_deref().unwrap_or("Unknown"),
                Style::default().fg(theme.foreground()),
            ),
        ]),
    ]).wrap(ratatui::widgets::Wrap { trim: true });
    f.render_widget(msg, chunks[0]);

    let footer = Paragraph::new("Press 'y' to Rollback | 'n' to Dismiss")
        .alignment(Alignment::Center)
        .style(Style::default().fg(theme.muted()));
    f.render_widget(footer, chunks[1]);
}
