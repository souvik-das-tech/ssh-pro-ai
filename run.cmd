@echo off
rem Launch SSH Pro AI without a console window.
cd /d "%~dp0"
if not exist ".venv\Scripts\pythonw.exe" (
    echo Virtual environment missing. Run setup.cmd first.
    pause
    exit /b 1
)
start "" ".venv\Scripts\pythonw.exe" -m sshpro
