//! Byte-preserving patching for verified Lua source lines.

/// Adds `hook` immediately after the one exact `anchor` line.
///
/// An already-installed adjacent hook is idempotent. A repeated anchor, or a hook at any other
/// location, is rejected so a changed game script is never patched by a guessed position.
pub fn patch_exact_line(source: &[u8], anchor: &[u8], hook: &[u8]) -> Result<Vec<u8>, HookError> {
    validate_tokens(anchor, hook)?;
    let lines = line_offsets(source);
    let matches = lines
        .iter()
        .filter(|(start, content_end, _)| source.get(*start..*content_end) == Some(anchor))
        .count();
    if matches != 1 {
        return Err(HookError::new("anchor must occur on exactly one complete source line"));
    }
    let anchor_line = lines
        .iter()
        .position(|(start, content_end, _)| source.get(*start..*content_end) == Some(anchor))
        .ok_or_else(|| HookError::new("anchor disappeared while patching"))?;
    insert_after_selected_line(source, anchor_line, hook)
}

fn insert_after_selected_line(source: &[u8], anchor_line: usize, hook: &[u8]) -> Result<Vec<u8>, HookError> {
    let lines = line_offsets(source);
    let hook_count = source.windows(hook.len()).filter(|window| *window == hook).count();
    if hook_count != 0 {
        if anchor_line
            .checked_add(1)
            .and_then(|next| lines.get(next))
            .is_some_and(|(start, content_end, _)| source.get(*start..*content_end) == Some(hook))
            && hook_count == 1
        {
            return Ok(source.to_vec());
        }
        return Err(HookError::new("hook already exists outside the unique anchor location"));
    }
    let (_, content_end, line_end) = lines
        .get(anchor_line)
        .copied()
        .ok_or_else(|| HookError::new("anchor line is missing"))?;
    let newline = if source
        .get(content_end..line_end)
        .is_some_and(|ending| ending == b"\r\n")
    {
        b"\r\n".as_slice()
    } else if source.get(content_end..line_end).is_some_and(|ending| ending == b"\n") {
        b"\n".as_slice()
    } else if source.get(content_end..line_end).is_some_and(|ending| ending == b"\r") {
        b"\r".as_slice()
    } else {
        b"\n".as_slice()
    };
    let insert_at = if line_end > content_end {
        content_end
    } else {
        source.len()
    };
    let mut patched = Vec::with_capacity(source.len().saturating_add(newline.len()).saturating_add(hook.len()));
    patched.extend_from_slice(
        source
            .get(..insert_at)
            .ok_or_else(|| HookError::new("source prefix range is invalid"))?,
    );
    patched.extend_from_slice(newline);
    patched.extend_from_slice(hook);
    patched.extend_from_slice(
        source
            .get(insert_at..)
            .ok_or_else(|| HookError::new("source suffix range is invalid"))?,
    );
    Ok(patched)
}

/// Removes the one adjacent hook inserted by [`patch_exact_line`].
pub fn remove_exact_line(source: &[u8], anchor: &[u8], hook: &[u8]) -> Result<Vec<u8>, HookError> {
    validate_tokens(anchor, hook)?;
    let lines = line_offsets(source);
    let anchor_indices = lines
        .iter()
        .enumerate()
        .filter_map(|(index, (start, content_end, _))| {
            (source.get(*start..*content_end) == Some(anchor)).then_some(index)
        })
        .collect::<Vec<_>>();
    if anchor_indices.len() != 1 {
        return Err(HookError::new("anchor must occur on exactly one complete source line"));
    }
    let anchor_index = *anchor_indices
        .first()
        .ok_or_else(|| HookError::new("anchor line is missing"))?;
    let hook_indices = lines
        .iter()
        .enumerate()
        .filter_map(|(index, (start, content_end, _))| {
            (source.get(*start..*content_end) == Some(hook)).then_some(index)
        })
        .collect::<Vec<_>>();
    if hook_indices.as_slice() != [anchor_index.saturating_add(1)] {
        return Err(HookError::new("installed hook is missing or ambiguous"));
    }
    let (_, anchor_content_end, _) = lines
        .get(anchor_index)
        .copied()
        .ok_or_else(|| HookError::new("anchor range is missing"))?;
    let (_, hook_content_end, _) = lines
        .get(anchor_index.saturating_add(1))
        .copied()
        .ok_or_else(|| HookError::new("hook range is missing"))?;
    let remove_start = anchor_content_end;
    let remove_end = hook_content_end;
    let mut restored = Vec::with_capacity(source.len());
    restored.extend_from_slice(
        source
            .get(..remove_start)
            .ok_or_else(|| HookError::new("source prefix range is invalid"))?,
    );
    restored.extend_from_slice(
        source
            .get(remove_end..)
            .ok_or_else(|| HookError::new("source suffix range is invalid"))?,
    );
    Ok(restored)
}

