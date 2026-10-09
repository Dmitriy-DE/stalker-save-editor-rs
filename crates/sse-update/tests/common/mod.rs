#![allow(dead_code, missing_docs)]

use sse_core::{Error, Result};
use sse_update::{ContentRange, Fetch, ProcessRunner, Response};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default)]
pub struct MemoryFetch {
    routes: BTreeMap<String, Vec<u8>>,
}

impl MemoryFetch {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, url: impl Into<String>, payload: Vec<u8>) {
        self.routes.insert(url.into(), payload);
    }
}

impl Fetch for MemoryFetch {
    fn get(&mut self, url: &str, range_from: u64, sink: &mut dyn FnMut(&[u8]) -> bool) -> Result<Response> {
        self.get_with_response(url, range_from, &mut |_| true, sink)
    }

    fn get_with_response(
        &mut self,
        url: &str,
        range_from: u64,
        on_response: &mut dyn FnMut(&Response) -> bool,
        sink: &mut dyn FnMut(&[u8]) -> bool,
    ) -> Result<Response> {
        let body = self
            .routes
            .get(url)
            .ok_or_else(|| Error::Refused(format!("404 Not Found: {url}")))?;
        let body_length = u64::try_from(body.len()).map_err(|_| Error::Refused("response too large".to_owned()))?;
        let start = usize::try_from(range_from).unwrap_or(body.len());
        let status = if range_from == 0 {
            200
        } else if start < body.len() {
            206
        } else {
            416
        };
        let slice = if status == 416 {
            &[][..]
        } else {
            body.get(start..).unwrap_or(&[])
        };
        let response = Response {
            status,
            content_length: Some(
                u64::try_from(slice.len()).map_err(|_| Error::Refused("response too large".to_owned()))?,
            ),
            content_range: (status == 206).then_some(ContentRange {
                start: range_from,
                end: body_length.saturating_sub(1),
                total: body_length,
            }),
            final_url: url.to_owned(),
        };
        if !on_response(&response) {
            return Err(Error::Refused("response rejected by caller".to_owned()));
        }
        for chunk in slice.chunks(64 * 1024) {
            if !sink(chunk) {
                return Err(Error::Refused("fetch cancelled by sink".to_owned()));
            }
        }
        Ok(response)
    }
}

#[derive(Clone, Debug, Default)]
pub struct MockProcessRunner {
    pub exit_code: i32,
    pub last_program: Option<String>,
    pub last_args: Vec<String>,
    pub call_count: usize,
    pub available_commands: Option<Vec<String>>,
}

impl MockProcessRunner {
    pub fn new(exit_code: i32) -> Self {
        Self {
            exit_code,
            ..Self::default()
        }
    }
}

impl ProcessRunner for MockProcessRunner {
    fn run(&mut self, program: &str, args: &[&str]) -> Result<i32> {
        self.call_count = self.call_count.saturating_add(1);
        self.last_program = Some(program.to_owned());
        self.last_args = args.iter().map(|argument| (*argument).to_owned()).collect();
        Ok(self.exit_code)
    }

    fn has_command(&self, program: &str) -> bool {
        self.available_commands
            .as_ref()
            .is_none_or(|commands| commands.iter().any(|command| command == program))
    }
}
