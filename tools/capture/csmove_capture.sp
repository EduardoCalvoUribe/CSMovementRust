// SPDX-License-Identifier: GPL-3.0-or-later
// csmove_capture: input injection and ground-truth logging for CSMovementRust (game-plan.md §9.4).
//
// Drives one bot with a recorded command stream and logs a `pre` row (before PlayerRunCommand) and a
// `post` row (after it) per command, plus meta.toml and scenario.toml, in the capture format of §9.3.
//
//   csmove_run <scenario>       run data/csmove/scenarios/<scenario>
//   csmove_batch <listfile>     run every scenario named in data/csmove/scenarios/<listfile>
//   csmove_stop                 abort
//   csmove_dump                 print the movement cvars
//
// Floats are logged as decimal and as raw IEEE-754 bits. Files are buffered in memory and written when
// a scenario ends, so the server never hitches on disk writes mid-run.

#include <sourcemod>
#include <sdktools>
#include <cstrike>

#pragma semicolon 1
#pragma newdecls required

#define PLUGIN_VERSION "1.1.0"
#define MAX_CMDS 4096

// GOKZ (optional): only the mode is set, so the native is declared here instead of pulling in GOKZ's
// include files (gokz/core.inc: Mode_Vanilla 0, Mode_SimpleKZ 1, Mode_KZTimer 2).
native bool GOKZ_SetMode(int client, int mode);
native any GOKZ_GetOption(int client, const char[] option);

public APLRes AskPluginLoad2(Handle myself, bool late, char[] error, int err_max)
{
	MarkNativeAsOptional("GOKZ_SetMode");
	MarkNativeAsOptional("GOKZ_GetOption");
	return APLRes_Success;
}

public Plugin myinfo =
{
	name = "csmove_capture",
	author = "CSMovementRust",
	description = "Command injection and movement logging for comparison captures",
	version = PLUGIN_VERSION,
	url = ""
};

enum RunState
{
	State_Idle,
	State_Respawn,
	State_Settle,
	State_Run,
	State_Cooldown
};

static const char g_Cvars[][] =
{
	"sv_gravity", "sv_jump_impulse", "sv_accelerate", "sv_airaccelerate", "sv_air_max_wishspeed",
	"sv_friction", "sv_stopspeed", "sv_maxspeed", "sv_maxvelocity", "sv_stepsize", "sv_bounce",
	"sv_enablebunnyhopping", "sv_autobunnyhopping", "sv_staminamax", "sv_staminajumpcost",
	"sv_staminalandcost", "sv_staminarecoveryrate", "sv_accelerate_use_weapon_speed",
	"sv_accelerate_debug_speed", "sv_ladder_scale_speed", "sv_ladder_angle", "sv_ladder_dampen",
	"sv_timebetweenducks", "sv_standable_normal", "sv_walkable_normal", "sv_wateraccelerate",
	"sv_waterfriction", "sv_noclipaccelerate", "sv_noclipspeed", "sv_specaccelerate", "sv_backspeed",
	"sv_maxusrcmdprocessticks", "sv_clamp_unsafe_velocities", "sv_extract_ammo_from_dropped_weapons",
	"sv_jump_spam_penalty_time", "sv_friction_and_accelerate_debug", "sv_cheats", "host_timescale"
};

RunState g_State = State_Idle;
int g_Bot = -1;
int g_Wait;
int g_Index;
int g_Settle;
int g_Count;
char g_Name[128];
char g_ScenarioMap[64];
char g_Source[PLATFORM_MAX_PATH];
float g_Origin[3];
float g_Yaw;
char g_Unavailable[512];
int g_LastCmd;
bool g_Gap;
// The run started with a slowdown left from fall damage (m_flVelocityModifier below 1).
bool g_Slowed;
// The run started without the knife as the active weapon (speed is the active weapon's: 240 for a pistol).
bool g_NoKnife;
int g_Retries;

// Command stream.
float g_Fwd[MAX_CMDS];
float g_Side[MAX_CMDS];
float g_Up[MAX_CMDS];
int g_Buttons[MAX_CMDS];
float g_Ang[MAX_CMDS][3];

ArrayList g_Rows;
ArrayList g_Queue;
ConVar g_QuitWhenDone;
ConVar g_MapHash;
ConVar g_Mode;
ConVar g_AutoBatch;
ConVar g_Target;