/// Patches the two exact bind-stalker anchors used by the shipped companion scripts.
///
/// The function checks the complete source line and surrounding indentation before inserting either hook.
pub fn patch_bind_stalker(source: &[u8], game: crate::bundled::Game) -> Result<Vec<u8>, HookError> {
    let updated = patch_after_statement(
        source,
        b"object_binder.update(self, delta)",
        b"if save_editor_companion then save_editor_companion.update() end",
        false,
    )?;
    match game {
        crate::bundled::Game::CallOfPripyat => patch_after_statement(
            &updated,
            b"function actor_binder:use_inventory_item(obj)",
            b"if save_editor_companion then save_editor_companion.on_use(obj) end",
            true,
        ),
        crate::bundled::Game::ShadowOfChernobyl | crate::bundled::Game::ClearSky => patch_after_statement(
            &updated,
            b"self.object:set_callback(callback.on_item_drop, self.on_item_drop, self)",
            b"if save_editor_companion then self.object:set_callback(callback.use_object, function(_, obj) save_editor_companion.on_use(obj) end) end",
            false,
        ),
    }
}

/// Removes exactly the two installed bind-stalker hooks and preserves original bytes.
pub fn remove_bind_stalker(source: &[u8], game: crate::bundled::Game) -> Result<Vec<u8>, HookError> {
    let (anchor, hook) = match game {
        crate::bundled::Game::CallOfPripyat => (
            b"function actor_binder:use_inventory_item(obj)".as_slice(),
            b"if save_editor_companion then save_editor_companion.on_use(obj) end".as_slice(),
        ),
        crate::bundled::Game::ShadowOfChernobyl | crate::bundled::Game::ClearSky => (
            b"self.object:set_callback(callback.on_item_drop, self.on_item_drop, self)".as_slice(),
            b"if save_editor_companion then self.object:set_callback(callback.use_object, function(_, obj) save_editor_companion.on_use(obj) end) end".as_slice(),
        ),
    };
    let without_use = remove_after_statement(source, anchor, hook, game == crate::bundled::Game::CallOfPripyat)?;
    remove_after_statement(
        &without_use,
        b"object_binder.update(self, delta)",
        b"if save_editor_companion then save_editor_companion.update() end",
        false,
    )
}

/// Patches the unique keyboard handler in `ui_main_menu.script`, matching the C# hook placement rule.
pub fn patch_main_menu(source: &[u8]) -> Result<Vec<u8>, HookError> {
    let tokens = lua_tokens(source);
    let function = unique_substring(source, b"function main_menu:OnKeyboard")?;
    let function_token = tokens
        .iter()
        .position(|token| token.start == function && token_is(source, *token, b"function"))
        .ok_or_else(|| HookError::new("main menu function anchor is not a Lua function"))?;
    let function_end = matching_block_end(source, &tokens, function_token)?;
    let function_end_position = tokens
        .get(function_end)
        .ok_or_else(|| HookError::new("main menu function end token is missing"))?
        .start;
    let window_starts = tokens
        .iter()
        .enumerate()
        .filter_map(|(index, token)| {
            (token.start > function && token.start < function_end_position && is_window_key_if(source, &tokens, index))
                .then_some(index)
        })
        .collect::<Vec<_>>();
    if window_starts.len() != 1 {
        return Err(HookError::new("window-key handler anchor must occur exactly once"));
    }
    let window_start = *window_starts
        .first()
        .ok_or_else(|| HookError::new("window-key handler anchor is missing"))?;
    let window_start_position = tokens
        .get(window_start)
        .ok_or_else(|| HookError::new("window-key token is missing"))?
        .start;
    let window_then = window_key_then_index(source, &tokens, window_start)
        .ok_or_else(|| HookError::new("window-key handler statement is malformed"))?;
    let window_end = matching_block_end(source, &tokens, window_start)?;
    let window_end_position = tokens
        .get(window_end)
        .ok_or_else(|| HookError::new("window-key block end token is missing"))?
        .start;
    let quit_starts = tokens
        .iter()
        .enumerate()
        .filter_map(|(index, token)| {
            (token.start > window_start_position
                && token.start < window_end_position
                && is_quit_key_if(source, &tokens, index))
            .then_some(index)
        })
        .collect::<Vec<_>>();
    if quit_starts.is_empty() {
        let span = source
            .get(
                tokens
                    .get(window_start)
                    .ok_or_else(|| HookError::new("window-key token is missing"))?
                    .start
                    ..tokens
                        .get(window_then)
                        .ok_or_else(|| HookError::new("window-key then token is missing"))?
                        .end,
            )
            .ok_or_else(|| HookError::new("window-key statement range is invalid"))?;
        patch_after_statement(
            source,
            span,
            b"if save_editor_companion_ui then save_editor_companion_ui.on_menu_key(dik, self) end",
            true,
        )
    } else {
        if quit_starts.len() != 1 {
            return Err(HookError::new("quit-key branch anchor must occur exactly once"));
        }
        let quit_start = *quit_starts
            .first()
            .ok_or_else(|| HookError::new("quit-key branch is missing"))?;
        let quit_end = matching_block_end(source, &tokens, quit_start)?;
        if tokens
            .get(quit_end)
            .ok_or_else(|| HookError::new("quit-key end token is missing"))?
            .start
            >= window_end_position
        {
            return Err(HookError::new("quit-key branch is outside the window-key block"));
        }
        insert_after_block_end(
            source,
            tokens
                .get(quit_end)
                .ok_or_else(|| HookError::new("quit-key end token is missing"))?
                .start,
            b"if save_editor_companion_ui then save_editor_companion_ui.on_menu_key(dik, self) end",
        )
    }
}

