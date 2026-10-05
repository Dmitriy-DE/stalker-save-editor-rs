//! Same-executable global-hotkey helper lifecycle and Companion command dispatch.

use crate::hotkeys::{HotkeyAction, HotkeyError, HotkeyLayout};
use crate::protocol::{CompanionClient, ReplyStatus};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Running focus-bound global companion hotkeys.
pub struct HotkeyRuntime {
    child: Child,
    input: Option<ChildStdin>,
    reader: Option<JoinHandle<()>>,
    exchange_directory: PathBuf,
    last_error: Arc<Mutex<Option<String>>>,
    stopped: bool,
}

impl std::fmt::Debug for HotkeyRuntime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HotkeyRuntime")
            .field("exchange_directory", &self.exchange_directory)
            .field("stopped", &self.stopped)
            .finish_non_exhaustive()
    }
}

impl HotkeyRuntime {
    /// Starts the same executable as a native hotkey helper and enables polling in the game mod.
    ///
    /// # Errors
    /// Returns an error if the mod rejects hotkeys, the helper cannot start, or native registration fails.
    pub fn start(layout: &HotkeyLayout, exchange_directory: PathBuf) -> Result<Self, HotkeyError> {
        let client = CompanionClient::new(exchange_directory.clone());
        let enabled = client
            .send("hotkeys", &["on"], Duration::from_secs(3))
            .map_err(|error| HotkeyError::new(error.to_string()))?;
        if enabled.status != ReplyStatus::Ok {
            return Err(HotkeyError::new(format!(
                "Enable companion hotkey polling failed: {} {}",
                status_name(enabled.status),
                enabled.text
            )));
        }

        let executable = std::env::current_exe().map_err(|error| HotkeyError::new(error.to_string()))?;
        let mut child = match Command::new(executable)
            .arg("--hotkey-helper")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                disable_polling(&exchange_directory);
                return Err(HotkeyError::new(error.to_string()));
            }
        };
        let mut input = match child.stdin.take() {
            Some(input) => input,
            None => {
                let _ = child.kill();
                disable_polling(&exchange_directory);
                return Err(HotkeyError::new("hotkey helper stdin was not piped"));
            }
        };
        let output = match child.stdout.take() {
            Some(output) => output,
            None => {
                let _ = child.kill();
                disable_polling(&exchange_directory);
                return Err(HotkeyError::new("hotkey helper stdout was not piped"));
            }
        };

        let mut by_id = Vec::new();
        for (index, (action, gesture)) in layout.bindings().enumerate() {
            let id =
                u32::try_from(index.saturating_add(1)).map_err(|_| HotkeyError::new("too many hotkey bindings"))?;
            let modifiers = u8::from(gesture.modifiers.control)
                | u8::from(gesture.modifiers.alt).wrapping_shl(1)
                | u8::from(gesture.modifiers.shift).wrapping_shl(2);
            writeln!(input, "bind {id} {modifiers} {}", gesture.key)
                .map_err(|error| HotkeyError::new(error.to_string()))?;
            by_id.push((id, action));
        }
        writeln!(input, "start").map_err(|error| HotkeyError::new(error.to_string()))?;
        input.flush().map_err(|error| HotkeyError::new(error.to_string()))?;

        let mut output = BufReader::new(output);
        let mut answer = String::new();
        output
            .read_line(&mut answer)
            .map_err(|error| HotkeyError::new(error.to_string()))?;
        let answer = answer.trim_end();
        if answer != "ready" {
            let reason = answer.strip_prefix("error ").unwrap_or(if answer.is_empty() {
                "The hotkey helper ended before it was ready."
            } else {
                answer
            });
            let _ = child.kill();
            let _ = child.wait();
            disable_polling(&exchange_directory);
            return Err(HotkeyError::new(reason));
        }

        let errors = Arc::new(Mutex::new(None));
        let reader_errors = Arc::clone(&errors);
        let reader_directory = exchange_directory.clone();
        let reader = std::thread::Builder::new()
            .name("companion-hotkeys".to_owned())
            .spawn(move || {
                let client = CompanionClient::new(reader_directory);
                let mut line = String::new();
                loop {
                    line.clear();
                    match output.read_line(&mut line) {
                        Ok(0) | Err(_) => return,
                        Ok(_) => {}
                    }
                    let Some(id) = line
                        .trim_end()
                        .strip_prefix("pressed ")
                        .and_then(|value| value.parse::<u32>().ok())
                    else {
                        continue;
                    };
                    let Some((_, action)) = by_id.iter().find(|(binding_id, _)| *binding_id == id) else {
                        continue;
                    };
                    let command = action_command(*action);
                    let result = client.send(command, &[], Duration::from_secs(3));
                    let message = match result {
                        Ok(reply) if reply.status == ReplyStatus::Ok => None,
                        Ok(reply) => Some(format!("Game returned {}: {}", status_name(reply.status), reply.text)),
                        Err(error) => Some(error.to_string()),
                    };
                    if let Ok(mut last_error) = reader_errors.lock() {
                        *last_error = message;
                    }
                }
            })
            .map_err(|error| {
                let _ = child.kill();
                disable_polling(&exchange_directory);
                HotkeyError::new(error.to_string())
            })?;

        Ok(Self {
            child,
            input: Some(input),
            reader: Some(reader),
            exchange_directory,
            last_error: errors,
            stopped: false,
        })
    }

    /// Exchange directory this runtime sends Companion commands through.
    #[must_use]
    pub fn exchange_directory(&self) -> &std::path::Path {
        &self.exchange_directory
    }

    /// Returns the most recent command failure, if any.
    #[must_use]
    pub fn last_error(&self) -> Option<String> {
        self.last_error.lock().ok().and_then(|error| error.clone())
    }

    /// Stops native grabs and tells the mod to stop polling hotkey commands.
    ///
    /// # Errors
    /// Returns the first cleanup failure after attempting every cleanup step.
    pub fn stop(&mut self) -> Result<(), HotkeyError> {
        if self.stopped {
            return Ok(());
        }
        self.stopped = true;
        let mut failure = None;
        if let Some(mut input) = self.input.take() {
            if writeln!(input, "stop").and_then(|()| input.flush()).is_err() {
                failure = Some("could not stop the hotkey helper cleanly".to_owned());
            }
        }
        let deadline = Instant::now()
            .checked_add(Duration::from_secs(3))
            .unwrap_or_else(Instant::now);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
                Ok(None) => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    break;
                }
                Err(error) => {
                    failure.get_or_insert_with(|| error.to_string());
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    break;
                }
            }
        }
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        let client = CompanionClient::new(self.exchange_directory.clone());
        match client.send("hotkeys", &["off"], Duration::from_secs(3)) {
            Ok(reply) if reply.status == ReplyStatus::Ok => {}
            Ok(reply) => {
                failure.get_or_insert_with(|| {
                    format!(
                        "Disable companion hotkey polling failed: {} {}",
                        status_name(reply.status),
                        reply.text
                    )
                });
            }
            Err(error) => {
                failure.get_or_insert_with(|| error.to_string());
            }
        }
        failure.map_or(Ok(()), |message| Err(HotkeyError::new(message)))
    }
}

impl Drop for HotkeyRuntime {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn disable_polling(directory: &std::path::Path) {
    let client = CompanionClient::new(directory.to_path_buf());
    let _ = client.send("hotkeys", &["off"], Duration::from_secs(3));
}

const fn action_command(action: HotkeyAction) -> &'static str {
    match action {
        HotkeyAction::Heal => "heal",
        HotkeyAction::RepairEquipped => "repair_equipped",
        HotkeyAction::Mark => "mark",
        HotkeyAction::JumpLast => "jump_last",
        HotkeyAction::QuickSave => "quicksave",
    }
}

const fn status_name(status: ReplyStatus) -> &'static str {
    match status {
        ReplyStatus::Ok => "ok",
        ReplyStatus::Error => "error",
        ReplyStatus::Unsupported => "unsupported",
    }
}