public void OnPluginStart()
{
	RegServerCmd("csmove_run", Cmd_Run, "csmove_run <scenario>");
	RegServerCmd("csmove_batch", Cmd_Batch, "csmove_batch <listfile>");
	RegServerCmd("csmove_stop", Cmd_Stop, "abort the current scenario and queue");
	RegServerCmd("csmove_dump", Cmd_Dump, "print movement cvars");
	g_QuitWhenDone = CreateConVar("csmove_quit_when_done", "0", "Quit the server when a batch finishes");
	g_MapHash = CreateConVar("csmove_map_hash", "", "Hash of the BSP, recorded in meta.toml");
	g_Mode = CreateConVar("csmove_mode", "vanilla", "Movement mode recorded in meta.toml");
	g_AutoBatch = CreateConVar("csmove_autobatch", "", "List file to run automatically after the map loads");
	g_Target = CreateConVar("csmove_target", "human", "Who is driven: human (a connected client) or bot");
	g_Rows = new ArrayList(ByteCountToCells(1024));
	g_Queue = new ArrayList(ByteCountToCells(128));
	char dir[PLATFORM_MAX_PATH];
	BuildPath(Path_SM, dir, sizeof(dir), "data/csmove");
	CreateDirectory(dir, 511);
	BuildPath(Path_SM, dir, sizeof(dir), "data/csmove/results");
	CreateDirectory(dir, 511);
}

public void OnMapStart()
{
	g_State = State_Idle;
	g_Bot = -1;
}

public void OnConfigsExecuted()
{
	// cfg/csmove_run.cfg is written by tools/capture/run.ps1 (mode, list, quit flag).
	ServerCommand("exec csmove_run");
	CreateTimer(3.0, Timer_AutoBatch);
}

public Action Timer_AutoBatch(Handle timer)
{
	char list[64];
	g_AutoBatch.GetString(list, sizeof(list));
	if (list[0] != '\0')
	{
		// Game-mode configs add their own bots; exactly one driven player may exist during a capture.
		// A server with no humans hibernates and stops ticking.
		ServerCommand("sv_hibernate_when_empty 0");
		ServerCommand("bot_kick");
		ServerCommand("bot_quota_mode normal");
		ServerCommand("bot_quota %d", UseBot() ? 1 : 0);
		ServerCommand("mp_warmup_end");
		CreateTimer(3.0, Timer_StartBatch);
	}
	return Plugin_Stop;
}

public Action Timer_StartBatch(Handle timer)
{
	char list[64];
	g_AutoBatch.GetString(list, sizeof(list));
	ServerCommand("csmove_batch %s", list);
	return Plugin_Stop;
}

// ---------------------------------------------------------------------------------------------------
// Hex helpers. StringToInt clamps above 0x7fffffff, and negative floats have the top bit set, so the
// bit patterns are converted by hand.

int HexToInt(const char[] s)
{
	int v = 0;
	for (int i = 0; s[i] != '\0'; i++)
	{
		int c = s[i];
		int d;
		if (c >= '0' && c <= '9') d = c - '0';
		else if (c >= 'a' && c <= 'f') d = c - 'a' + 10;
		else if (c >= 'A' && c <= 'F') d = c - 'A' + 10;
		else break;
		v = (v << 4) | d;
	}
	return v;
}

void IntToHex(int v, char[] out, int len)
{
	static const char digits[] = "0123456789abcdef";
	char buf[9];
	for (int i = 0; i < 8; i++)
	{
		buf[i] = digits[(v >>> (28 - 4 * i)) & 0xF];
	}
	buf[8] = '\0';
	strcopy(out, len, buf);
}

void FloatBits(float f, char[] out, int len)
{
	IntToHex(view_as<int>(f), out, len);
}

// ---------------------------------------------------------------------------------------------------
// Commands

public Action Cmd_Run(int args)
{
	char name[128];
	GetCmdArg(1, name, sizeof(name));
	g_Queue.Clear();
	g_Queue.PushString(name);
	StartNext();
	return Plugin_Handled;
}