/// Removes the unique menu hook from the same verified block selected by [`patch_main_menu`].
pub fn remove_main_menu(source: &[u8]) -> Result<Vec<u8>, HookError> {
    let tokens = lua_tokens(source);
    let function = unique_substring(source, b"function main_menu:OnKeyboard")?;
    let function_token = tokens
        .iter()
        .position(|token| token.start == function && token_is(source, *token, b"function"))
        .ok_or_else(|| HookError::new("main menu function anchor is not a Lua function"))?;
    let function_end = matching_block_end(source, &tokens, function_token)?;
    let function_end_position = tokens
        .get(function_end)
        .ok_or_else(|| HookError::new("main menu function end token is missing"))?
        .start;
    let window_starts = tokens
        .iter()
        .enumerate()
        .filter_map(|(index, token)| {
            (token.start > function && token.start < function_end_position && is_window_key_if(source, &tokens, index))
                .then_some(index)
        })
        .collect::<Vec<_>>();
    if window_starts.len() != 1 {
        return Err(HookError::new("window-key handler anchor must occur exactly once"));
    }
    let window_start = *window_starts
        .first()
        .ok_or_else(|| HookError::new("window-key handler anchor is missing"))?;
    let window_start_position = tokens
        .get(window_start)
        .ok_or_else(|| HookError::new("window-key token is missing"))?
        .start;
    let window_then = window_key_then_index(source, &tokens, window_start)
        .ok_or_else(|| HookError::new("window-key handler statement is malformed"))?;
    let window_end = matching_block_end(source, &tokens, window_start)?;
    let window_end_position = tokens
        .get(window_end)
        .ok_or_else(|| HookError::new("window-key block end token is missing"))?
        .start;
    let quit_starts = tokens
        .iter()
        .enumerate()
        .filter_map(|(index, token)| {
            (token.start > window_start_position
                && token.start < window_end_position
                && is_quit_key_if(source, &tokens, index))
            .then_some(index)
        })
        .collect::<Vec<_>>();
    if quit_starts.is_empty() {
        let span = source
            .get(
                tokens
                    .get(window_start)
                    .ok_or_else(|| HookError::new("window-key token is missing"))?
                    .start
                    ..tokens
                        .get(window_then)
                        .ok_or_else(|| HookError::new("window-key then token is missing"))?
                        .end,
            )
            .ok_or_else(|| HookError::new("window-key statement range is invalid"))?;
        remove_after_statement(
            source,
            span,
            b"if save_editor_companion_ui then save_editor_companion_ui.on_menu_key(dik, self) end",
            true,
        )
    } else {
        if quit_starts.len() != 1 {
            return Err(HookError::new("quit-key branch anchor must occur exactly once"));
        }
        let quit_start = *quit_starts
            .first()
            .ok_or_else(|| HookError::new("quit-key branch is missing"))?;
        let quit_end = matching_block_end(source, &tokens, quit_start)?;
        remove_after_block_end(
            source,
            tokens
                .get(quit_end)
                .ok_or_else(|| HookError::new("quit-key end token is missing"))?
                .start,
            b"if save_editor_companion_ui then save_editor_companion_ui.on_menu_key(dik, self) end",
        )
    }
}

