@echo off
set "FLOATBOARD_EXE=%~dp0releases\v0.8.1\CodexFloatboard\CodexFloatboard.exe"
if not exist "%FLOATBOARD_EXE%" (
    echo Stable release not found. Extract v0.8.1 into releases\v0.8.1 first.
    pause
    exit /b 1
)
start "" "%FLOATBOARD_EXE%"