public Action Cmd_Batch(int args)
{
	char file[64], path[PLATFORM_MAX_PATH], line[128];
	GetCmdArg(1, file, sizeof(file));
	BuildPath(Path_SM, path, sizeof(path), "data/csmove/scenarios/%s", file);
	File f = OpenFile(path, "r");
	if (f == null)
	{
		PrintToServer("[csmove] cannot open %s", path);
		return Plugin_Handled;
	}
	g_Queue.Clear();
	while (f.ReadLine(line, sizeof(line)))
	{
		TrimString(line);
		if (line[0] != '\0')
		{
			g_Queue.PushString(line);
		}
	}
	delete f;
	PrintToServer("[csmove] batch %s: %d scenarios", file, g_Queue.Length);
	StartNext();
	return Plugin_Handled;
}

public Action Cmd_Stop(int args)
{
	g_Queue.Clear();
	g_State = State_Idle;
	PrintToServer("[csmove] stopped");
	return Plugin_Handled;
}

public Action Cmd_Dump(int args)
{
	char v[64];
	for (int i = 0; i < sizeof(g_Cvars); i++)
	{
		ConVar c = FindConVar(g_Cvars[i]);
		if (c != null)
		{
			c.GetString(v, sizeof(v));
			PrintToServer("%s = %s", g_Cvars[i], v);
		}
	}
	return Plugin_Handled;
}

void StartNext()
{
	if (g_Queue.Length == 0)
	{
		g_State = State_Idle;
		PrintToServer("[csmove] batch done");
		if (g_QuitWhenDone.BoolValue)
		{
			ServerCommand("quit");
		}
		return;
	}
	char name[128], map[64];
	g_Queue.GetString(0, name, sizeof(name));
	g_Queue.Erase(0);
	if (!LoadScenario(name))
	{
		StartNext();
		return;
	}
	// Scenarios name their map (the test level unless `map` is given in scenario.cfg).
	GetCurrentMap(map, sizeof(map));
	if (!StrEqual(map, g_ScenarioMap))
	{
		PrintToServer("[csmove] %s is for map %s, server runs %s; skipped", name, g_ScenarioMap, map);
		StartNext();
		return;
	}
	g_Rows.Clear();
	g_Unavailable[0] = '\0';
	g_Gap = false;
	g_Slowed = false;
	g_NoKnife = false;
	g_Index = 0;
	g_Wait = 0;
	g_State = State_Respawn;
	PrintToServer("[csmove] %s: %d commands, settle %d", g_Name, g_Count, g_Settle);
}

bool LoadScenario(const char[] name)
{
	char path[PLATFORM_MAX_PATH], line[512];
	strcopy(g_Name, sizeof(g_Name), name);
	BuildPath(Path_SM, g_Source, sizeof(g_Source), "data/csmove/scenarios/%s", name);

	Format(path, sizeof(path), "%s/scenario.cfg", g_Source);
	File f = OpenFile(path, "r");
	if (f == null)
	{
		PrintToServer("[csmove] missing %s", path);
		return false;
	}
	g_Settle = 1;
	g_Yaw = 0.0;
	strcopy(g_ScenarioMap, sizeof(g_ScenarioMap), "csmove_capture");
	while (f.ReadLine(line, sizeof(line)))
	{
		TrimString(line);
		char parts[4][64];
		int n = ExplodeString(line, " ", parts, sizeof(parts), sizeof(parts[]));
		if (n >= 4 && StrEqual(parts[0], "origin"))
		{
			g_Origin[0] = StringToFloat(parts[1]);
			g_Origin[1] = StringToFloat(parts[2]);
			g_Origin[2] = StringToFloat(parts[3]);
		}
		else if (n >= 2 && StrEqual(parts[0], "yaw"))
		{
			g_Yaw = StringToFloat(parts[1]);
		}
		else if (n >= 2 && StrEqual(parts[0], "map"))
		{
			strcopy(g_ScenarioMap, sizeof(g_ScenarioMap), parts[1]);
		}
		else if (n >= 2 && StrEqual(parts[0], "settle"))
		{
			g_Settle = StringToInt(parts[1]);
		}
		else if (n >= 2 && StrEqual(parts[0], "tickrate"))
		{
			int want = StringToInt(parts[1]);
			int have = RoundToNearest(1.0 / GetTickInterval());
			if (want != have)
			{
				PrintToServer("[csmove] %s wants %d tick, server runs %d; skipped", name, want, have);
				delete f;
				return false;
			}
		}
	}
	delete f;

	Format(path, sizeof(path), "%s/cmds.csv", g_Source);
	f = OpenFile(path, "r");
	if (f == null)
	{
		PrintToServer("[csmove] missing %s", path);
		return false;
	}
	g_Count = 0;
	f.ReadLine(line, sizeof(line)); // header
	while (f.ReadLine(line, sizeof(line)) && g_Count < MAX_CMDS)
	{
		TrimString(line);
		if (line[0] == '\0')
		{
			continue;
		}
		char c[15][24];
		if (ExplodeString(line, ",", c, sizeof(c), sizeof(c[])) != 15)
		{
			PrintToServer("[csmove] bad cmds row: %s", line);
			delete f;
			return false;
		}
		g_Buttons[g_Count] = StringToInt(c[4]);
		g_Fwd[g_Count] = view_as<float>(HexToInt(c[9]));
		g_Side[g_Count] = view_as<float>(HexToInt(c[10]));
		g_Up[g_Count] = view_as<float>(HexToInt(c[11]));
		g_Ang[g_Count][0] = view_as<float>(HexToInt(c[12]));
		g_Ang[g_Count][1] = view_as<float>(HexToInt(c[13]));
		g_Ang[g_Count][2] = view_as<float>(HexToInt(c[14]));
		g_Count++;
	}
	delete f;
	return g_Count > 0;
}

