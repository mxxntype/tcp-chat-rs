use super::{registry::EditingMode, Registry};
use super::{App, Stage};
use ratatui::backend::Backend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::widgets::{Block, Paragraph, Wrap};
use ratatui::{text::Line, Frame};
use std::{io, rc::Rc};

impl<B> App<B>
where
    B: Backend + Send + Sync,
{
    /// Render the UI.
    pub(super) fn render_ui(&mut self) -> io::Result<()> {
        match &self.stage {
            Stage::NotLoggedIn { registry: _ } => {
                self.render_login_screen()?;
            }

            Stage::LoggedIn { chat: _ } => {
                self.render_main_screen()?;
            }
        }

        Ok(())
    }

    /// Draw the main screen of the application.
    fn render_main_screen(&mut self) -> Result<(), io::Error> {
        if let Stage::LoggedIn { chat } = &self.stage {
            self.terminal.draw(|frame| {
                frame.render_widget(
                    Paragraph::new(format!("{chat:#?}"))
                        .style(Style::default().bold())
                        .block(Block::bordered())
                        .wrap(Wrap { trim: false }),
                    frame.size(),
                );
            })?;
        }

        Ok(())
    }

    /// Render the login screen.
    fn render_login_screen(&mut self) -> Result<(), io::Error> {
        if let Stage::NotLoggedIn { registry } = &self.stage {
            self.terminal.draw(|frame| {
                let vertical_areas = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Fill(2),
                        Constraint::Length(8),
                        Constraint::Fill(3),
                    ])
                    .split(frame.size());
                let horizontal_areas = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Fill(1),
                        Constraint::Percentage(80),
                        Constraint::Fill(1),
                    ])
                    .split(vertical_areas[1]);

                // Render the borders of the input area, as well as navigation hints.
                let input_area = horizontal_areas[1];
                let hint_style = Style::default().italic().dark_gray();
                let title_top = " Welcome! Log into your account... ";
                let title_top = Line::styled(title_top, Style::default().bold().magenta());
                let title_bottom_left = Line::styled(" <Tab> to switch field ", hint_style);
                let title_bottom_right = Line::styled(" <Enter> to log in ", hint_style);
                let mut input_area_border = Block::bordered()
                    .border_style(Style::new().bold().dark_gray())
                    .title_top(title_top.centered())
                    .title_bottom(title_bottom_left.left_aligned())
                    .title_bottom(title_bottom_right.right_aligned());
                if registry.failed {
                    input_area_border = input_area_border.title_bottom(
                        Line::styled(
                            "Invalid username or password!",
                            Style::default().bold().red(),
                        )
                        .centered(),
                    );
                }
                frame.render_widget(input_area_border, input_area);

                // Split the input are in half vertically.
                let input_area_parts = Layout::vertical(Constraint::from_lengths([3, 3]))
                    .vertical_margin(1)
                    .horizontal_margin(2)
                    .flex(ratatui::layout::Flex::Start)
                    .split(input_area);

                render_username_prompt(registry, &input_area_parts, frame);
                render_password_prompt(registry, &input_area_parts, frame);
            })?;
        } else {
            unreachable!()
        }

        Ok(())
    }
}

fn render_username_prompt(
    registry: &Registry,
    input_area_halves: &Rc<[Rect]>,
    frame: &mut Frame<'_>,
) {
    let focused = *registry.editing_mode() == EditingMode::Username;
    let username_area = Layout::horizontal([Constraint::Length(10), Constraint::Fill(1)])
        .split(input_area_halves[0]);

    let username = format!(" {}{}", registry.username, if focused { "_" } else { "" });
    let mut username_label = Paragraph::new("\nUsername:");
    let mut username_field = Paragraph::new(username)
        .style(Style::default().bold())
        .block(Block::bordered());
    if focused {
        username_label = username_label.style(Style::default().bold());
        username_field =
            username_field.block(Block::bordered().border_style(Style::default().magenta()));
    }

    frame.render_widget(username_label, username_area[0]);
    frame.render_widget(username_field, username_area[1]);
}

fn render_password_prompt(
    registry: &Registry,
    input_area_halves: &Rc<[Rect]>,
    frame: &mut Frame<'_>,
) {
    let focused: bool = *registry.editing_mode() == EditingMode::Password;
    let password_area = Layout::horizontal([Constraint::Length(10), Constraint::Fill(1)])
        .split(input_area_halves[1]);

    let password = format!(
        " {}{}",
        registry.password.chars().map(|_| '*').collect::<String>(),
        if focused { "_" } else { "" }
    );

    let mut password_label = Paragraph::new("\nPassword:");
    let mut password_field = Paragraph::new(password.as_str())
        .style(Style::default().bold())
        .block(Block::bordered());
    if focused {
        password_label = password_label.style(Style::default().bold());
        password_field =
            password_field.block(Block::bordered().border_style(Style::default().magenta()));
    }

    frame.render_widget(password_label, password_area[0]);
    frame.render_widget(password_field, password_area[1]);
}