/// Adds or removes the companion include at the very end of a quest-items LTX file.
pub fn patch_quest_include(source: &[u8], insert: bool) -> Result<Vec<u8>, HookError> {
    const INCLUDE: &[u8] = b"#include \"save_editor_companion.ltx\"";
    let lines = line_offsets(source);
    let occurrences = source
        .windows(INCLUDE.len())
        .filter(|window| *window == INCLUDE)
        .count();
    if insert {
        if occurrences != 0 {
            let final_line = lines.iter().rev().find(|(start, end, _)| {
                source
                    .get(*start..*end)
                    .is_some_and(|line| !line.iter().all(u8::is_ascii_whitespace))
            });
            if occurrences == 1 && final_line.is_some_and(|(start, end, _)| source.get(*start..*end) == Some(INCLUDE)) {
                return Ok(source.to_vec());
            }
            return Err(HookError::new(
                "companion include already exists outside the end of file",
            ));
        }
        let newline: &[u8] = if source.windows(2).any(|window| window == b"\r\n") {
            b"\r\n"
        } else if source.contains(&b'\r') && !source.contains(&b'\n') {
            b"\r"
        } else {
            b"\n"
        };
        let capacity = source
            .len()
            .saturating_add(INCLUDE.len())
            .saturating_add(newline.len().saturating_mul(2));
        let mut output = Vec::with_capacity(capacity);
        output.extend_from_slice(source);
        if !source.is_empty() && !source.ends_with(b"\n") && !source.ends_with(b"\r") {
            output.extend_from_slice(newline);
        }
        output.extend_from_slice(INCLUDE);
        output.extend_from_slice(newline);
        Ok(output)
    } else {
        if occurrences != 1 {
            return Err(HookError::new("installed companion include is missing or ambiguous"));
        }
        let Some((start, _, line_end)) = lines
            .iter()
            .rev()
            .find(|(start, end, _)| source.get(*start..*end) == Some(INCLUDE))
            .copied()
        else {
            return Err(HookError::new(
                "installed companion include is not the final non-empty line",
            ));
        };
        if lines
            .iter()
            .skip_while(|(line_start, _, _)| *line_start != start)
            .skip(1)
            .any(|(next_start, next_end, _)| {
                source
                    .get(*next_start..*next_end)
                    .is_some_and(|line| !line.iter().all(u8::is_ascii_whitespace))
            })
        {
            return Err(HookError::new(
                "installed companion include is not the final non-empty line",
            ));
        }
        let mut output = Vec::with_capacity(source.len());
        output.extend_from_slice(
            source
                .get(..start)
                .ok_or_else(|| HookError::new("include prefix range is invalid"))?,
        );
        output.extend_from_slice(
            source
                .get(line_end..)
                .ok_or_else(|| HookError::new("include suffix range is invalid"))?,
        );
        Ok(output)
    }
}

/// Removes the unique final include inserted by [`patch_quest_include`].
pub fn remove_quest_include(source: &[u8]) -> Result<Vec<u8>, HookError> {
    patch_quest_include(source, false)
}

fn patch_after_statement(
    source: &[u8],
    statement: &[u8],
    hook: &[u8],
    indent_from_following_line: bool,
) -> Result<Vec<u8>, HookError> {
    validate_tokens(statement, hook)?;
    let lines = line_offsets(source);
    let (line_index, relative) = unique_lua_statement_line(source, statement, &lines)?;
    let (line_start, content_end, line_end) = *lines
        .get(line_index)
        .ok_or_else(|| HookError::new("source anchor line is missing"))?;
    let content = source
        .get(line_start..content_end)
        .ok_or_else(|| HookError::new("source anchor range is invalid"))?;
    let prefix = content
        .get(..relative)
        .ok_or_else(|| HookError::new("source anchor prefix range is invalid"))?;
    let suffix_start = relative.saturating_add(statement.len());
    let suffix = content
        .get(suffix_start..)
        .ok_or_else(|| HookError::new("source anchor suffix range is invalid"))?;
    if !prefix.iter().all(|byte| matches!(byte, b' ' | b'\t')) || !suffix.iter().all(u8::is_ascii_whitespace) {
        return Err(HookError::new("source anchor is not a standalone statement line"));
    }
    let indentation = if indent_from_following_line {
        following_indentation(source, line_end)?
    } else {
        prefix.to_vec()
    };
    let mut hook_line = indentation;
    hook_line.extend_from_slice(hook);
    insert_after_selected_line(source, line_index, &hook_line)
}

