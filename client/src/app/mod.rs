pub mod chat;
pub mod interceptor;
pub mod registry;
mod ui;

pub use chat::Chat;
pub use interceptor::Interceptor;
pub use registry::Registry;

use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::Terminal;
use std::mem;
use std::{io, panic, time::Duration};
use tcp_chat_server::proto::{self, ClientsideMessage};
use tokio::{sync::oneshot, task::JoinHandle};
use tokio_util::sync::CancellationToken;

#[derive(Debug)]
pub enum Stage {
    NotLoggedIn { registry: Registry },
    LoggedIn { chat: Chat<Interceptor> },
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
            stage: Stage::NotLoggedIn { registry },
            terminal,
        }
    }
}

impl<B> App<B>
where
    B: Backend + Sync + Send,
{
    #[allow(clippy::significant_drop_in_scrutinee)]
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
            self.render_ui().await?;

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

                    match self.stage {
                        Stage::NotLoggedIn { ref mut registry } => match event.code {
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

                            KeyCode::Enter => self.attempt_login().await,

                            KeyCode::Tab | KeyCode::BackTab => registry.toggle_mode(),

                            _ => {}
                        },

                        Stage::LoggedIn { ref mut chat } => match event.code {
                            KeyCode::Enter => {
                                if !chat.message_draft.is_empty() {
                                    let rooms = chat.rooms.lock().await;
                                    let room_uuid = match chat.room_list_state.selected() {
                                        Some(i) => rooms.keys().nth(i).unwrap(),
                                        None => todo!(),
                                    };

                                    chat.client
                                        .lock()
                                        .await
                                        .send_message(ClientsideMessage {
                                            room_uuid: Some(proto::Uuid::from(*room_uuid)),
                                            text: mem::take(&mut chat.message_draft),
                                        })
                                        .await
                                        .unwrap();

                                    drop(rooms);
                                }
                            }

                            KeyCode::Char(c) => chat.message_draft.push(c),
                            KeyCode::Backspace => {
                                chat.message_draft.pop();
                            }

                            _ => {}
                        },
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

    #[allow(clippy::significant_drop_in_scrutinee)]
    async fn attempt_login(&mut self) {
        if let Stage::NotLoggedIn { registry } = &mut self.stage {
            match registry.clone().into_chat().await {
                Err(_) => registry.failed = true,
                Ok(mut chat) => {
                    chat.load_data().await.unwrap();
                    self.stage = Stage::LoggedIn { chat }
                }
            }
        }
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