// ---------------------------------------------------------------------------------------------------
// Bot control

bool UseBot()
{
	char t[16];
	g_Target.GetString(t, sizeof(t));
	return StrEqual(t, "bot");
}

// Bots have AI-side movement behavior (they crouch-jump by themselves), so captures drive a real
// client by default; `csmove_target bot` remains for rig debugging.
int FindDriven()
{
	bool bot = UseBot();
	for (int i = 1; i <= MaxClients; i++)
	{
		if (!IsClientInGame(i) || IsClientSourceTV(i) || IsFakeClient(i) != bot)
		{
			continue;
		}
		if (GetClientTeam(i) < 2)
		{
			ChangeClientTeam(i, 2);
		}
		return i;
	}
	return -1;
}

void StripToKnife(int client)
{
	for (int slot = 0; slot < 5; slot++)
	{
		if (slot == CS_SLOT_KNIFE)
		{
			continue;
		}
		// Bounded: RemovePlayerItem can refuse (C4), and the slot would never empty.
		for (int tries = 0; tries < 8; tries++)
		{
			int w = GetPlayerWeaponSlot(client, slot);
			if (w == -1 || !RemovePlayerItem(client, w))
			{
				break;
			}
			AcceptEntityInput(w, "Kill");
		}
	}
	if (GetPlayerWeaponSlot(client, CS_SLOT_KNIFE) == -1)
	{
		GivePlayerItem(client, "weapon_knife");
	}
	int knife = GetPlayerWeaponSlot(client, CS_SLOT_KNIFE);
	if (knife != -1)
	{
		EquipPlayerWeapon(client, knife);
		SetEntPropEnt(client, Prop_Send, "m_hActiveWeapon", knife);
	}
}

/// Players other than the driven one: any of them could collide with or shoot it.
int CountOthers()
{
	int n = 0;
	for (int i = 1; i <= MaxClients; i++)
	{
		if (i != g_Bot && IsClientInGame(i) && !IsClientSourceTV(i) && GetClientTeam(i) > 1)
		{
			n++;
		}
	}
	return n;
}

int g_Frames;

