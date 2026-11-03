@echo off
set "FLOATBOARD_EXE=%LOCALAPPDATA%\Programs\CodexFloatboard\CodexFloatboard.exe"
if not exist "%FLOATBOARD_EXE%" (
    echo Installed stable release not found. Install the GitHub portable release into LOCALAPPDATA\Programs\CodexFloatboard first.
    pause
    exit /b 1
)
start "" "%FLOATBOARD_EXE%"
