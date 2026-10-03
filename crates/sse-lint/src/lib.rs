//! Static analysis and game script linters for S.T.A.L.K.E.R. games.
//!
//! Owner: Gemini (G7).
//!
//! Implements native game script static checkers over [`sse_content::GameFileTree`]:
//! - `check_condlists`: Syntax validation of `{}` and `%` condlists in LTX logic
//! - `check_condfuncs`: Logic conditions and effects referencing undefined script functions
//! - `check_dialogs`: Dialog phrase graph checks (missing phrases, duplicate IDs, missing start phrase 0)
//! - `check_infos`: Logic testing info portions that are never given anywhere
//! - `check_logic_refs`: Logic scheme section references
//! - `check_module_calls`: Undefined `module.func` calls across scripts and XML/LTX
//! - `check_trade_items`: Trade configs selling items not defined in game sections
//! - `lua_globals`: Misspelled or unassigned Lua globals
//! - `spawn_diff`: Level spawn binary difference analysis
//! - `ee_diff`: Retail vs Enhanced Edition script and config diffs

pub mod condfuncs;
pub mod condlists;
pub mod dialogs;
pub mod ee_diff;
pub mod engine;
pub mod globals;
pub mod infos;
pub mod lexer;
pub mod logic_refs;
pub mod models;
pub mod regex;
pub mod module_calls;
pub mod spawn_diff;
pub mod trade_items;

pub use condfuncs::{check_condfuncs_line, extract_lua_function_defs};
pub use condlists::{check_condlist_line, check_condlists_text};
pub use dialogs::check_dialogs_xml;
pub use ee_diff::{diff_retail_and_ee, DiffHunk};
pub use engine::{LintEngine, LintOptions};
pub use globals::LuaGlobalsAnalyzer;
pub use infos::InfoPortionIndex;
pub use lexer::{is_keyword, LuaLexer, Token, TokenKind};
pub use logic_refs::check_logic_refs_text;
pub use models::{LintFinding, LintReport, LintSeverity};
pub use module_calls::{analyze_script_module, check_script_module_calls, check_xml_module_refs, ScriptModuleInfo};
pub use spawn_diff::{diff_spawns, parse_spawn_objects, PatrolPath, PatrolPoint, SpawnDiffResult, SpawnObject};
pub use trade_items::check_trade_file;
