@echo off
rem Build the Rust desktop app: target\release\sshpro.exe (+ WebView2Loader.dll next to it).
rem Needs Rust (rustup, GNU toolchain) and a MinGW gcc for the C parts (ring, SQLite), e.g.
rem   winget install BrechtSanders.WinLibs.POSIX.UCRT
setlocal
cd /d "%~dp0"
where gcc >nul 2>&1
if errorlevel 1 (
    for /d %%D in ("%LOCALAPPDATA%\Microsoft\WinGet\Packages\BrechtSanders.WinLibs*") do (
        if exist "%%D\mingw64\bin\gcc.exe" set "PATH=%%D\mingw64\bin;%PATH%"
    )
)
where gcc >nul 2>&1
if errorlevel 1 (
    echo MinGW gcc not found. Install it: winget install BrechtSanders.WinLibs.POSIX.UCRT
    exit /b 1
)
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
cargo build --release %*
if errorlevel 1 exit /b 1
echo.
echo Built: %~dp0target\release\sshpro.exe
