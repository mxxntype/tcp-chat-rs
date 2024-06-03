pub mod chat;
pub mod interceptor;
pub mod registry;

pub use chat::Chat;
pub use interceptor::Interceptor;
pub use registry::Registry;

use crate::app::registry::EditingMode;
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Style, Stylize};
use ratatui::widgets::{Block, Paragraph, Wrap};
use ratatui::{text::Line, Terminal};
use std::{io, panic, time::Duration};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

#[derive(Debug)]
pub enum Stage {
    NotLoggedIn(Registry),
    LoggedIn(Chat<Interceptor>),
}

#[derive(Debug)]
pub struct App<B>
where
    B: Backend,
{
    stage: Stage,
    terminal: Terminal<B>,
}

impl App<CrosstermBackend<io::Stderr>> {
    pub async fn new() -> Self {
        let backend = CrosstermBackend::new(io::stderr());
        let terminal = Terminal::new(backend).unwrap();
        let registry = Registry::new().await;

        Self {
            stage: Stage::NotLoggedIn(registry),
            terminal,
        }
    }
}

impl<B> App<B>
where
    B: Backend + Sync,
{
    pub async fn run(&mut self) -> io::Result<()> {
        self.setup_terminal()?;

        let (canceller_thread, cancel_signal, _) = Self::canceller_thread::<()>();

        // TODO: Summon server threads.
        // let _ = tokio::spawn(async move {
        //     tokio::select! {
        //         _ = token.cancelled() => {}
        //     }
        // });

        loop {
            self.render_ui()?;

            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(event) = event::read()? {
                    // HACK: Skip key release events.
                    if event.kind == event::KeyEventKind::Release {
                        continue;
                    }

                    // Quit with no questions asked if the user hits Escape.
                    if event.code == KeyCode::Esc {
                        let _ = cancel_signal.send(());
                        break;
                    }

                    // A flag that turns to `true` when the users confirms the credentials.
                    let mut attempt_login = false;

                    match &mut self.stage {
                        Stage::NotLoggedIn(registry) => match event.code {
                            KeyCode::Char(c) => {
                                registry.failed = false;
                                match registry.editing_mode() {
                                    registry::EditingMode::Username => registry.username.push(c),
                                    registry::EditingMode::Password => registry.password.push(c),
                                }
                            }

                            KeyCode::Backspace | KeyCode::Delete => {
                                registry.failed = false;
                                match registry.editing_mode() {
                                    registry::EditingMode::Username => registry.username.pop(),
                                    registry::EditingMode::Password => registry.password.pop(),
                                };
                            }

                            KeyCode::Enter => match registry.editing_mode() {
                                registry::EditingMode::Username => registry.toggle_mode(),
                                registry::EditingMode::Password => attempt_login = true,
                            },

                            KeyCode::Tab | KeyCode::BackTab => registry.toggle_mode(),

                            _ => {}
                        },

                        Stage::LoggedIn(_) => {}
                    }

                    // Attempt to login with the provided credentials, anvancing to `Stage::LoggedIn` if successful.
                    //
                    // TODO: If the credentials are incorrect or something goes wrong, notify the user of the error.
                    if attempt_login {
                        if let Stage::NotLoggedIn(registry) = &mut self.stage {
                            match registry.clone().into_chat().await {
                                Ok(chat) => self.stage = Stage::LoggedIn(chat),
                                Err(_) => registry.failed = true,
                            }
                        }
                    }
                }
            }
        }

        // Clean up after ourselves by shutting down spawned threads and resetting the terminal.
        let _ = canceller_thread.await;
        Self::reset_terminal();
        self.terminal.show_cursor()?;

        Ok(())
    }

    fn canceller_thread<M>() -> (JoinHandle<()>, oneshot::Sender<M>, CancellationToken)
    where
        M: Send + 'static,
    {
        let token = CancellationToken::new();
        let token_clone = token.clone();
        let (tx, rx) = oneshot::channel::<M>();

        let handle = tokio::spawn(async move {
            let _ = rx.await;
            token.cancel();
        });

        (handle, tx, token_clone)
    }

    pub fn render_ui(&mut self) -> io::Result<()> {
        match &self.stage {
            Stage::NotLoggedIn(registry) => {
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
                        .border_style(Style::new().bold().black())
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
                    let input_area_halves = Layout::vertical(Constraint::from_lengths([3, 3]))
                        .vertical_margin(1)
                        .horizontal_margin(2)
                        .flex(ratatui::layout::Flex::Start)
                        .split(input_area);

                    // Render the username field in the top part.
                    let focused = *registry.editing_mode() == EditingMode::Username;
                    let username_area =
                        Layout::horizontal([Constraint::Length(10), Constraint::Fill(1)])
                            .split(input_area_halves[0]);
                    let username =
                        format!("{}{}", registry.username, if focused { "_" } else { "" });
                    let mut username_label = Paragraph::new("\nUsername:");
                    let mut username_field = Paragraph::new(username)
                        .style(Style::default().bold())
                        .block(Block::bordered());
                    if focused {
                        username_label = username_label.style(Style::default().bold());
                        username_field = username_field
                            .block(Block::bordered().border_style(Style::default().magenta()));
                    }
                    frame.render_widget(username_label, username_area[0]);
                    frame.render_widget(username_field, username_area[1]);

                    // Render the password field in the bottom part.
                    let focused: bool = *registry.editing_mode() == EditingMode::Password;
                    let password_area =
                        Layout::horizontal([Constraint::Length(10), Constraint::Fill(1)])
                            .split(input_area_halves[1]);
                    let mut password_label = Paragraph::new("\nPassword:");
                    let obfuscated_password = format!(
                        "{}{}",
                        registry.password.chars().map(|_| '*').collect::<String>(),
                        if focused { "_" } else { "" }
                    );
                    let mut password_field = Paragraph::new(obfuscated_password.as_str())
                        .style(Style::default().bold())
                        .block(Block::bordered());
                    if focused {
                        password_label = password_label.style(Style::default().bold());
                        password_field = password_field
                            .block(Block::bordered().border_style(Style::default().magenta()));
                    }
                    frame.render_widget(password_label, password_area[0]);
                    frame.render_widget(password_field, password_area[1]);
                })?;
            }

            Stage::LoggedIn(chat) => {
                self.terminal.draw(|frame| {
                    frame.render_widget(
                        Paragraph::new(format!("{:?}", &chat.user))
                            .style(Style::default().bold())
                            .block(Block::bordered())
                            .wrap(Wrap { trim: false }),
                        frame.size(),
                    );
                })?;
            }
        }

        Ok(())
    }

    fn setup_terminal(&mut self) -> io::Result<()> {
        let eyre_hook = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            Self::reset_terminal();
            eyre_hook(info);
        }));

        terminal::enable_raw_mode()?;
        crossterm::execute!(io::stderr(), EnterAlternateScreen, EnableMouseCapture)?;
        self.terminal.hide_cursor()?;
        self.terminal.clear()?;

        Ok(())
    }

    fn reset_terminal() {
        let _ = terminal::disable_raw_mode();
        let _ = crossterm::execute!(io::stderr(), LeaveAlternateScreen, DisableMouseCapture);
    }
}
