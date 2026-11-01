@echo off
rem One-time setup: creates .venv, installs dependencies, adds the desktop shortcut.
cd /d "%~dp0"
where py >nul 2>nul && (set "PY=py -3.12") || (set "PY=python")
if not exist ".venv\Scripts\python.exe" (
    %PY% -m venv .venv || (echo Python 3.12+ is required. & pause & exit /b 1)
)
".venv\Scripts\python.exe" -m pip install --upgrade pip
".venv\Scripts\python.exe" -m pip install -r requirements.txt || (pause & exit /b 1)
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\create_desktop_shortcut.ps1
where agy >nul 2>nul || if not exist "%LOCALAPPDATA%\agy\bin\agy.exe" (
    echo.
    echo NOTE: AGY CLI not found. SSH and terminals work, but the AI chat needs it:
    echo   PowerShell:  irm https://antigravity.google/install.ps1 ^| iex
    echo   or:          winget install Google.AntigravityCLI
    echo   Guide:       https://docs.flutter.dev/ai/antigravity-cli
)
echo.
echo Done. Start it from the "SSH Pro AI" desktop icon or run.cmd.
pause
