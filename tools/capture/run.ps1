# Run one batch of scenarios on a local dedicated server and import the captures (game-plan.md ?9.6).
#
#   powershell -File tools\capture\run.ps1 -Server D:\Programs\csgo_ds -Tickrate 64 -Mode vanilla -Out data\captures\raw\run1
#
# Starts srcds on a LAN port with the capture map, connects the local CS:GO client (whose input the
# plugin replaces), lets csmove_capture run `list_<mode>_<tickrate>.txt` and quit, then imports the
# results with `compare import`. `-Bot` drives a bot instead (rig debugging only).
# `-MapFile <file.bsp>` runs on another map (copied to the server and the client), for scenarios made
# with `compare scenarios --bsp`. KZ modes also load GOKZ's jumpstats, whose reports land in the
# client console log, saved next to the captures as client_console.log.
param(
    [Parameter(Mandatory)] [string] $Server,
    [Parameter(Mandatory)] [int] $Tickrate,
    [string] $Mode = 'vanilla',
    [Parameter(Mandatory)] [string] $Out,
    [string] $List = '',
    [string] $MapFile = '',
    [int] $TimeoutMinutes = 60,
    [switch] $Bot,
    [string] $Game = 'D:\SteamLibrary\steamapps\common\Counter-Strike Global Offensive'
)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path "$PSScriptRoot\..\..").Path
$csgo = Join-Path $Server 'csgo'
$data = Join-Path $csgo 'addons\sourcemod\data\csmove'
if ($List -eq '') { $List = "list_${Mode}_${Tickrate}.txt" }

# KZ modes come from GOKZ, which must not be loaded for vanilla runs.
$plugins = Join-Path $csgo 'addons\sourcemod\plugins'
$kzOff = Join-Path $plugins 'disabled\gokz'
$kzFiles = @('gokz-core', 'gokz-mode-vanilla', 'gokz-mode-simplekz', 'gokz-mode-kztimer', 'gokz-jumpstats', 'movementapi')
function Set-Gokz([bool] $on) {
    foreach ($f in $kzFiles) {
        $live = Join-Path $plugins "$f.smx"
        $off = Join-Path $kzOff "$f.smx"
        if ($on -and (Test-Path $off)) { Move-Item -Force $off $live }
        if (-not $on -and (Test-Path $live)) { Move-Item -Force $live $off }
    }
}
Set-Gokz ($Mode -ne 'vanilla')

$map = 'csmove_capture'
if ($MapFile -ne '') {
    $map = [IO.Path]::GetFileNameWithoutExtension($MapFile)
    Copy-Item $MapFile (Join-Path $csgo "maps\$map.bsp") -Force
    Copy-Item $MapFile (Join-Path $Game "csgo\maps\$map.bsp") -Force
}
$clientLog = Join-Path $Game 'csgo\console.log'
if (Test-Path $clientLog) { Remove-Item $clientLog -ErrorAction SilentlyContinue }

$results = Join-Path $data 'results'
if (Test-Path $results) { Remove-Item -Recurse -Force $results }
New-Item -ItemType Directory -Force $results | Out-Null

Set-Content -Encoding ascii (Join-Path $csgo 'cfg\csmove_run.cfg') @"
csmove_mode $Mode
csmove_quit_when_done 1
csmove_autobatch $List
csmove_target $(if ($Bot) { 'bot' } else { 'human' })
"@
$srcdsArgs = @('-game', 'csgo', '-console', '-insecure', '-norestart', '-nohltv', '-condebug', '-port', '27115',
    '-tickrate', "$Tickrate", '+game_type', '0', '+game_mode', '1', '+map', $map)
$log = Join-Path $csgo 'console.log'
if (Test-Path $log) { Remove-Item $log }
Write-Host "srcds $($srcdsArgs -join ' ')"
$p = Start-Process -FilePath (Join-Path $Server 'srcds.exe') -ArgumentList $srcdsArgs -WorkingDirectory $Server -PassThru -WindowStyle Minimized
if (-not $Bot) {
    # The driven player is a real client on loopback (bots crouch-jump on their own). Its input is
    # replaced by the plugin, so nobody needs to touch it.
    Start-Sleep -Seconds 15
    # Launched directly (steam_appid.txt is present): `steam -applaunch` asks before passing arguments.
    # The client defers `+connect` at startup and never runs it, so the connect goes through its
    # console port once it is up. Loopback (127.0.0.1) never answers on this build; the LAN IP does.
    $clientArgs = @('-steam', '-insecure', '-novid', '-windowed', '-noborder', '-w', '640', '-h', '480',
        '-netconport', '29015', '-condebug', '+cl_cmdrate', "$Tickrate", '+cl_updaterate', "$Tickrate", '+rate', '786432')
    Start-Process -FilePath (Join-Path $Game 'csgo.exe') -ArgumentList $clientArgs -WorkingDirectory $Game | Out-Null
    $ip = (Get-NetIPAddress -AddressFamily IPv4 | Where-Object {
            $_.IPAddress -notlike '127.*' -and $_.IPAddress -notlike '169.254.*' } | Select-Object -First 1).IPAddress
    $netcon = $null
    for ($i = 0; $i -lt 60 -and $null -eq $netcon; $i++) {
        Start-Sleep -Seconds 2
        try { $netcon = New-Object Net.Sockets.TcpClient('127.0.0.1', 29015) } catch { }
    }
    if ($null -eq $netcon) { throw 'CS:GO client console port never opened' }
    Start-Sleep -Seconds 10
    $w = New-Object IO.StreamWriter($netcon.GetStream())
    $w.Write("connect ${ip}:27115`n")
    $w.Flush()
}
$finished = $p.WaitForExit($TimeoutMinutes * 60 * 1000)
if (-not $Bot) {
    if ($netcon) { $netcon.Close() }
    Get-Process csgo -ErrorAction SilentlyContinue | Stop-Process -Force
}
Set-Gokz $false
if (-not $finished) {
    Stop-Process -Id $p.Id -Force
    throw "srcds did not finish within $TimeoutMinutes minutes; see $log"
}
Select-String -Path $log -Pattern '\[csmove\]' | ForEach-Object { $_.Line } | Select-Object -Last 5

Push-Location $repo
& cargo run -q --release -p compare -- import $results $Out
if (Test-Path $clientLog) { Copy-Item $clientLog (Join-Path $Out 'client_console.log') -Force }
$ok = $LASTEXITCODE
Pop-Location
if ($ok -ne 0) { throw 'import failed' }