fn remove_after_statement(
    source: &[u8],
    statement: &[u8],
    hook: &[u8],
    indent_from_following_line: bool,
) -> Result<Vec<u8>, HookError> {
    validate_tokens(statement, hook)?;
    let lines = line_offsets(source);
    let (anchor_index, relative) = unique_lua_statement_line(source, statement, &lines)?;
    let anchor = lines
        .get(anchor_index)
        .ok_or_else(|| HookError::new("source anchor line is missing"))?;
    let line = source
        .get(anchor.0..anchor.1)
        .ok_or_else(|| HookError::new("source anchor line range is invalid"))?;
    let indentation = if indent_from_following_line {
        following_indentation(source, anchor.2)?
    } else {
        line.get(..relative)
            .ok_or_else(|| HookError::new("source anchor prefix range is invalid"))?
            .to_vec()
    };
    let mut hook_line = indentation;
    hook_line.extend_from_slice(hook);
    remove_after_selected_line(source, anchor_index, &hook_line)
}

fn unique_lua_statement_line(
    source: &[u8],
    statement: &[u8],
    lines: &[(usize, usize, usize)],
) -> Result<(usize, usize), HookError> {
    let tokens = lua_tokens(source);
    let mut matched = None;
    let mut match_count = 0_usize;
    for (line_index, (line_start, content_end, _)) in lines.iter().copied().enumerate() {
        let Some(line) = source.get(line_start..content_end) else {
            continue;
        };
        for (relative, window) in line.windows(statement.len()).enumerate() {
            if window != statement {
                continue;
            }
            let Some(prefix) = line.get(..relative) else { continue };
            let suffix_start = relative.saturating_add(statement.len());
            let Some(suffix) = line.get(suffix_start..) else {
                continue;
            };
            let absolute = line_start.saturating_add(relative);
            if !prefix.iter().all(u8::is_ascii_whitespace)
                || !suffix.iter().all(u8::is_ascii_whitespace)
                || tokens.binary_search_by_key(&absolute, |token| token.start).is_err()
            {
                continue;
            }
            match_count = match_count.saturating_add(1);
            matched = Some((line_index, relative));
        }
    }
    if match_count != 1 {
        return Err(HookError::new(
            "source anchor must occur exactly once as a Lua statement",
        ));
    }
    matched.ok_or_else(|| HookError::new("source anchor line is missing"))
}

fn remove_after_selected_line(source: &[u8], anchor_index: usize, hook: &[u8]) -> Result<Vec<u8>, HookError> {
    let lines = line_offsets(source);
    let anchor = lines
        .get(anchor_index)
        .ok_or_else(|| HookError::new("source anchor line is missing"))?;
    let (hook_start, hook_content_end, _) = lines
        .get(anchor_index.saturating_add(1))
        .copied()
        .ok_or_else(|| HookError::new("installed hook line is missing"))?;
    if source.get(hook_start..hook_content_end) != Some(hook) {
        return Err(HookError::new("installed hook is not adjacent to its source anchor"));
    }
    if source.windows(hook.len()).filter(|window| *window == hook).count() != 1 {
        return Err(HookError::new("installed hook is missing or ambiguous"));
    }
    let remove_start = anchor.1;
    let mut restored = Vec::with_capacity(source.len());
    restored.extend_from_slice(
        source
            .get(..remove_start)
            .ok_or_else(|| HookError::new("source prefix range is invalid"))?,
    );
    restored.extend_from_slice(
        source
            .get(hook_content_end..)
            .ok_or_else(|| HookError::new("source suffix range is invalid"))?,
    );
    Ok(restored)
}

fn following_indentation(source: &[u8], line_end: usize) -> Result<Vec<u8>, HookError> {
    let lines = line_offsets(source);
    for (start, end, _) in lines.iter().filter(|(start, _, _)| *start >= line_end) {
        let Some(line) = source.get(*start..*end) else { continue };
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        return Ok(line
            .iter()
            .take_while(|byte| matches!(byte, b' ' | b'\t'))
            .copied()
            .collect());
    }
    Err(HookError::new("following source indentation could not be determined"))
}

#[derive(Debug, Clone, Copy)]
struct LuaToken {
    start: usize,
    end: usize,
}

