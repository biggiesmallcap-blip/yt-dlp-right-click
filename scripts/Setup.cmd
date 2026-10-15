@echo off
setlocal
title yt-dlp Right Click setup

set "POWERSHELL_EXE=%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe"
if not exist "%POWERSHELL_EXE%" set "POWERSHELL_EXE=powershell.exe"

"%POWERSHELL_EXE%" -NoProfile -ExecutionPolicy Bypass -File "%~dp0setup.ps1" %*
set "EXIT_CODE=%ERRORLEVEL%"

echo.
if not "%EXIT_CODE%"=="0" (
  echo Setup did not finish. The message above says what went wrong.
)
pause
exit /b %EXIT_CODE%
