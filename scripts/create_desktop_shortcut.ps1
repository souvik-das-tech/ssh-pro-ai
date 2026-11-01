# Creates an "SSH Pro AI" icon on the Desktop. Double-click = the app opens.
# Run:  powershell -ExecutionPolicy Bypass -File scripts\create_desktop_shortcut.ps1  (setup.cmd does this)
$ErrorActionPreference = 'Stop'
$app = Split-Path -Parent $PSScriptRoot          # repo root
$pythonw = Join-Path $app '.venv\Scripts\pythonw.exe'
if (-not (Test-Path $pythonw)) { throw "Virtual environment not found: $pythonw (run setup.cmd)" }

$desktop = [Environment]::GetFolderPath('Desktop')   # follows OneDrive-redirected Desktops
$link = Join-Path $desktop 'SSH Pro AI.lnk'
$shell = New-Object -ComObject WScript.Shell
$shortcut = $shell.CreateShortcut($link)
$shortcut.TargetPath = $pythonw
$shortcut.Arguments = '-m sshpro'
$shortcut.WorkingDirectory = $app
$shortcut.IconLocation = (Join-Path $app 'sshpro.ico') + ',0'
$shortcut.Description = 'SSH Pro AI - all servers, terminals and the AGY ops bot in one window'
$shortcut.Save()
Write-Output "Created $link"