fn lua_tokens(source: &[u8]) -> Vec<LuaToken> {
    let mut tokens = Vec::new();
    let mut index = 0_usize;
    while index < source.len() {
        if source.get(index..index.saturating_add(2)) == Some(b"--") {
            index = skip_lua_comment(source, index.saturating_add(2));
            continue;
        }
        let Some(byte) = source.get(index).copied() else {
            break;
        };
        if byte == b'[' {
            if let Some(end) = skip_lua_long_bracket(source, index) {
                index = end;
                continue;
            }
        }
        if matches!(byte, b'\'' | b'"') {
            index = skip_lua_string(source, index);
            continue;
        }
        if byte.is_ascii_alphabetic() || byte == b'_' {
            let start = index;
            index = index.saturating_add(1);
            while source
                .get(index)
                .is_some_and(|value| value.is_ascii_alphanumeric() || *value == b'_')
            {
                index = index.saturating_add(1);
            }
            tokens.push(LuaToken { start, end: index });
            continue;
        }
        if byte == b'=' && source.get(index.saturating_add(1)) == Some(&b'=') {
            tokens.push(LuaToken {
                start: index,
                end: index.saturating_add(2),
            });
            index = index.saturating_add(2);
            continue;
        }
        if matches!(byte, b'.' | b'=' | b':') {
            tokens.push(LuaToken {
                start: index,
                end: index.saturating_add(1),
            });
        }
        index = index.saturating_add(1);
    }
    tokens
}

fn skip_lua_comment(source: &[u8], start: usize) -> usize {
    if let Some(end) = skip_lua_long_bracket(source, start) {
        return end;
    }
    source
        .get(start..)
        .and_then(|rest| rest.iter().position(|byte| matches!(byte, b'\r' | b'\n')))
        .map_or(source.len(), |relative| start.saturating_add(relative))
}

fn skip_lua_long_bracket(source: &[u8], start: usize) -> Option<usize> {
    if source.get(start) != Some(&b'[') {
        return None;
    }
    let mut delimiter = start.saturating_add(1);
    while source.get(delimiter) == Some(&b'=') {
        delimiter = delimiter.saturating_add(1);
    }
    if source.get(delimiter) != Some(&b'[') {
        return None;
    }
    let equals = delimiter.saturating_sub(start).saturating_sub(1);
    let mut search = delimiter.saturating_add(1);
    while let Some(relative) = source
        .get(search..)
        .and_then(|remaining| remaining.iter().position(|byte| *byte == b']'))
    {
        let close = search.saturating_add(relative);
        let mut close_delimiter = close.saturating_add(1);
        while source.get(close_delimiter) == Some(&b'=') {
            close_delimiter = close_delimiter.saturating_add(1);
        }
        if close_delimiter.saturating_sub(close).saturating_sub(1) == equals
            && source.get(close_delimiter) == Some(&b']')
        {
            return Some(close_delimiter.saturating_add(1));
        }
        search = close.saturating_add(1);
    }
    Some(source.len())
}

fn skip_lua_string(source: &[u8], start: usize) -> usize {
    let Some(quote) = source.get(start).copied() else {
        return source.len();
    };
    let mut index = start.saturating_add(1);
    while let Some(byte) = source.get(index).copied() {
        if byte == b'\\' {
            index = index.saturating_add(1);
            if index < source.len() {
                index = index.saturating_add(1);
            }
        } else if byte == quote {
            return index.saturating_add(1);
        } else {
            index = index.saturating_add(1);
        }
    }
    source.len()
}

fn token_is(source: &[u8], token: LuaToken, expected: &[u8]) -> bool {
    source.get(token.start..token.end) == Some(expected)
}

fn next_token_is(source: &[u8], tokens: &[LuaToken], index: &mut usize, expected: &[u8]) -> bool {
    let Some(next) = index.checked_add(1) else {
        return false;
    };
    let Some(token) = tokens.get(next).copied() else {
        return false;
    };
    if token_is(source, token, expected) {
        *index = next;
        true
    } else {
        false
    }
}

fn window_key_then_index(source: &[u8], tokens: &[LuaToken], start: usize) -> Option<usize> {
    let mut cursor = start;
    let current = tokens.get(cursor).copied()?;
    if !token_is(source, current, b"if")
        || !next_token_is(source, tokens, &mut cursor, b"keyboard_action")
        || !next_token_is(source, tokens, &mut cursor, b"==")
    {
        return None;
    }
    let maybe_ui = cursor.checked_add(1).and_then(|index| tokens.get(index).copied());
    if maybe_ui.is_some_and(|token| token_is(source, token, b"ui_events"))
        && (!next_token_is(source, tokens, &mut cursor, b"ui_events")
            || !next_token_is(source, tokens, &mut cursor, b"."))
    {
        return None;
    }
    if !next_token_is(source, tokens, &mut cursor, b"WINDOW_KEY_PRESSED")
        || !next_token_is(source, tokens, &mut cursor, b"then")
    {
        return None;
    }
    Some(cursor)
}

fn is_window_key_if(source: &[u8], tokens: &[LuaToken], start: usize) -> bool {
    window_key_then_index(source, tokens, start).is_some()
}

