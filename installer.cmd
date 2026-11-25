@echo off
rem Build the Windows installer: target\installer\SSH-Pro-AI-Setup-<version>.exe
rem Needs everything build.cmd needs, plus Inno Setup 6:  winget install JRSoftware.InnoSetup
setlocal
cd /d "%~dp0"
call "%~dp0build.cmd" || exit /b 1

set "ISCC="
for %%P in ("%LOCALAPPDATA%\Programs\Inno Setup 6\ISCC.exe" "%ProgramFiles(x86)%\Inno Setup 6\ISCC.exe" "%ProgramFiles%\Inno Setup 6\ISCC.exe") do (
    if exist %%P set "ISCC=%%~P"
)
if not defined ISCC (
    echo Inno Setup 6 not found. Install it: winget install JRSoftware.InnoSetup
    exit /b 1
)

rem version = "x.y.z" from Cargo.toml
for /f "tokens=2 delims==" %%V in ('findstr /b /c:"version" Cargo.toml') do set "VERSION=%%~V"
set "VERSION=%VERSION: =%"
set "VERSION=%VERSION:"=%"

"%ISCC%" /Q /DAppVersion=%VERSION% installer\sshpro.iss || exit /b 1
echo.
echo Installer: %~dp0target\installer\SSH-Pro-AI-Setup-%VERSION%.exe
