@echo off
set "FLOATBOARD_EXE=%~dp0releases\v0.9.0\CodexFloatboard\CodexFloatboard.exe"
if not exist "%FLOATBOARD_EXE%" (
    echo Stable release not found. Extract v0.9.0 into releases\v0.9.0 first.
    pause
    exit /b 1
)
start "" "%FLOATBOARD_EXE%"