public void OnGameFrame()
{
	g_Frames++;
	if (g_State != State_Idle && g_Frames % 128 == 0)
	{
		PrintToServer("[csmove] trace: state %d index %d wait %d bot %d alive %d bots %d", g_State, g_Index, g_Wait, g_Bot,
			(g_Bot > 0 && IsClientInGame(g_Bot)) ? view_as<int>(IsPlayerAlive(g_Bot)) : -1, CountOthers());
	}
	if (g_State == State_Settle || g_State == State_Run)
	{
		// A run where the bot died, left, or gained company is not comparable: discard and retry.
		bool dead = !IsClientInGame(g_Bot) || !IsPlayerAlive(g_Bot);
		if (dead || CountOthers() != 0)
		{
			ServerCommand("bot_kick");
			Requeue(dead ? "player dead" : "other players");
		}
		return;
	}
	if (g_State != State_Respawn && g_State != State_Cooldown)
	{
		return;
	}
	g_Wait++;
	if (g_State == State_Cooldown)
	{
		if (g_Wait >= 16)
		{
			StartNext();
		}
		return;
	}
	g_Bot = FindDriven();
	if (g_Bot == -1)
	{
		if (UseBot() && g_Wait % 64 == 1)
		{
			ServerCommand("bot_add_t");
		}
		return;
	}
	// Respawning a live bot hangs the server; a dead one is respawned, a live one is reused (the
	// teleport and settle ticks reset its motion, and stamina is set below).
	if (!IsPlayerAlive(g_Bot))
	{
		if (g_Wait % 32 == 1)
		{
			CS_RespawnPlayer(g_Bot);
		}
		return;
	}
	if (g_Wait == 8)
	{
		StripToKnife(g_Bot);
	}
	if (g_Wait >= 16)
	{
		if (!SetGokzMode(g_Bot))
		{
			g_Queue.Clear();
			g_State = State_Idle;
			return;
		}
		SetEntPropFloat(g_Bot, Prop_Send, "m_flStamina", 0.0);
		g_Index = -g_Settle;
		g_State = State_Settle;
	}
}

public Action OnPlayerRunCmd(int client, int &buttons, int &impulse, float vel[3], float angles[3], int &weapon,
	int &subtype, int &cmdnum, int &tickcount, int &seed, int mouse[2])
{
	if (client != g_Bot || (g_State != State_Settle && g_State != State_Run))
	{
		return Plugin_Continue;
	}
	// A repeated or skipped command number means the engine ran something other than one fresh
	// client command (a dropped or duplicated command): the run is discarded and retried.
	if (g_Index != -g_Settle && cmdnum != g_LastCmd + 1)
	{
		g_Gap = true;
	}
	g_LastCmd = cmdnum;
	if (g_State == State_Settle)
	{
		if (g_Index == -g_Settle)
		{
			float ang[3];
			ang[1] = g_Yaw;
			TeleportEntity(client, g_Origin, ang, view_as<float>({0.0, 0.0, 0.0}));
			// Fall damage from an earlier run leaves a recovering slowdown (m_flVelocityModifier) that
			// would scale this run's acceleration; reset it and health so runs are independent.
			SetEntPropFloat(client, Prop_Send, "m_flVelocityModifier", 1.0);
			SetEntityHealth(client, 100);
			SetEntPropFloat(client, Prop_Send, "m_flStamina", 0.0);
			// A run that ended in mid-air leaves its fall speed behind: without this the settle lands
			// with it, takes fall damage, and the next run starts slowed.
			SetEntPropFloat(client, Prop_Send, "m_flFallVelocity", 0.0);
		}
		// Re-applied every settle tick: the slowdown can also be set after the teleport (first run
		// after the client joins showed a 0.96 modifier at tick 0).
		SetEntPropFloat(client, Prop_Send, "m_flVelocityModifier", 1.0);
		buttons = 0;
		vel[0] = 0.0;
		vel[1] = 0.0;
		vel[2] = 0.0;
		angles[0] = 0.0;
		angles[1] = g_Yaw;
		angles[2] = 0.0;
	}
	else
	{
		int k = g_Index;
		if (k == 0 && GetEntPropFloat(client, Prop_Send, "m_flVelocityModifier") < 1.0)
		{
			g_Slowed = true;
		}
		if (k == 0)
		{
			char cls[64];
			ActiveWeapon(client, cls, sizeof(cls));
			if (!StrEqual(cls, "weapon_knife") && StrContains(cls, "knife") == -1)
			{
				g_NoKnife = true;
			}
		}
		LogRow(client, k, "pre", cmdnum);
		buttons = g_Buttons[k];
		vel[0] = g_Fwd[k];
		vel[1] = g_Side[k];
		vel[2] = g_Up[k];
		angles[0] = g_Ang[k][0];
		angles[1] = g_Ang[k][1];
		angles[2] = g_Ang[k][2];
	}
	impulse = 0;
	weapon = 0;
	subtype = 0;
	mouse[0] = 0;
	mouse[1] = 0;
	return Plugin_Changed;
}