fn is_quit_key_if(source: &[u8], tokens: &[LuaToken], start: usize) -> bool {
    let Some(token) = tokens.get(start).copied() else {
        return false;
    };
    if !token_is(source, token, b"if") {
        return false;
    }
    let mut cursor = start;
    if !next_token_is(source, tokens, &mut cursor, b"dik") || !next_token_is(source, tokens, &mut cursor, b"==") {
        return false;
    }
    let maybe_keys = cursor.checked_add(1).and_then(|index| tokens.get(index).copied());
    if maybe_keys.is_some_and(|value| token_is(source, value, b"DIK_keys"))
        && (!next_token_is(source, tokens, &mut cursor, b"DIK_keys")
            || !next_token_is(source, tokens, &mut cursor, b"."))
    {
        return false;
    }
    next_token_is(source, tokens, &mut cursor, b"DIK_Q") && next_token_is(source, tokens, &mut cursor, b"then")
}

fn matching_block_end(source: &[u8], tokens: &[LuaToken], start: usize) -> Result<usize, HookError> {
    let mut depth = 0_i32;
    let mut pending_do = 0_i32;
    for (index, token) in tokens.iter().enumerate().skip(start) {
        if token_is(source, *token, b"if")
            || token_is(source, *token, b"function")
            || token_is(source, *token, b"repeat")
        {
            depth = depth
                .checked_add(1)
                .ok_or_else(|| HookError::new("Lua block nesting is too deep"))?;
        } else if token_is(source, *token, b"for") || token_is(source, *token, b"while") {
            depth = depth
                .checked_add(1)
                .ok_or_else(|| HookError::new("Lua block nesting is too deep"))?;
            pending_do = pending_do
                .checked_add(1)
                .ok_or_else(|| HookError::new("Lua loop nesting is too deep"))?;
        } else if token_is(source, *token, b"do") {
            if pending_do > 0 {
                pending_do = pending_do
                    .checked_sub(1)
                    .ok_or_else(|| HookError::new("Lua loop nesting is invalid"))?;
            } else {
                depth = depth
                    .checked_add(1)
                    .ok_or_else(|| HookError::new("Lua block nesting is too deep"))?;
            }
        } else if token_is(source, *token, b"end") || token_is(source, *token, b"until") {
            depth = depth
                .checked_sub(1)
                .ok_or_else(|| HookError::new("Lua block nesting is invalid near the requested hook"))?;
            if depth == 0 {
                return Ok(index);
            }
            if depth < 0 {
                return Err(HookError::new("Lua block nesting is invalid near the requested hook"));
            }
        }
    }
    Err(HookError::new("could not find the end of the required Lua block"))
}

fn unique_substring(source: &[u8], needle: &[u8]) -> Result<usize, HookError> {
    let positions = source
        .windows(needle.len())
        .enumerate()
        .filter_map(|(index, window)| (window == needle).then_some(index))
        .take(2)
        .collect::<Vec<_>>();
    if positions.len() != 1 {
        return Err(HookError::new("source function anchor must occur exactly once"));
    }
    positions
        .first()
        .copied()
        .ok_or_else(|| HookError::new("source function anchor is missing"))
}

fn insert_after_block_end(source: &[u8], end_start: usize, hook: &[u8]) -> Result<Vec<u8>, HookError> {
    let lines = line_offsets(source);
    let line_index = lines
        .iter()
        .position(|(start, end, _)| end_start >= *start && end_start < *end)
        .ok_or_else(|| HookError::new("Lua block end line is missing"))?;
    let (start, end, line_end) = *lines
        .get(line_index)
        .ok_or_else(|| HookError::new("Lua block end line range is invalid"))?;
    let prefix = source
        .get(start..end_start)
        .ok_or_else(|| HookError::new("Lua block end prefix is invalid"))?;
    let keyword_end = end_start.saturating_add(3);
    let suffix = source
        .get(keyword_end..end)
        .ok_or_else(|| HookError::new("Lua block end suffix is invalid"))?;
    if !prefix.iter().all(|byte| matches!(byte, b' ' | b'\t')) || !suffix.iter().all(u8::is_ascii_whitespace) {
        return Err(HookError::new("Lua block end is not a standalone line"));
    }
    let mut hook_line = prefix.to_vec();
    hook_line.extend_from_slice(hook);
    let hook_count = source
        .windows(hook_line.len())
        .filter(|window| *window == hook_line)
        .count();
    if hook_count != 0 {
        if hook_count == 1
            && lines
                .get(line_index.saturating_add(1))
                .is_some_and(|(next, next_end, _)| source.get(*next..*next_end) == Some(hook_line.as_slice()))
        {
            return Ok(source.to_vec());
        }
        return Err(HookError::new(
            "companion menu hook already exists at an unexpected location",
        ));
    }
    let newline: &[u8] = if source.get(end..line_end).is_some_and(|bytes| bytes == b"\r\n") {
        b"\r\n"
    } else if source.get(end..line_end).is_some_and(|bytes| bytes == b"\r") {
        b"\r"
    } else {
        b"\n"
    };
    let insert_at = if line_end > end { line_end } else { end };
    let mut output = Vec::with_capacity(
        source
            .len()
            .saturating_add(hook_line.len())
            .saturating_add(newline.len()),
    );
    output.extend_from_slice(
        source
            .get(..insert_at)
            .ok_or_else(|| HookError::new("Lua block prefix is invalid"))?,
    );
    if line_end == end {
        output.extend_from_slice(newline);
    }
    output.extend_from_slice(&hook_line);
    output.extend_from_slice(newline);
    output.extend_from_slice(
        source
            .get(insert_at..)
            .ok_or_else(|| HookError::new("Lua block suffix is invalid"))?,
    );
    Ok(output)
}

