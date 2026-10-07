# Prepare a CS:GO dedicated server for captures (game-plan.md §9.2, §9.4).
#
#   powershell -File tools\capture\setup.ps1 -Server D:\Programs\csgo_ds -Mods D:\Programs\csgo_mods `
#       -Sdk D:\Programs\csgo_sdk\bin -Game "D:\SteamLibrary\steamapps\common\Counter-Strike Global Offensive"
#
# Installs MetaMod:Source and SourceMod (from the zips in -Mods), compiles and installs csmove_capture,
# builds the test-level map from `compare map` with the SDK compilers, and copies the scenario ladder.
# Nothing here touches the repository except data/ (ignored output).
param(
    [Parameter(Mandatory)] [string] $Server,
    [Parameter(Mandatory)] [string] $Mods,
    [Parameter(Mandatory)] [string] $Sdk,
    [Parameter(Mandatory)] [string] $Game
)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path "$PSScriptRoot\..\..").Path
$csgo = Join-Path $Server 'csgo'
$sm = Join-Path $csgo 'addons\sourcemod'

function Invoke-Cargo { & cargo.exe @args; if ($LASTEXITCODE -ne 0) { throw "cargo $args failed" } }

# 1. MetaMod:Source + SourceMod.
$mmZip = Get-ChildItem $Mods -Filter 'mmsource-*-windows.zip' | Sort-Object Name | Select-Object -Last 1
$smZip = Get-ChildItem $Mods -Filter 'sourcemod-*-windows.zip' | Sort-Object Name | Select-Object -Last 1
Expand-Archive -Force $mmZip.FullName $csgo
Expand-Archive -Force $smZip.FullName $csgo
# The metamod.vdf in the zip points at addons/metamod/bin; CS:GO loads it from csgo/addons.
Write-Host "installed $($mmZip.Name) and $($smZip.Name)"

# Map-changing and voting plugins could move the server off the capture map mid-batch.
$disabled = Join-Path $sm 'plugins\disabled'
foreach ($p in 'nextmap.smx', 'basevotes.smx', 'funvotes.smx', 'mapchooser.smx', 'rockthevote.smx', 'nominations.smx') {
    $f = Join-Path $sm "plugins\$p"
    if (Test-Path $f) { Move-Item -Force $f $disabled }
}

# 2. The capture plugin.
$scripting = Join-Path $sm 'scripting'
Copy-Item "$PSScriptRoot\csmove_capture.sp" $scripting -Force
Push-Location $scripting
& .\spcomp.exe csmove_capture.sp "-o..\plugins\csmove_capture.smx" -iinclude
$ok = $LASTEXITCODE
Pop-Location
if ($ok -ne 0) { throw 'spcomp failed' }

# 2b. GOKZ (the core, the three modes and jumpstats) and MovementAPI, for the per-mode scenarios (plan §9.5
#     M-*). Installed disabled; run.ps1 enables them for KZTimer/SimpleKZ runs only. GOKZ's surf fix
#     plugin is deliberately left out: it changes collision.
$gokz = Join-Path $Mods 'gokz'
$mapi = Join-Path $Mods 'movementapi'
if ((Test-Path $gokz) -and (Test-Path $mapi)) {
    $kz = Join-Path $sm 'plugins\disabled\gokz'
    New-Item -ItemType Directory -Force $kz | Out-Null
    foreach ($p in 'gokz-core', 'gokz-mode-vanilla', 'gokz-mode-simplekz', 'gokz-mode-kztimer', 'gokz-jumpstats') {
        Copy-Item (Join-Path $gokz "addons\sourcemod\plugins\$p.smx") $kz -Force
    }
    Copy-Item (Join-Path $mapi 'addons\sourcemod\plugins\movementapi.smx') $kz -Force
    Copy-Item (Join-Path $mapi 'addons\sourcemod\gamedata\*') (Join-Path $sm 'gamedata') -Force
    Copy-Item (Join-Path $gokz 'addons\sourcemod\gamedata\gokz-core.games.txt') (Join-Path $sm 'gamedata') -Force
    Copy-Item (Join-Path $gokz 'addons\sourcemod\translations\*') (Join-Path $sm 'translations') -Recurse -Force
    Copy-Item (Join-Path $gokz 'cfg\sourcemod\gokz') (Join-Path $csgo 'cfg\sourcemod') -Recurse -Force
    Write-Host 'installed GOKZ core + modes and MovementAPI (disabled until a KZ run)'
}

# 3. The map, generated from the test level and compiled with the SDK tools.
Push-Location $repo
Invoke-Cargo run -q --release -p compare -- map data/map/csmove_capture.vmf
Pop-Location
$vmf = Join-Path $repo 'data\map\csmove_capture'
Push-Location $Sdk
& .\vbsp.exe -novconfig -game "$Game\csgo" $vmf | Out-Null
if ($LASTEXITCODE -ne 0) { Pop-Location; throw 'vbsp failed' }
& .\vvis.exe -fast -novconfig -game "$Game\csgo" $vmf | Out-Null
& .\vrad.exe -fast -novconfig -game "$Game\csgo" $vmf | Out-Null
Pop-Location
Copy-Item "$vmf.bsp" (Join-Path $csgo 'maps\csmove_capture.bsp') -Force
# The client gets the same map so it never downloads it (our own generated map, not a Valve asset).
Copy-Item "$vmf.bsp" (Join-Path $Game 'csgo\maps\csmove_capture.bsp') -Force
# The server advertises the bot nav mesh too and the client waits for it, but uploads are off.
$nav = Join-Path $csgo 'maps\csmove_capture.nav'
if (Test-Path $nav) { Copy-Item $nav (Join-Path $Game 'csgo\maps\csmove_capture.nav') -Force }
$hash = (Get-FileHash "$vmf.bsp" -Algorithm SHA256).Hash.Substring(0, 16).ToLower()

# 4. Server config. Competitive game mode supplies the movement cvars; the plugin dumps them anyway.
$cfg = @"
sv_lan 1
sv_cheats 0
sv_hibernate_when_empty 0
mp_do_warmup_period 0
mp_warmuptime 0
mp_freezetime 0
mp_roundtime 60
mp_roundtime_defuse 60
mp_roundtime_hostage 60
mp_ignore_round_win_conditions 1
mp_autoteambalance 0
mp_limitteams 0
mp_autokick 0
bot_quota 1
bot_quota_mode normal
bot_join_after_player 0
bot_join_team t
bot_dont_shoot 1
bot_chatter off
bot_knives_only 1
bot_difficulty 0
csmove_map_hash $hash
"@
Set-Content -Encoding ascii (Join-Path $csgo 'cfg\server.cfg') $cfg
Set-Content -Encoding ascii (Join-Path $csgo 'cfg\gamemode_competitive_server.cfg') $cfg

# 5. The scenario ladder.
$scen = Join-Path $sm 'data\csmove\scenarios'
if (Test-Path $scen) { Remove-Item -Recurse -Force $scen }
Push-Location $repo
Invoke-Cargo run -q --release -p compare -- scenarios $scen
Pop-Location
Write-Host "server ready: map hash $hash"