public void OnPlayerRunCmdPost(int client, int buttons, int impulse, const float vel[3], const float angles[3],
	int weapon, int subtype, int cmdnum, int tickcount, int seed, const int mouse[2])
{
	if (client != g_Bot)
	{
		return;
	}
	if (g_State == State_Settle)
	{
		g_Index++;
		if (g_Index == 0)
		{
			g_State = State_Run;
		}
		return;
	}
	if (g_State != State_Run)
	{
		return;
	}
	LogRow(client, g_Index, "post", cmdnum);
	g_Index++;
	if (g_Index >= g_Count)
	{
		Finish();
	}
}

// ---------------------------------------------------------------------------------------------------
// Logging

void AddFloat(char[] dec, int declen, char[] bits, int bitslen, float f, bool available)
{
	char d[32], b[16];
	if (available)
	{
		Format(d, sizeof(d), "%.9f", f);
		FloatBits(f, b, sizeof(b));
	}
	Format(dec, declen, "%s,%s", dec, d);
	Format(bits, bitslen, "%s,%s", bits, b);
}

bool Has(int client, PropType type, const char[] prop)
{
	if (HasEntProp(client, type, prop))
	{
		return true;
	}
	if (StrContains(g_Unavailable, prop) == -1)
	{
		Format(g_Unavailable, sizeof(g_Unavailable), "%s%s;", g_Unavailable, prop);
	}
	return false;
}

void ActiveWeapon(int client, char[] cls, int len)
{
	int w = GetEntPropEnt(client, Prop_Send, "m_hActiveWeapon");
	cls[0] = ' ';
	if (w != -1 && IsValidEntity(w))
	{
		GetEntityClassname(w, cls, len);
	}
}

void LogRow(int client, int k, const char[] phase, int cmdnum)
{
	float origin[3], velocity[3], basevel[3], eye[3], ladder[3];
	GetEntPropVector(client, Prop_Data, "m_vecOrigin", origin);
	GetEntPropVector(client, Prop_Data, "m_vecVelocity", velocity);
	GetEntPropVector(client, Prop_Data, "m_vecBaseVelocity", basevel);
	GetClientEyeAngles(client, eye);
	bool hasLadder = Has(client, Prop_Send, "m_vecLadderNormal");
	if (hasLadder)
	{
		GetEntPropVector(client, Prop_Send, "m_vecLadderNormal", ladder);
	}
	bool hasDA = Has(client, Prop_Send, "m_flDuckAmount");
	bool hasDS = Has(client, Prop_Send, "m_flDuckSpeed");
	bool hasSt = Has(client, Prop_Send, "m_flStamina");
	bool hasSF = Has(client, Prop_Data, "m_surfaceFriction");
	bool hasMS = Has(client, Prop_Send, "m_flMaxspeed");
	bool hasFV = Has(client, Prop_Send, "m_flFallVelocity");
	bool hasOB = Has(client, Prop_Data, "m_nOldButtons");
	float da = hasDA ? GetEntPropFloat(client, Prop_Send, "m_flDuckAmount") : 0.0;
	float ds = hasDS ? GetEntPropFloat(client, Prop_Send, "m_flDuckSpeed") : 0.0;
	float st = hasSt ? GetEntPropFloat(client, Prop_Send, "m_flStamina") : 0.0;
	float sf = hasSF ? GetEntPropFloat(client, Prop_Data, "m_surfaceFriction") : 0.0;
	float ms = hasMS ? GetEntPropFloat(client, Prop_Send, "m_flMaxspeed") : 0.0;
	float fv = hasFV ? GetEntPropFloat(client, Prop_Send, "m_flFallVelocity") : 0.0;

	char ob[16];
	if (hasOB)
	{
		IntToString(GetEntProp(client, Prop_Data, "m_nOldButtons"), ob, sizeof(ob));
	}

	// The column order matches STATES_HEADER in tools/compare/src/capture.rs.
	char dec[1024], bits[512];
	Format(dec, sizeof(dec), "%d,%s,%d,%d", k, phase, cmdnum, GetGameTickCount());
	bits[0] = '\0';
	for (int i = 0; i < 3; i++) AddFloat(dec, sizeof(dec), bits, sizeof(bits), origin[i], true);
	for (int i = 0; i < 3; i++) AddFloat(dec, sizeof(dec), bits, sizeof(bits), velocity[i], true);
	for (int i = 0; i < 3; i++) AddFloat(dec, sizeof(dec), bits, sizeof(bits), basevel[i], true);
	for (int i = 0; i < 3; i++) AddFloat(dec, sizeof(dec), bits, sizeof(bits), eye[i], true);

	int ground = GetEntPropEnt(client, Prop_Data, "m_hGroundEntity");
	Format(dec, sizeof(dec), "%s,%d,%d,%d,%d,%d", dec, ground, GetEntityFlags(client), view_as<int>(GetEntityMoveType(client)),
		GetEntProp(client, Prop_Send, "m_bDucked"), GetEntProp(client, Prop_Send, "m_bDucking"));

	char tail[256];
	tail[0] = '\0';
	AddFloat(dec, sizeof(dec), tail, sizeof(tail), da, hasDA);
	AddFloat(dec, sizeof(dec), tail, sizeof(tail), ds, hasDS);
	AddFloat(dec, sizeof(dec), tail, sizeof(tail), st, hasSt);
	AddFloat(dec, sizeof(dec), tail, sizeof(tail), sf, hasSF);
	AddFloat(dec, sizeof(dec), tail, sizeof(tail), ms, hasMS);
	AddFloat(dec, sizeof(dec), tail, sizeof(tail), fv, hasFV);
	Format(dec, sizeof(dec), "%s,%s", dec, ob);
	char lb[128];
	lb[0] = '\0';
	for (int i = 0; i < 3; i++) AddFloat(dec, sizeof(dec), lb, sizeof(lb), ladder[i], hasLadder);

	char row[1024];
	char cls[64];
	ActiveWeapon(client, cls, sizeof(cls));
	Format(row, sizeof(row), "%s%s%s%s,%d,%.9f,%s", dec, bits, tail, lb, GetClientHealth(client),
		GetEntPropFloat(client, Prop_Send, "m_flVelocityModifier"), cls);
	g_Rows.PushString(row);
}