fn remove_after_block_end(source: &[u8], end_start: usize, hook: &[u8]) -> Result<Vec<u8>, HookError> {
    let lines = line_offsets(source);
    let line_index = lines
        .iter()
        .position(|(start, end, _)| end_start >= *start && end_start < *end)
        .ok_or_else(|| HookError::new("Lua block end line is missing"))?;
    let (start, _, _) = *lines
        .get(line_index)
        .ok_or_else(|| HookError::new("Lua block end line range is invalid"))?;
    let prefix = source
        .get(start..end_start)
        .ok_or_else(|| HookError::new("Lua block end prefix is invalid"))?;
    let mut hook_line = prefix.to_vec();
    hook_line.extend_from_slice(hook);
    if source
        .windows(hook_line.len())
        .filter(|window| *window == hook_line)
        .count()
        != 1
    {
        return Err(HookError::new("installed companion menu hook is missing or ambiguous"));
    }
    let Some((hook_start, hook_end, hook_line_end)) = lines.get(line_index.saturating_add(1)).copied() else {
        return Err(HookError::new("installed companion menu hook is missing"));
    };
    if source.get(hook_start..hook_end) != Some(hook_line.as_slice()) {
        return Err(HookError::new(
            "installed companion menu hook is not adjacent to the quit block",
        ));
    }
    let mut output = Vec::with_capacity(source.len());
    output.extend_from_slice(
        source
            .get(..hook_start)
            .ok_or_else(|| HookError::new("menu hook prefix is invalid"))?,
    );
    output.extend_from_slice(
        source
            .get(hook_line_end..)
            .ok_or_else(|| HookError::new("menu hook suffix is invalid"))?,
    );
    Ok(output)
}

/// Patching failure caused by an absent, duplicate, or changed source anchor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookError {
    message: &'static str,
}

impl HookError {
    fn new(message: &'static str) -> Self {
        Self { message }
    }
}

impl std::fmt::Display for HookError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.message)
    }
}

impl std::error::Error for HookError {}

fn validate_tokens(anchor: &[u8], hook: &[u8]) -> Result<(), HookError> {
    if anchor.is_empty() || hook.is_empty() || anchor.contains(&b'\n') || hook.contains(&b'\n') || hook.contains(&b'\r')
    {
        return Err(HookError::new(
            "anchor and hook must be non-empty single-line byte strings",
        ));
    }
    Ok(())
}

fn line_offsets(source: &[u8]) -> Vec<(usize, usize, usize)> {
    let mut offsets = Vec::new();
    let mut start = 0_usize;
    while start < source.len() {
        let line_end = source
            .get(start..)
            .and_then(|remaining| remaining.iter().position(|byte| *byte == b'\n'))
            .map_or(source.len(), |relative| {
                start.saturating_add(relative).saturating_add(1)
            });
        let mut content_end = line_end;
        if source.get(content_end.saturating_sub(1)) == Some(&b'\n') {
            content_end = content_end.saturating_sub(1);
            if source.get(content_end.saturating_sub(1)) == Some(&b'\r') {
                content_end = content_end.saturating_sub(1);
            }
        } else if source.get(content_end.saturating_sub(1)) == Some(&b'\r') {
            content_end = content_end.saturating_sub(1);
        }
        offsets.push((start, content_end, line_end));
        start = line_end;
    }
    if source.is_empty() {
        offsets.push((0, 0, 0));
    }
    offsets
}
