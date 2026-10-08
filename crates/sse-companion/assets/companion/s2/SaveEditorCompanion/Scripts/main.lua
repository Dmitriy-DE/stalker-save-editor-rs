--[[
  S.T.A.L.K.E.R. 2: Heart of Chornobyl — Save Editor Companion (UE4SS Lua Mod).
  EXPERIMENTAL: not verified in the game yet. Everything it does and every error
  is written to %LOCALAPPDATA%\Stalker2\Saved\save_editor_companion.log, which the
  editor adds to its diagnostic reports so problems can be fixed.
  Implements protocol v1 (MOD_COMPANION_PROTOCOL.md) using UE4SS Lua scripting
  and Unreal Engine reflection / GSC debug console commands.

  Target folder: Stalker2/Binaries/Win64/ue4ss/Mods/SaveEditorCompanion/Scripts/main.lua
--]]

local PROTOCOL = "v1"
local POLL_INTERVAL_MS = 2000
local MOD_BUILD = "2026.10.08.1-s2-experimental"
local LOG_LIMIT = 512 * 1024
local MAX_COMMAND_BYTES = 1024 * 1024
local COMMAND_MAX_AGE_SECONDS = 30

local cmd_file_path = nil
local tmp_file_path = nil
local out_file_path = nil

local function get_storage_paths()
	if cmd_file_path ~= nil then
		return cmd_file_path, tmp_file_path, out_file_path
	end

	-- Look for %LOCALAPPDATA%\Stalker2\Saved or fallback to current directory
	local local_app_data = os.getenv("LOCALAPPDATA")
	local base_dir = "."
	if local_app_data ~= nil and local_app_data ~= "" then
		base_dir = local_app_data .. "\\Stalker2\\Saved"
	end

	cmd_file_path = base_dir .. "\\save_editor_cmd.txt"
	tmp_file_path = base_dir .. "\\save_editor_out.tmp"
	out_file_path = base_dir .. "\\save_editor_out.txt"
	return cmd_file_path, tmp_file_path, out_file_path
end

local function log_path()
	local base = os.getenv("LOCALAPPDATA")
	if base == nil or base == "" then
		return "save_editor_companion.log"
	end
	return base .. "\\Stalker2\\Saved\\save_editor_companion.log"
end

-- Append one line; keep the file below LOG_LIMIT by moving it to .old.
local function log(text)
	local path = log_path()
	local f = io.open(path, "r")
	if f ~= nil then
		local size = f:seek("end")
		f:close()
		if size ~= nil and size > LOG_LIMIT then
			os.remove(path .. ".old")
			os.rename(path, path .. ".old")
		end
	end
	f = io.open(path, "a")
	if f == nil then
		return
	end
	f:write(os.date("!%Y-%m-%dT%H:%M:%SZ"), " ", tostring(text), "\n")
	f:close()
end

local function clean_text(text)
	return (string.gsub(tostring(text or ""), "[\r\n]+", " "))
end

local function command_timestamp(id)
	local timestamp = string.match(tostring(id or ""), "^(%d+)%-")
	return tonumber(timestamp)
end

local function stale_command_error(id)
	local timestamp = command_timestamp(id)
	if timestamp == nil then
		return "request timestamp is missing"
	end
	local now = os.time()
	if now == nil then
		return "system clock is unavailable"
	end
	if timestamp > now + 5 then
		return "request timestamp is in the future"
	end
	if now - timestamp > COMMAND_MAX_AGE_SECONDS then
		return "request is stale"
	end
	return nil
end

local function write_reply(id, status, text)
	local _, tmp_path, out_path = get_storage_paths()
	local f = io.open(tmp_path, "w")
	if f == nil then
		print("[SaveEditorCompanion] Error: unable to open output file " .. tostring(tmp_path))
		return
	end
	f:write(PROTOCOL, " ", id, " ", status, " ", clean_text(text), "\n")
	f:close()
	os.remove(out_path)
	os.rename(tmp_path, out_path)
end

-- Unreal Engine helper functions via UE4SS
local function get_world()
	if FindFirstOf ~= nil then
		return FindFirstOf("World")
	end
	return nil
end

local function get_player_controller()
	if FindFirstOf ~= nil then
		return FindFirstOf("PlayerController")
	end
	return nil
end

local function execute_console_command(cmd)
	local world = get_world()
	local pc = get_player_controller()
	if StaticFindObject ~= nil then
		local kismet = StaticFindObject("/Script/Engine.Default__KismetSystemLibrary")
		if kismet ~= nil and kismet.ExecuteConsoleCommand ~= nil and world ~= nil then
			kismet:ExecuteConsoleCommand(world, cmd, pc)
			return true
		end
	end
	if ExecuteConsoleCommand ~= nil and world ~= nil then
		ExecuteConsoleCommand(world, cmd)
		return true
	end
	log("console command not delivered (no World/KismetSystemLibrary): " .. tostring(cmd))
	return false
end

local handlers = {}

function handlers.ping(args)
	return "ok", "pong " .. MOD_BUILD
end

function handlers.info(args)
	return "ok", "stalker2 1.x-ue4ss"
end

function handlers.pos(args)
	local pc = get_player_controller()
	if pc ~= nil and pc:IsValid() and pc.Pawn ~= nil and pc.Pawn:IsValid() then
		local pos = pc.Pawn:K2_GetActorLocation()
		if pos ~= nil then
			return "ok", string.format("%.2f,%.2f,%.2f", pos.X, pos.Y, pos.Z)
		end
	end
	return "error", "player actor not available"
end

function handlers.money(args)
	local delta = tonumber(args[1])
	if delta == nil then
		return "error", "usage: money <amount>"
	end
	if not execute_console_command("XAddMoneyToPlayer " .. tostring(delta)) then
		return "error", "console command unavailable"
	end
	return "ok", "money modified by " .. tostring(delta)