void Requeue(const char[] why)
{
	g_Retries++;
	if (g_Retries > 5)
	{
		PrintToServer("[csmove] %s: %s, giving up after 5 retries", g_Name, why);
		g_Retries = 0;
	}
	else
	{
		PrintToServer("[csmove] %s: %s, retrying", g_Name, why);
		if (g_Queue.Length == 0)
		{
			g_Queue.PushString(g_Name);
		}
		else
		{
			g_Queue.ShiftUp(0);
			g_Queue.SetString(0, g_Name);
		}
	}
	g_State = State_Cooldown;
	g_Wait = 0;
}

/// Put the driven player in the GOKZ mode named by csmove_mode. Vanilla runs need no GOKZ.
bool SetGokzMode(int client)
{
	char mode[32];
	g_Mode.GetString(mode, sizeof(mode));
	int id = -1;
	if (StrEqual(mode, "simplekz")) id = 1;
	else if (StrEqual(mode, "kztimer")) id = 2;
	else if (StrEqual(mode, "vanilla")) id = 0;
	bool loaded = LibraryExists("gokz-core")
		|| GetFeatureStatus(FeatureType_Native, "GOKZ_SetMode") == FeatureStatus_Available;
	if (!loaded)
	{
		if (id > 0)
		{
			PrintToServer("[csmove] mode %s needs GOKZ, which is not loaded; aborting batch", mode);
			return false;
		}
		return true;
	}
	// SetMode reports false when nothing changes (GOKZ remembers the client's mode), so check the
	// resulting option instead.
	GOKZ_SetMode(client, id);
	int now = GOKZ_GetOption(client, "GOKZ - Mode");
	if (now != id)
	{
		PrintToServer("[csmove] GOKZ refused mode %s (%d, still %d)", mode, id, now);
		return false;
	}
	return true;
}

