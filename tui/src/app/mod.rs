pub mod chat;
pub mod interceptor;
pub mod registry;

pub use chat::Chat;
pub use interceptor::Interceptor;
pub use registry::Registry;

use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::widgets::{Block, Paragraph, Wrap};
use ratatui::{text::Line, Terminal};
use std::{io, panic, time::Duration};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

#[derive(Debug)]
pub struct App<B: Backend> {
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

impl<B: Backend + Sync> App<B> {
    pub async fn run(&mut self) -> io::Result<()> {
        self.setup_terminal()?;

        let (canceller_thread, canceller_tx, _) = Self::canceller_thread::<()>();

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

                    if event.code == KeyCode::Esc {
                        let _ = canceller_tx.send(());
                        break;
                    }

                    let mut attempt_login = false;
                    match &mut self.stage {
                        Stage::NotLoggedIn(registry) => match event.code {
                            KeyCode::Char(c) => match registry.editing_mode() {
                                registry::EditingMode::Username => registry.username.push(c),
                                registry::EditingMode::Password => registry.password.push(c),
                            },

                            KeyCode::Backspace => {
                                match registry.editing_mode() {
                                    registry::EditingMode::Username => registry.username.pop(),
                                    registry::EditingMode::Password => registry.password.pop(),
                                };
                            }

                            KeyCode::Enter => match registry.editing_mode() {
                                registry::EditingMode::Username => registry.toggle_mode(),
                                registry::EditingMode::Password => attempt_login = true,
                            },

                            KeyCode::Tab => registry.toggle_mode(),

                            _ => {}
                        },

                        Stage::LoggedIn(_) => {}
                    }

                    if attempt_login {
                        if let Stage::NotLoggedIn(registry) = &self.stage {
                            match registry.clone().into_chat().await {
                                Ok(chat) => self.stage = Stage::LoggedIn(chat),
                                Err(_) => todo!("Handle incorrect credentials"),
                            }
                        }
                    }
                }
            }
        }

        // Shut everything down.
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
                        .constraints(Constraint::from_fills([1, 1, 1]))
                        .split(frame.size());
                    let horizontal_areas = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints(Constraint::from_fills([1, 4, 1]))
                        .split(*vertical_areas.get(1).unwrap());

                    let central_area = horizontal_areas.get(1).unwrap();
                    let block = Block::bordered()
                        .title_top(" Welcome! Log into your account... ")
                        .title_bottom(Line::from(" Tab to switch line ").left_aligned())
                        .title_bottom(Line::from(" Enter to log in ").right_aligned());

                    let text = format!(
                        "{} Username: {:?}\n{} Password: {:?}",
                        if *registry.editing_mode() == registry::EditingMode::Username {
                            '>'
                        } else {
                            ' '
                        },
                        registry.username.as_str(),
                        if *registry.editing_mode() == registry::EditingMode::Password {
                            '>'
                        } else {
                            ' '
                        },
                        registry.password.as_str()
                    );

                    let widget = Paragraph::new(text).block(block).wrap(Wrap { trim: false });
                    frame.render_widget(widget, *central_area);
                })?;
            }

            Stage::LoggedIn(chat) => {
                self.terminal.draw(|frame| {
                    let text = format!("{:?}", &chat.user);
                    frame.render_widget(
                        Paragraph::new(text)
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

#[derive(Debug)]
pub enum Stage {
    NotLoggedIn(Registry),
    LoggedIn(Chat<Interceptor>),
}
