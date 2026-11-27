use crate::audio::{Config, Engine, Graph, Processor};
use crate::process_capture::{CaptureState, SharedCaptureState};
use std::{
    sync::{mpsc, Arc, Mutex},
    thread::{self, JoinHandle},
    time::Duration,
};

pub struct Snapshot {
    pub processor: Arc<Processor>,
    pub description: String,
    status: Arc<Mutex<String>>,
    app_captures: Vec<SharedCaptureState>,
}
impl Snapshot {
    pub fn system_status(&self) -> String {
        self.status
            .try_lock()
            .map(|s| s.clone())
            .unwrap_or_else(|_| "设备状态正在更新".into())
    }
    pub fn app_capture_state(&self, slot: usize) -> Option<CaptureState> {
        self.app_captures
            .get(slot)?
            .try_lock()
            .ok()
            .map(|state| state.clone())
    }
}
pub enum Event {
    Started {
        revision: u64,
        graph: Graph,
        snapshot: Snapshot,
    },
    Failed {
        revision: u64,
        error: String,
    },
    Stopped {
        revision: u64,
    },
}
enum Command {
    Restart {
        revision: u64,
        config: Config,
        graph: Graph,
    },
    Stop {
        revision: u64,
    },
    Shutdown,
}
pub struct Controller {
    commands: mpsc::Sender<Command>,
    events: mpsc::Receiver<Event>,
    worker: Option<JoinHandle<()>>,
}
impl Controller {
    pub fn new() -> Self {
        let (commands, receive) = mpsc::channel();
        let (send, events) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("soon-link-audio-control".into())
            .spawn(move || {
                // ASIO COM objects are created, used and released on this same owner thread.
                let mut engine: Option<Engine> = None;
                let mut status: Option<Arc<Mutex<String>>> = None;
                loop {
                    match receive.recv_timeout(Duration::from_millis(250)) {
                        Ok(mut command) => {
                            // Rapid layout edits are coalesced before opening devices again.
                            while let Ok(next) = receive.try_recv() {
                                if matches!(command, Command::Shutdown) {
                                    break;
                                }
                                command = next;
                            }
                            engine = None;
                            status = None;
                            match command {
                                Command::Shutdown => break,
                                Command::Stop { revision } => {
                                    let _ = send.send(Event::Stopped { revision });
                                }
                                Command::Restart {
                                    revision,
                                    config,
                                    mut graph,
                                } => match Engine::start(&config, &mut graph) {
                                    Ok(active) => {
                                        let state = Arc::new(Mutex::new(
                                            active
                                                .system_transport
                                                .as_ref()
                                                .map(|transport| transport.status())
                                                .unwrap_or_else(crate::usb_audio::driver_status),
                                        ));
                                        let snapshot = Snapshot {
                                            processor: active.processor.clone(),
                                            description: active.description.clone(),
                                            status: state.clone(),
                                            app_captures: active.app_capture_states(),
                                        };
                                        engine = Some(active);
                                        status = Some(state);
                                        if send
                                            .send(Event::Started {
                                                revision,
                                                graph,
                                                snapshot,
                                            })
                                            .is_err()
                                        {
                                            break;
                                        }
                                    }
                                    Err(error) => {
                                        if send.send(Event::Failed { revision, error }).is_err() {
                                            break;
                                        }
                                    }
                                },
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            if let (Some(active), Some(state)) = (&engine, &status) {
                                if let Some(transport) = &active.system_transport {
                                    if let Ok(mut current) = state.lock() {
                                        *current = transport.status();
                                    }
                                }
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
            })
            .ok();
        Self {
            commands,
            events,
            worker,
        }
    }
    pub fn restart(&self, revision: u64, config: Config, graph: Graph) -> Result<(), String> {
        self.commands
            .send(Command::Restart {
                revision,
                config,
                graph,
            })
            .map_err(|_| "无法创建音频控制线程".into())
    }
    pub fn stop(&self, revision: u64) {
        let _ = self.commands.send(Command::Stop { revision });
    }
    pub fn poll(&self) -> Option<Event> {
        self.events.try_recv().ok()
    }
}
impl Drop for Controller {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