void Finish()
{
	if (g_Gap)
	{
		Requeue("command number gap");
		return;
	}
	if (g_Slowed)
	{
		Requeue("started slowed by fall damage");
		return;
	}
	if (g_NoKnife)
	{
		Requeue("knife not active at the start");
		return;
	}
	g_Retries = 0;
	char dir[PLATFORM_MAX_PATH], path[PLATFORM_MAX_PATH], v[128];
	BuildPath(Path_SM, dir, sizeof(dir), "data/csmove/results/%s", g_Name);
	CreateDirectory(dir, 511);

	Format(path, sizeof(path), "%s/states.csv", dir);
	File f = OpenFile(path, "w");
	f.WriteLine("tick,phase,cmdnum,server_tick,origin_x,origin_y,origin_z,vel_x,vel_y,vel_z,basevel_x,basevel_y,basevel_z,pitch,yaw,roll,ground,flags,move_type,ducked,ducking,duck_amount,duck_speed,stamina,surface_friction,max_speed,fall_velocity,old_buttons,ladder_n_x,ladder_n_y,ladder_n_z,origin_x_bits,origin_y_bits,origin_z_bits,vel_x_bits,vel_y_bits,vel_z_bits,basevel_x_bits,basevel_y_bits,basevel_z_bits,pitch_bits,yaw_bits,roll_bits,duck_amount_bits,duck_speed_bits,stamina_bits,surface_friction_bits,max_speed_bits,fall_velocity_bits,ladder_n_x_bits,ladder_n_y_bits,ladder_n_z_bits,health,velocity_modifier,weapon");
	char row[1024];
	for (int i = 0; i < g_Rows.Length; i++)
	{
		g_Rows.GetString(i, row, sizeof(row));
		f.WriteLine(row);
	}
	delete f;

	Format(path, sizeof(path), "%s/meta.toml", dir);
	f = OpenFile(path, "w");
	f.WriteLine("format = 1");
	char build[64] = "unknown";
	File inf = OpenFile("steam.inf", "r");
	if (inf != null)
	{
		char line[128];
		while (inf.ReadLine(line, sizeof(line)))
		{
			TrimString(line);
			if (StrContains(line, "PatchVersion=") == 0)
			{
				strcopy(build, sizeof(build), line[13]);
			}
			else if (StrContains(line, "ServerVersion=") == 0)
			{
				Format(build, sizeof(build), "%s (server %s)", build, line[14]);
			}
		}
		delete inf;
	}
	f.WriteLine("build = \"%s\"", build);
	GetCurrentMap(v, sizeof(v));
	f.WriteLine("map = \"%s\"", v);
	g_MapHash.GetString(v, sizeof(v));
	f.WriteLine("map_hash = \"%s\"", v);
	f.WriteLine("tickrate = %d", RoundToNearest(1.0 / GetTickInterval()));
	g_Mode.GetString(v, sizeof(v));
	f.WriteLine("mode = \"%s\"", v);
	f.WriteLine("sourcemod = \"%s\"", SOURCEMOD_VERSION);
	ConVar mm = FindConVar("metamod_version");
	v = "";
	if (mm != null)
	{
		mm.GetString(v, sizeof(v));
	}
	f.WriteLine("metamod = \"%s\"", v);
	f.WriteLine("plugin = \"csmove_capture %s\"", PLUGIN_VERSION);
	f.WriteLine("injection = \"%s, OnPlayerRunCmd override\"", UseBot() ? "bot" : "human client");
	int active = GetEntPropEnt(g_Bot, Prop_Send, "m_hActiveWeapon");
	v = "none";
	if (active != -1)
	{
		GetEntityClassname(active, v, sizeof(v));
	}
	f.WriteLine("weapon = \"%s\"", v);
	f.WriteLine("weapon_max_speed = 250");
	ReplaceString(g_Unavailable, sizeof(g_Unavailable), ";", ",");
	f.WriteLine("unavailable_props = \"%s\"", g_Unavailable);
	f.WriteLine("");
	f.WriteLine("[cvars]");
	for (int i = 0; i < sizeof(g_Cvars); i++)
	{
		ConVar c = FindConVar(g_Cvars[i]);
		if (c != null)
		{
			c.GetString(v, sizeof(v));
			f.WriteLine("%s = \"%s\"", g_Cvars[i], v);
		}
	}
	delete f;

	Format(path, sizeof(path), "%s/scenario.toml", dir);
	f = OpenFile(path, "w");
	f.WriteLine("scenario = \"%s\"", g_Name);
	f.WriteLine("requested_origin = \"%.6f %.6f %.6f\"", g_Origin[0], g_Origin[1], g_Origin[2]);
	f.WriteLine("requested_yaw = %.6f", g_Yaw);
	f.WriteLine("settle = %d", g_Settle);
	f.WriteLine("geometry = \"%s\"", StrEqual(g_ScenarioMap, "csmove_capture") ? "testlevel" : g_ScenarioMap);
	delete f;

	PrintToServer("[csmove] %s: wrote %d rows", g_Name, g_Rows.Length);
	g_State = State_Cooldown;
	g_Wait = 0;
}