end

function handlers.give(args)
	local proto_id = args[1]
	local count = tonumber(args[2]) or 1
	if proto_id == nil or proto_id == "" then
		return "error", "usage: give <prototype_id> [count]"
	end
	-- Native S2 command: XCreateItemInInventoryByID <PrototypeID> <ObjUID> <Count> <Durability>
	if not execute_console_command(string.format("XCreateItemInInventoryByID %s 0 %d 1.0", proto_id, count)) then
		return "error", "console command unavailable"
	end
	return "ok", string.format("spawned %d of %s", count, proto_id)
end

function handlers.teleport(args)
	local x = tonumber(args[1])
	local y = tonumber(args[2])
	local z = tonumber(args[3])
	if x == nil or y == nil or z == nil then
		return "error", "usage: teleport <x> <y> <z>"
	end
	-- Use native GSC command XTeleportTo or actor K2_SetActorLocation
	local pc = get_player_controller()
	if pc ~= nil and pc:IsValid() and pc.Pawn ~= nil and pc.Pawn:IsValid() then
		pc.Pawn:K2_SetActorLocation({ X = x, Y = y, Z = z }, false, {}, true)
		return "ok", string.format("teleported to %.2f, %.2f, %.2f", x, y, z)
	end
	if not execute_console_command(string.format("XTeleportTo %.2f %.2f %.2f", x, y, z)) then
		return "error", "console command unavailable"
	end
	return "ok", string.format("teleported via command to %.2f, %.2f, %.2f", x, y, z)
end

-- Native GSC debug commands of S2 (the same ones UETools / Stalker2Control type into the console).
local function on_off(value)
	if value == "on" then return true end
	if value == "off" then return false end
	return nil
end

function handlers.god(args)
	local on = on_off(args[1])
	if on == nil then
		return "error", "usage: god on|off"
	end
	if not execute_console_command("XSetGodMode " .. (on and "true" or "false")) then
		return "error", "console command unavailable"
	end
	return "ok", "god mode " .. args[1]
end

function handlers.noclip(args)
	local on = on_off(args[1])
	if on == nil then
		return "error", "usage: noclip on|off"
	end
	if not execute_console_command(on and "XSetNoClipGSC 1" or "XSetNoClipGSC 0") then
		return "error", "console command unavailable"
	end
	return "ok", "free flight " .. args[1]
end

function handlers.timespeed(args)
	local speed = tonumber(args[1])
	if speed == nil or speed < 0 or speed > 100 then
		return "error", "usage: timespeed <0..100> (0 = normal)"
	end
	if not execute_console_command("XSetTimeSpeed " .. tostring(speed)) then
		return "error", "console command unavailable"
	end
	return "ok", "time speed " .. tostring(speed)
end

function handlers.weather(args)
	if args[1] == nil then
		return "error", "usage: weather <preset>"
	end
	if not execute_console_command("XForceWeather " .. args[1]) then
		return "error", "console command unavailable"
	end
	return "ok", "weather " .. args[1]
end

local function process_command_line(line)
	local tokens = {}
	for token in string.gmatch(line, "%S+") do
		table.insert(tokens, token)
	end

	if #tokens < 2 then
		return
	end

	local proto = tokens[1]
	local id = tokens[2]
	local cmd = tokens[3]

	if proto ~= PROTOCOL then
		write_reply(id or "0", "error", "unsupported protocol version: " .. tostring(proto))
		return
	end
	local stale_error = stale_command_error(id)
	if stale_error ~= nil then
		write_reply(id, "error", stale_error)
		return
	end

	if cmd == nil then
		write_reply(id, "error", "missing command")
		return
	end

	local args = {}
	for i = 4, #tokens do
		table.insert(args, tokens[i])
	end

	local handler = handlers[cmd]
	if handler == nil then
		write_reply(id, "unsupported", "command not supported in S2: " .. cmd)
		return
	end

	local ok, status, text = pcall(handler, args)
	if not ok then
		log("error in " .. cmd .. ": " .. tostring(status))
		write_reply(id, "error", "mod error: " .. tostring(status))
		return
	end
	log(string.format("%s %s -> %s %s", cmd, table.concat(args, " "), tostring(status), tostring(text or "")))
	write_reply(id, status, text or "")
end

local function poll_commands()
	local cmd_path, _, _ = get_storage_paths()
	local f = io.open(cmd_path, "r")
	if f == nil then
		return
	end

	local content = f:read(MAX_COMMAND_BYTES + 1)
	f:close()
	os.remove(cmd_path)

	if content == nil or content == "" then
		return
	end
	if #content > MAX_COMMAND_BYTES then
		write_reply("0", "error", "command exceeds the size limit")
		return
	end
	local line = string.match(content, "^([^\r\n]*)\r?\n?$")
	if line == nil or line == "" then
		write_reply("0", "error", "expected exactly one command line")
		return
	end
	process_command_line(line)
end

-- Initialize periodic polling in UE4SS
if LoopAsync ~= nil then
	LoopAsync(POLL_INTERVAL_MS, function()
		local ok, err = pcall(poll_commands)
		if not ok then
			log("poll error: " .. tostring(err))
		end
		return false -- continue looping
	end)
	log("started " .. MOD_BUILD .. " (experimental)")
	print("[SaveEditorCompanion] S.T.A.L.K.E.R. 2 companion initialized with LoopAsync (" .. POLL_INTERVAL_MS .. "ms)")
else
	print("[SaveEditorCompanion] S.T.A.L.K.E.R. 2 companion loaded (LoopAsync not available)")
end
